use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::{model::SourceId, source_manager::SourceManager};

pub async fn install_source(
    source_manager: &mut SourceManager,
    arc_manager: &Arc<Mutex<SourceManager>>,
    source_lists: &[Url],
    source_id: SourceId,
    source_of_source: String,
) -> Result<()> {
    let (source_list, source_list_item, source_of_source) =
        stream::iter(source_lists.iter().filter(|url| {
            let domain = url.domain().unwrap_or("").to_string();
            domain == source_of_source
        }))
        .then(|source_list| async move {
            let domain = source_list.domain().unwrap_or("").to_string();

            let response = reqwest::get(source_list.clone())
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
        .find(|(_, item, _)| item.id == source_id)
        .ok_or_else(|| anyhow!("couldn't find source with id '{:?}'", source_id))?;

    let aix_url = if source_list_item.file.starts_with("sources/") {
        source_list.join(&source_list_item.file).unwrap()
    } else {
        source_list
            .join(&format!("sources/{}", &source_list_item.file))
            .unwrap()
    };
    let aix_content = reqwest::get(aix_url).await?.bytes().await?;

    source_manager.install_source(&source_id, aix_content, source_of_source, arc_manager)?;

    Ok(())
}

#[derive(Deserialize)]
struct SourceListItem {
    id: SourceId,
    #[serde(alias = "downloadURL")]
    file: String,
}
