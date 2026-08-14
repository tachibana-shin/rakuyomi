use anyhow::{Context, Result};
use futures::{stream, StreamExt, TryStreamExt};

use crate::model::SourceInformation;
use crate::settings::SourceList;
use crate::usecases::fetch_source_list::fetch_source_list;
use crate::usecases::resolve_source_list::{resolve_source_list, source_list_key};
use serde_json::Value;

pub async fn list_available_sources(
    source_lists: Vec<SourceList>,
) -> Result<Vec<SourceInformation>> {
    let mut source_informations: Vec<SourceInformation> = stream::iter(source_lists)
        .then(|source_list| async move {
            let resolved_list = resolve_source_list(&source_list).await;
            let key = source_list_key(&source_list);

            let client = crate::tls::client_builder()
                .build()
                .with_context(|| "failed to create HTTP client".to_string())?;
            let value = fetch_source_list(&client, &resolved_list).await?;

            // Try both formats
            let mut sources = if value.is_array() {
                serde_json::from_value::<Vec<SourceInformation>>(normalize_ids(value))?
            } else if let Some(arr) = value.get("sources").and_then(|v| v.as_array()) {
                serde_json::from_value::<Vec<SourceInformation>>(normalize_ids(Value::Array(
                    arr.clone(),
                )))?
            } else {
                anyhow::bail!(
                    "unexpected JSON format for source list at {}: {}",
                    resolved_list,
                    value
                );
            };

            for src in &mut sources {
                src.source_of_source = Some(key.clone());
            }

            Ok(sources)
        })
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect();

    source_informations.sort_by_key(|source| source.name.clone());

    Ok(source_informations)
}

/// MangaYomi index entries publish numeric ids (`"id": 638504049`) and
/// keiyoushi index entries publish the extension package as `pkg`; the
/// shared `SourceInformation` model expects a string id, so both are
/// normalised here before deserialisation.
fn normalize_ids(value: Value) -> Value {
    let Value::Array(items) = value else {
        return value;
    };
    Value::Array(
        items
            .into_iter()
            .map(|item| {
                let Value::Object(mut map) = item else {
                    return item;
                };
                if let Some(Value::Number(n)) = map.get("id") {
                    if let Some(s) = n.as_i64().map(|n| n.to_string()) {
                        map.insert("id".to_string(), Value::String(s));
                    }
                } else if let Some(Value::String(pkg)) = map.get("pkg") {
                    map.insert("id".to_string(), Value::String(pkg.clone()));
                }
                Value::Object(map)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ids_stringifies_mangayomi_numeric_ids() {
        let value = serde_json::json!([
            {"id": 638504049, "name": "Madara Fixture", "lang": "en"},
            {"id": "royalroad", "name": "Royal Road"}
        ]);
        let normalized = normalize_ids(value);
        let items = normalized.as_array().unwrap();
        assert_eq!(items[0]["id"], serde_json::json!("638504049"));
        assert_eq!(items[1]["id"], serde_json::json!("royalroad"));
    }

    #[test]
    fn normalize_ids_uses_keiyoushi_pkg_as_id() {
        let value = serde_json::json!([
            {"name": "MangaPill", "pkg": "eu.kanade.tachiyomi.en.mangapill", "apk": "https://github.com/keiyoushi/extensions/releases/download/v1.4.x/tachiyomi-en.mangapill-v1.4.x.apk", "lang": "en", "code": 199, "version": "1.4.199", "hasReadme": true, "isNsfw": true},
            {"id": "en.aquamanga", "name": "Aqua Manga"}
        ]);
        let normalized = normalize_ids(value);
        let items = normalized.as_array().unwrap();
        assert_eq!(
            items[0]["id"],
            serde_json::json!("eu.kanade.tachiyomi.en.mangapill")
        );
        assert_eq!(items[0]["lang"], serde_json::json!("en"));
        assert_eq!(items[1]["id"], serde_json::json!("en.aquamanga"));
    }
}
