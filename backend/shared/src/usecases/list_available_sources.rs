use anyhow::{Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use url::Url;

use crate::model::SourceInformation;
use serde_json::Value;

/// One entry from a `source_lists` URL, before we know which of the two
/// shapes it is. Both Aidoku's `index.min.json` (already-packaged, has
/// `downloadURL`/`file`) and LNReader's `plugins.min.json` (raw, unpackaged
/// `.js`, has `url`) can appear in the very same list — see
/// `docs/lnreader/REFERENCE.md` §5.1 for why this is detected from the JSON
/// shape itself rather than a separate settings field.
fn looks_like_lnreader_entry(item: &Value) -> bool {
    item.get("downloadURL").is_none() && item.get("file").is_none() && item.get("url").is_some()
}

#[cfg(feature = "lnreader")]
fn lnreader_entry_to_source_information(item: Value) -> Result<SourceInformation> {
    use crate::{model::SourceId, source::packaging};

    let entry: packaging::UpstreamIndexEntry =
        serde_json::from_value(item).context("couldn't parse an LNReader source list entry")?;

    Ok(SourceInformation {
        id: SourceId::new(entry.id),
        name: entry.name,
        version: packaging::encode_version_str(entry.version.as_deref()),
        display_version: entry.version.filter(|version| !version.trim().is_empty()),
        source_of_source: None,
    })
}

pub async fn list_available_sources(
    source_lists: Vec<Url>,
    lnreader_enabled: bool,
) -> Result<Vec<SourceInformation>> {
    let lnreader_mode_on = crate::source::lnreader_mode_enabled(lnreader_enabled);

    let mut source_informations: Vec<SourceInformation> =
        stream::iter(source_lists)
            .then(move |source_list| async move {
                let domain = source_list.domain().unwrap_or("").to_string();

                let client = crate::tls::client_builder()
                    .build()
                    .with_context(|| "failed to create HTTP client".to_string())?;
                let response = client
                    .get(source_list.clone())
                    .send()
                    .await
                    .with_context(|| format!("failed to fetch source list at {}", &source_list))?;

                let value: Value = response
                    .json()
                    .await
                    .with_context(|| format!("failed to parse source list at {}", &source_list))?;

                // Try both container formats: a bare array (LNReader's
                // `plugins.min.json`) or a `{"sources": [...]}` wrapper
                // (Aidoku's `index.min.json`).
                let items: Vec<Value> = if let Value::Array(arr) = value {
                    arr
                } else if let Some(arr) = value.get("sources").and_then(|v| v.as_array()) {
                    arr.clone()
                } else {
                    anyhow::bail!(
                        "unexpected JSON format for source list at {}: {}",
                        &source_list,
                        value
                    );
                };

                let mut sources = Vec::with_capacity(items.len());
                for item in items {
                    if looks_like_lnreader_entry(&item) {
                        if !lnreader_mode_on {
                            log::warn!(
                                "skipping an LNReader-shaped source list entry from {} \
                             — LNReader support is off (Cargo feature not compiled in, \
                             or `lnreader_enabled` is false)",
                                &source_list
                            );
                            continue;
                        }

                        #[cfg(feature = "lnreader")]
                        sources.push(lnreader_entry_to_source_information(item)?);
                    } else {
                        sources.push(serde_json::from_value::<SourceInformation>(item).with_context(
                        || format!("couldn't parse a packaged source list entry at {source_list}"),
                    )?);
                    }
                }

                for src in &mut sources {
                    src.source_of_source = Some(domain.clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_like_lnreader_entry_detects_aidoku_shape() {
        let item = serde_json::json!({
            "id": "en.asurascans",
            "name": "Asura Scans",
            "version": 18,
            "downloadURL": "sources/en.asurascans-v18.aix",
            "languages": ["en"],
        });
        assert!(!looks_like_lnreader_entry(&item));
    }

    #[test]
    fn looks_like_lnreader_entry_detects_aidoku_file_alias_shape() {
        let item = serde_json::json!({
            "id": "some-source",
            "name": "Some Source",
            "version": 1,
            "file": "some-source.aix",
        });
        assert!(!looks_like_lnreader_entry(&item));
    }

    #[test]
    fn looks_like_lnreader_entry_detects_lnreader_shape() {
        let item = serde_json::json!({
            "id": "arnovel",
            "name": "ArNovel",
            "site": "https://ar-no.com/",
            "version": "2.2.0",
            "url": "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/arabic/ArNovel[madara].js",
            "iconUrl": "https://example.com/icon.png",
        });
        assert!(looks_like_lnreader_entry(&item));
    }

    #[cfg(feature = "lnreader")]
    #[test]
    fn lnreader_entry_to_source_information_encodes_version() {
        let item = serde_json::json!({
            "id": "arnovel",
            "name": "ArNovel",
            "site": "https://ar-no.com/",
            "version": "2.2.0",
            "url": "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/arabic/ArNovel[madara].js",
        });

        let info = lnreader_entry_to_source_information(item).unwrap();

        assert_eq!(info.id, crate::model::SourceId::new("arnovel".to_string()));
        assert_eq!(info.name, "ArNovel");
        assert_eq!(info.version, 2_002_000);
        assert_eq!(info.display_version.as_deref(), Some("2.2.0"));
        assert!(info.source_of_source.is_none());
    }

    #[test]
    fn aidoku_source_without_display_version_stays_compatible() {
        let info: SourceInformation = serde_json::from_value(serde_json::json!({
            "id": "en.example",
            "name": "Example",
            "version": 18
        }))
        .unwrap();
        assert_eq!(info.version, 18);
        assert!(info.display_version.is_none());
    }
}
