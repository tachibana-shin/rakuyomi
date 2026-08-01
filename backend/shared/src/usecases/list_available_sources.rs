use anyhow::{Context, Result};
use futures::{stream, StreamExt, TryStreamExt};

use crate::model::SourceInformation;
use crate::settings::SourceList;
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
            let mut sources = if value.is_array() {
                serde_json::from_value::<Vec<SourceInformation>>(value)?
            } else if let Some(arr) = value.get("sources").and_then(|v| v.as_array()) {
                serde_json::from_value::<Vec<SourceInformation>>(Value::Array(arr.clone()))?
            } else {
                anyhow::bail!(
                    "unexpected JSON format for source list at {}: {}",
                    &resolved_list,
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
