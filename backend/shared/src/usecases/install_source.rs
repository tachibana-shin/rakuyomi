use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    model::{InstallOutcome, SourceId},
    settings::SourceList,
    source_manager::SourceManager,
    usecases::fetch_source_list::fetch_source_list,
    usecases::resolve_source_list::{resolve_source_list, source_list_key},
};

/// Installs the source identified by `source_id` from the source list
/// named by `source_of_source`.
///
/// For keiyoushi extensions, `languages` restricts which bundled sources
/// of a multi-source APK get registered. Passing `None` for a multi-source
/// APK installs nothing and returns
/// [`InstallOutcome::SelectionRequired`] with the bundled languages so the
/// caller can ask the user for a selection; a non-empty selection must
/// only contain languages the APK actually bundles.
pub async fn install_source(
    source_manager: &mut SourceManager,
    arc_manager: &Arc<Mutex<SourceManager>>,
    source_lists: &[SourceList],
    source_id: SourceId,
    source_of_source: String,
    languages: Option<Vec<String>>,
) -> Result<InstallOutcome> {
    let (source_list, source_list_item, source_of_source) = stream::iter(
        source_lists
            .iter()
            .filter(|list| source_list_key(list) == source_of_source),
    )
    .then(|source_list| async move {
        let resolved_list = resolve_source_list(source_list).await;
        let key = source_list_key(source_list);

        let client = crate::tls::client_builder()
            .build()
            .with_context(|| "failed to create HTTP client".to_string())?;
        let value = fetch_source_list(&client, &resolved_list).await?;

        // Try both formats
        let mut source_list_items = if value.is_array() {
            serde_json::from_value::<Vec<SourceListItem>>(value)?
        } else if let Some(arr) = value.get("sources").and_then(|v| v.as_array()) {
            serde_json::from_value::<Vec<SourceListItem>>(Value::Array(arr.clone()))?
        } else {
            anyhow::bail!(
                "unexpected JSON format for source list at {}: {}",
                resolved_list,
                value
            );
        };

        // Match the id expansion `list_available_sources` applies: entries
        // of a keiyoushi package published with several languages are named
        // `<pkg>:<lang>`.
        if source_list.source_type == crate::settings::SourceListType::Keiyoushi {
            let mut entries = source_list_items
                .iter()
                .map(|item| {
                    (
                        item.id.value().to_string(),
                        item.item
                            .get("lang")
                            .and_then(|lang| lang.as_str())
                            .map(String::from),
                    )
                })
                .collect::<Vec<_>>();
            crate::usecases::fetch_source_list::expand_keiyoushi_ids(&mut entries);
            for (item, (id, _)) in source_list_items.iter_mut().zip(entries) {
                item.id = SourceId::new(id);
            }
        }

        anyhow::Ok((source_list, source_list_items, key))
    })
    .try_collect::<Vec<_>>()
    .await?
    .into_iter()
    .flat_map(|(source_list, items, key)| {
        items
            .into_iter()
            .map(|item| (source_list.clone(), item, key.clone()))
            .collect::<Vec<_>>()
    })
    .find(|(_, item, _)| item.id == source_id)
    .ok_or_else(|| anyhow!("couldn't find source with id '{:?}'", source_id))?;

    let client = crate::tls::client_builder().build()?;

    match source_list.source_type {
        crate::settings::SourceListType::LnReader => {
            // LNReader plugin: the index publishes an absolute URL to the
            // compiled `.js` file.
            let url = source_list_item
                .url
                .context("LNReader source list item is missing a `url`")?;
            let plugin_content = client.get(url).send().await?.bytes().await?;
            source_manager.install_lnreader_source(
                &source_id,
                plugin_content,
                source_of_source,
                arc_manager,
            )?;
        }
        crate::settings::SourceListType::Mangayomi => {
            // MangaYomi extension: the index entry itself carries the
            // `sourceCodeUrl` of the `.dart`/`.js` file; the whole entry is
            // stored as the extension metadata (its `sourceCodeLanguage`
            // decides the stored suffix). Anime extensions are rejected in
            // `SourceManager::install_mangayomi_source`.
            let code_url = source_list_item
                .source_code_url
                .context("MangaYomi source list item is missing a `sourceCodeUrl`")?;
            let code = client.get(code_url).send().await?.bytes().await?;
            // `#[serde(flatten)]` consumes the `id` key into its own field, so
            // it is missing from `item`; put it back so the stored metadata
            // (and `ExtensionMeta::from_value`) sees it.
            let mut metadata_obj = source_list_item.item.clone();
            metadata_obj.insert(
                "id".to_string(),
                serde_json::json!(source_list_item.id.value()),
            );
            let metadata = serde_json::to_vec(&metadata_obj)
                .context("failed to serialise MangaYomi extension metadata")?;
            source_manager.install_mangayomi_source(
                &source_id,
                code,
                metadata,
                source_of_source,
                arc_manager,
            )?;
        }
        crate::settings::SourceListType::Keiyoushi => {
            // Keiyoushi extension: the index publishes the release APK URL.
            // Anime-only entries (`isNsfw` is irrelevant here) and other
            // non-manga packages are rejected by the extension VM.
            let apk_url = source_list_item
                .file
                .context("Keiyoushi source list item is missing an `apk` URL")?;
            let apk_content = client.get(apk_url).send().await?.bytes().await?;

            // A keiyoushi APK bundles one source per language; installing a
            // multi-source APK without a selection would register every
            // language, so it first asks which languages to install.
            let probe = crate::source::keiyoushi::probe_keiyoushi_apk(&apk_content)?;
            if probe.sources.len() == 1 {
                source_manager.install_keiyoushi_source(
                    &source_id,
                    apk_content,
                    source_of_source,
                    arc_manager,
                    None,
                )?;
            } else if let Some(languages) = languages {
                let bundled = probe
                    .sources
                    .iter()
                    .map(|(_, lang, _)| lang.as_str())
                    .collect::<std::collections::HashSet<_>>();
                if languages.is_empty() {
                    anyhow::bail!("keiyoushi language selection is empty");
                }
                if let Some(unknown) = languages
                    .iter()
                    .find(|lang| !bundled.contains(lang.as_str()))
                {
                    anyhow::bail!(
                        "keiyoushi extension does not bundle the selected language '{unknown}'"
                    );
                }

                source_manager.install_keiyoushi_source(
                    &source_id,
                    apk_content,
                    source_of_source,
                    arc_manager,
                    Some(&languages),
                )?;
            } else {
                let mut bundled_languages = probe
                    .sources
                    .iter()
                    .map(|(_, lang, _)| lang.clone())
                    .collect::<Vec<_>>();
                bundled_languages.sort();
                bundled_languages.dedup();
                let name = source_list_item
                    .item
                    .get("name")
                    .and_then(|name| name.as_str())
                    .unwrap_or(source_id.value())
                    .to_string();

                return Ok(InstallOutcome::SelectionRequired {
                    name,
                    languages: bundled_languages,
                });
            }
        }
        crate::settings::SourceListType::Aidoku => {
            let file = source_list_item
                .file
                .context("source list item is missing a `file`")?;
            let aix_url = if file.starts_with("sources/") {
                source_list.url.join(&file).unwrap()
            } else {
                source_list.url.join(&format!("sources/{}", file)).unwrap()
            };
            let aix_content = client.get(aix_url).send().await?.bytes().await?;

            source_manager.install_source(
                &source_id,
                aix_content,
                source_of_source,
                arc_manager,
            )?;
        }
    }

    Ok(InstallOutcome::Installed)
}

#[derive(Deserialize)]
struct SourceListItem {
    /// MangaYomi index entries use numeric ids; both are stringified.
    /// Keiyoushi index entries (protobuf-decoded and JSON alike) name the
    /// field `pkg` instead of `id`.
    #[serde(deserialize_with = "de_source_id", alias = "pkg")]
    id: SourceId,
    /// Aidoku index: file name of the `.aix`, relative to the source list URL.
    #[serde(alias = "downloadURL", alias = "apk")]
    file: Option<String>,
    /// LNReader index: absolute URL of the compiled plugin `.js` file.
    url: Option<String>,
    /// MangaYomi index: absolute URL of the extension `.dart` file.
    #[serde(rename = "sourceCodeUrl")]
    source_code_url: Option<String>,
    /// The raw index entry, stored as the extension metadata.
    #[serde(flatten)]
    item: serde_json::Map<String, Value>,
}

fn de_source_id<'de, D>(deserializer: D) -> Result<SourceId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(SourceId::new(s)),
        Value::Number(n) => Ok(SourceId::new(n.to_string())),
        _ => Err(D::Error::custom("source id must be a string or a number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_list_item_parses_aidoku_format() {
        let json = r#"{"id":"en.aquamanga","name":"Aqua Manga","version":1,"downloadURL":"sources/en.aquamanga-v1.aix"}"#;
        let item: SourceListItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.id.value(), "en.aquamanga");
        assert_eq!(item.file.as_deref(), Some("sources/en.aquamanga-v1.aix"));
        assert_eq!(item.url, None);
    }

    #[test]
    fn test_source_list_item_parses_lnreader_format() {
        let json = r#"{"id":"royalroad","name":"Royal Road","version":"2.3.1","url":"https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/english/royalroad.js"}"#;
        let item: SourceListItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.id.value(), "royalroad");
        assert_eq!(item.file, None);
        assert!(item.url.unwrap().ends_with("royalroad.js"));
    }

    #[test]
    fn test_source_list_item_keiyoushi_uses_pkg_as_id() {
        // The protobuf decoder in `fetch_source_list` publishes keiyoushi
        // entries with `pkg` (not `id`); the install path must accept it.
        let json = r#"{"name":"MoeTruyen","pkg":"eu.kanade.tachiyomi.extension.vi.moetruyen","apk":"https://example.com/tachiyomi-vi.moetruyen-v1.6.8.apk","lang":"vi","version":"1.6.8"}"#;
        let item: SourceListItem = serde_json::from_str(json).unwrap();
        assert_eq!(
            item.id.value(),
            "eu.kanade.tachiyomi.extension.vi.moetruyen"
        );
        assert_eq!(
            item.file.as_deref(),
            Some("https://example.com/tachiyomi-vi.moetruyen-v1.6.8.apk")
        );
        assert_eq!(item.item.get("pkg"), None, "flatten consumes the pkg key");
    }

    #[test]
    fn test_source_list_item_mangayomi_item_keeps_id() {
        // `#[serde(flatten)]` pulls the `id` key out of `item`; rebuilding the
        // metadata in the install path must put it back (the MangaYomi
        // backend rejects metadata without an id).
        let json = r#"{"id":524070078,"name":"Madara Fixture","lang":"en","version":"1.2.0","sourceCodeUrl":"https://example.com/madara.dart"}"#;
        let item: SourceListItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.id.value(), "524070078");
        assert_eq!(
            item.source_code_url.as_deref(),
            Some("https://example.com/madara.dart")
        );
        assert_eq!(item.item.get("id"), None, "flatten consumes the id key");

        let mut metadata_obj = item.item.clone();
        metadata_obj.insert("id".to_string(), serde_json::json!(item.id.value()));
        let metadata = serde_json::to_string(&metadata_obj).unwrap();
        assert!(
            metadata.contains("\"id\":\"524070078\""),
            "metadata: {metadata}"
        );
    }
}
