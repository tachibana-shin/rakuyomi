use anyhow::{Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use url::Url;

use crate::model::SourceInformation;
use serde_json::Value;

pub async fn list_available_sources(source_lists: Vec<Url>) -> Result<Vec<SourceInformation>> {
    let mut source_informations: Vec<SourceInformation> = stream::iter(source_lists)
        .then(|source_list| async move {
            let response = reqwest::get(source_list.clone())
                .await
                .with_context(|| format!("failed to fetch source list at {}", &source_list))?;

            let value: Value = response
                .json()
                .await
                .with_context(|| format!("failed to parse source list at {}", &source_list))?;

            // Try both formats
            let sources = if value.is_array() {
                serde_json::from_value::<Vec<SourceInformation>>(value)?
            } else if let Some(arr) = value.get("sources").and_then(|v| v.as_array()) {
                serde_json::from_value::<Vec<SourceInformation>>(Value::Array(arr.clone()))?
            } else {
                anyhow::bail!(
                    "unexpected JSON format for source list at {}: {}",
                    &source_list,
                    value
                );
            };

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
