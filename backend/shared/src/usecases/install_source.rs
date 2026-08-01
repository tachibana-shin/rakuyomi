use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    model::SourceId,
    settings::SourceList,
    source_manager::SourceManager,
    usecases::resolve_source_list::{resolve_source_list, source_list_key},
};

pub async fn install_source(
    source_manager: &mut SourceManager,
    arc_manager: &Arc<Mutex<SourceManager>>,
    source_lists: &[SourceList],
    source_id: SourceId,
    source_of_source: String,
) -> Result<()> {
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
        let response = client
            .get(resolved_list.clone())
            .send()
            .await
            .with_context(|| format!("failed to fetch source list at {}", &resolved_list))?;

        let value: Value = response
            .json()
            .await
            .with_context(|| format!("failed to parse source list at {}", &resolved_list))?;

        // Try both formats
        let source_list_items = if value.is_array() {
            serde_json::from_value::<Vec<SourceListItem>>(value)?
        } else if let Some(arr) = value.get("sources").and_then(|v| v.as_array()) {
            serde_json::from_value::<Vec<SourceListItem>>(Value::Array(arr.clone()))?
        } else {
            anyhow::bail!(
                "unexpected JSON format for source list at {}: {}",
                &resolved_list,
                value
            );
        };
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
            source_manager.install_lnreader_source(&source_id, plugin_content, source_of_source)?;
        }
        crate::settings::SourceListType::Aidoku => {
            let file = source_list_item
                .file
                .context("source list item is missing a `file`")?;
            let aix_url = if file.starts_with("sources/") {
                source_list.url.join(&file).unwrap()
            } else {
                source_list.url.join(&format!("sources/{}", &file)).unwrap()
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

    Ok(())
}

#[derive(Deserialize)]
struct SourceListItem {
    id: SourceId,
    /// Aidoku index: file name of the `.aix`, relative to the source list URL.
    #[serde(alias = "downloadURL")]
    file: Option<String>,
    /// LNReader index: absolute URL of the compiled plugin `.js` file.
    url: Option<String>,
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
}
