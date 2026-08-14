use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(not(feature = "lnreader"))]
use anyhow::bail;
use anyhow::{anyhow, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::{model::SourceId, source_manager::SourceManager};

/// Installs `source_id` by matching it against `source_lists`, fetching the
/// matching entry, and handing the result to [`SourceManager::install_source`].
///
/// `source_lists` entries can be either the already-packaged Aidoku shape
/// (`file`/`downloadURL` pointing at a ready `.aix`, downloaded as-is) or
/// LNReader's raw shape (`url` pointing at a compiled `.js`, packaged into an
/// `.aix` on the fly via `packaging::package_plugin_js`) -- see
/// [`SourceListItem`] and `docs/lnreader/REFERENCE.md` §5.1. The LNReader
/// path is only taken when `lnreader_enabled` is `true` and this crate was
/// built with the `lnreader` feature; otherwise it returns an error instead
/// of silently falling back to the Aidoku shape.
///
/// `arc_manager` is only locked for the final, local
/// `SourceManager::install_source` call -- every network fetch above runs
/// unlocked so a slow source list/plugin download doesn't block every other
/// route that needs the source manager.
///
/// Returns an error if no entry in `source_lists` matches `source_id`, if
/// fetching or parsing a source list fails, if the matched entry's `.aix`/JS
/// download fails, or if the final install (write + load) fails.
pub async fn install_source(
    arc_manager: &Arc<Mutex<SourceManager>>,
    source_lists: &[Url],
    source_id: SourceId,
    source_of_source: String,
    lnreader_enabled: bool,
) -> Result<()> {
    // Only actually read inside the `#[cfg(feature = "lnreader")]` branch
    // below -- referenced unconditionally here so a build without the
    // feature doesn't warn about an unused parameter.
    let _ = lnreader_enabled;

    let (source_list, source_list_item, source_of_source) =
        stream::iter(source_lists.iter().filter(|url| {
            let domain = url.domain().unwrap_or("").to_string();
            domain == source_of_source
        }))
        .then(|source_list| async move {
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

            // Try both formats
            let source_list_items = if value.is_array() {
                serde_json::from_value::<Vec<SourceListItem>>(value)?
            } else if let Some(arr) = value.get("sources").and_then(|v| v.as_array()) {
                serde_json::from_value::<Vec<SourceListItem>>(Value::Array(arr.clone()))?
            } else {
                anyhow::bail!(
                    "unexpected JSON format for source list at {}: {}",
                    &source_list,
                    value
                );
            };
            anyhow::Ok((source_list, source_list_items, domain))
        })
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flat_map(|(source_list, items, domain)| {
            items
                .into_iter()
                .map(|item| (source_list.clone(), item, domain.clone()))
                .collect::<Vec<_>>()
        })
        .find(|(_, item, _)| item.id() == &source_id)
        .ok_or_else(|| anyhow!("couldn't find source with id '{:?}'", source_id))?;

    let aix_content: Vec<u8> = match source_list_item {
        SourceListItem::Packaged { file, .. } => {
            let aix_url = if file.starts_with("sources/") {
                source_list
                    .join(&file)
                    .with_context(|| format!("invalid file URL '{file}' in {source_list}"))?
            } else {
                source_list
                    .join(&format!("sources/{file}"))
                    .with_context(|| format!("invalid file URL '{file}' in {source_list}"))?
            };
            let client = crate::tls::client_builder().build()?;
            client.get(aix_url).send().await?.bytes().await?.to_vec()
        }
        SourceListItem::LnReaderRaw { url, .. } => {
            // Only actually read when the `lnreader` feature is compiled
            // in (see below) -- referenced unconditionally here so a build
            // without the feature doesn't warn about an unused field.
            let _ = &url;

            #[cfg(feature = "lnreader")]
            {
                crate::source::packaging::install_from_url(&url, lnreader_enabled)
                    .await
                    .with_context(|| format!("cannot install '{}'", source_id.value()))?
            }
            #[cfg(not(feature = "lnreader"))]
            bail!(
                "cannot install '{}': this build of Rakuyomi was compiled without LNReader support",
                source_id.value()
            );
        }
    };

    // Locked only now, after every network fetch above has completed --
    // holding this for the whole function would block every other route
    // that needs `source_manager` for as long as the source list/plugin
    // JS download takes.
    arc_manager.lock().await.install_source(
        &source_id,
        aix_content,
        source_of_source,
        arc_manager,
    )?;

    Ok(())
}

/// One entry of a `source_lists` document, once we know which of the two
/// shapes it is — the already-packaged Aidoku shape (`file`/`downloadURL`
/// pointing at a ready `.aix`) or LNReader's raw, unpackaged shape (`url`
/// pointing at a compiled `.js` that still needs to go through
/// `packaging::package_plugin_js`). `#[serde(untagged)]` picks whichever
/// variant actually matches the entry's fields — see
/// `docs/lnreader/REFERENCE.md` §5.1.
#[derive(Deserialize)]
#[serde(untagged)]
enum SourceListItem {
    Packaged {
        id: SourceId,
        #[serde(alias = "downloadURL")]
        file: String,
    },
    LnReaderRaw {
        id: SourceId,
        url: String,
    },
}

impl SourceListItem {
    fn id(&self) -> &SourceId {
        match self {
            Self::Packaged { id, .. } => id,
            Self::LnReaderRaw { id, .. } => id,
        }
    }
}

#[cfg(test)]
mod source_list_item_tests {
    use super::*;

    #[test]
    fn deserializes_aidoku_shaped_entry_as_packaged() {
        let item: SourceListItem = serde_json::from_value(serde_json::json!({
            "id": "en.asurascans",
            "name": "Asura Scans",
            "version": 18,
            "downloadURL": "sources/en.asurascans-v18.aix",
            "languages": ["en"],
        }))
        .unwrap();

        assert!(matches!(
            item,
            SourceListItem::Packaged { ref file, .. } if file == "sources/en.asurascans-v18.aix"
        ));
    }

    #[test]
    fn deserializes_bare_file_field_as_packaged() {
        let item: SourceListItem = serde_json::from_value(serde_json::json!({
            "id": "some-source",
            "file": "some-source.aix",
        }))
        .unwrap();

        assert!(matches!(item, SourceListItem::Packaged { .. }));
    }

    #[test]
    fn deserializes_lnreader_shaped_entry_as_raw() {
        let item: SourceListItem = serde_json::from_value(serde_json::json!({
            "id": "arnovel",
            "name": "ArNovel",
            "site": "https://ar-no.com/",
            "version": "2.2.0",
            "url": "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/arabic/ArNovel[madara].js",
        }))
        .unwrap();

        assert!(matches!(
            item,
            SourceListItem::LnReaderRaw { ref url, .. } if url.ends_with("ArNovel[madara].js")
        ));
    }

    #[test]
    fn id_accessor_works_for_both_variants() {
        let packaged: SourceListItem = serde_json::from_value(serde_json::json!({
            "id": "a", "file": "a.aix",
        }))
        .unwrap();
        let raw: SourceListItem = serde_json::from_value(serde_json::json!({
            "id": "b", "url": "https://example.com/b.js",
        }))
        .unwrap();

        assert_eq!(packaged.id(), &SourceId::new("a".to_string()));
        assert_eq!(raw.id(), &SourceId::new("b".to_string()));
    }
}
