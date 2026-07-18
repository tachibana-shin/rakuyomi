use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, Tracker};

pub struct SuwayomiTracker;

#[derive(Serialize)]
struct GraphqlRequest {
    query: String,
    variables: Option<serde_json::Value>,
}

impl Tracker for SuwayomiTracker {
    fn service(&self) -> TrackingService {
        TrackingService::Suwayomi
    }

    async fn search(&self, settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct SearchResponse {
            data: SearchData,
        }

        #[derive(Deserialize)]
        struct SearchData {
            search: Vec<SeriesNode>,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        #[allow(non_snake_case)]
        struct SeriesNode {
            id: i64,
            title: String,
            #[serde(default)]
            chapters: Option<ChaptersConnection>,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct ChaptersConnection {
            #[serde(default)]
            totalCount: Option<i64>,
        }

        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();

        let graphql_query = r#"
            query SearchSeries($query: String!) {
                search(query: $query, type: MANGA) {
                    id
                    title
                    chapters {
                        totalCount
                    }
                }
            }
        "#;

        let request = client
            .post(format!("{base_url}/graphql"))
            .basic_auth(api_key, None::<&str>)
            .json(&GraphqlRequest {
                query: graphql_query.to_string(),
                variables: Some(serde_json::json!({ "query": query })),
            });

        let response: SearchResponse = request
            .send()
            .await?
            .error_for_status()?
            .json::<SearchResponse>()
            .await
            .context("failed to decode Suwayomi search response")?;

        Ok(response
            .data
            .search
            .into_iter()
            .map(|series| TrackingCandidate {
                service: TrackingService::Suwayomi,
                remote_media_id: series.id,
                title: series.title,
                url: Url::parse(&format!("{}/manga/{}", base_url, series.id)).ok(),
                total_chapters: series.chapters.and_then(|c| c.totalCount),
                total_volumes: None,
            })
            .collect())
    }

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        struct FetchResponse {
            data: FetchData,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct FetchData {
            #[serde(rename = "manga")]
            series: Option<SeriesDetail>,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct SeriesDetail {
            unreadCount: Option<i64>,
            chapters: Option<ChaptersList>,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct ChaptersList {
            nodes: Vec<ChapterNode>,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        #[allow(non_snake_case)]
        struct ChapterNode {
            id: i64,
            chapterNumber: Option<f64>,
            isRead: bool,
        }

        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();

        let graphql_query = r#"
            query MangaProgress($id: Long!) {
                manga(id: $id) {
                    unreadCount
                    chapters {
                        nodes {
                            id
                            chapterNumber
                            isRead
                        }
                    }
                }
            }
        "#;

        let request = client
            .post(format!("{base_url}/graphql"))
            .basic_auth(api_key, None::<&str>)
            .json(&GraphqlRequest {
                query: graphql_query.to_string(),
                variables: Some(serde_json::json!({ "id": media_id })),
            });

        let response: FetchResponse = request
            .send()
            .await?
            .error_for_status()?
            .json::<FetchResponse>()
            .await
            .context("failed to decode Suwayomi fetch response")?;

        let series = match response.data.series {
            Some(s) => s,
            None => return Ok(TrackingProgressSnapshot::default()),
        };

        let chapters = series.chapters.map(|c| c.nodes).unwrap_or_default();
        let total = chapters.len() as i64;
        let read = total - series.unreadCount.unwrap_or(total);

        let status = if total == 0 {
            None
        } else if read == total {
            Some(TrackingStatus::Completed)
        } else if read > 0 {
            Some(TrackingStatus::Current)
        } else {
            Some(TrackingStatus::Planning)
        };

        Ok(TrackingProgressSnapshot {
            status,
            chapter_progress: Some(read),
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        })
    }

    async fn push_progress(
        &self,
        settings: &Settings,
        media_id: i64,
        snapshot: &TrackingProgressSnapshot,
    ) -> Result<TrackingProgressSnapshot> {
        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();

        // Suwayomi has no series-level status concept.
        // When status is COMPLETED, mark all chapters as read.
        if matches!(snapshot.status, Some(TrackingStatus::Completed)) {
            // Fetch all chapter IDs first
            let chapters = self
                .fetch_all_chapter_ids(base_url, api_key, &client, media_id)
                .await?;
            if !chapters.is_empty() {
                let graphql_mutation = r#"
                    mutation MarkChaptersRead($ids: [Int!]!) {
                        updateChapters(input: { ids: $ids, patch: { isRead: true } }) {
                            chapters { id isRead }
                        }
                    }
                "#;
                let request = client
                    .post(format!("{base_url}/graphql"))
                    .basic_auth(api_key, None::<&str>)
                    .json(&GraphqlRequest {
                        query: graphql_mutation.to_string(),
                        variables: Some(serde_json::json!({ "ids": chapters })),
                    });
                request.send().await?.error_for_status()?;
            }
        }

        self.fetch_progress(settings, media_id).await
    }
}

impl SuwayomiTracker {
    async fn fetch_all_chapter_ids(
        &self,
        base_url: &str,
        api_key: &str,
        client: &reqwest::Client,
        manga_id: i64,
    ) -> Result<Vec<i64>> {
        #[derive(Deserialize)]
        struct Response {
            data: Data,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct Data {
            #[serde(rename = "manga")]
            series: Option<Series>,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct Series {
            chapters: Option<ChaptersList>,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct ChaptersList {
            nodes: Vec<ChapterNode>,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        #[allow(non_snake_case)]
        struct ChapterNode {
            id: i64,
        }

        let graphql_query = r#"
            query MangaChapters($id: Long!) {
                manga(id: $id) {
                    chapters {
                        nodes { id }
                    }
                }
            }
        "#;

        let request = client
            .post(format!("{base_url}/graphql"))
            .basic_auth(api_key, None::<&str>)
            .json(&GraphqlRequest {
                query: graphql_query.to_string(),
                variables: Some(serde_json::json!({ "id": manga_id })),
            });

        let response: Response = request
            .send()
            .await?
            .error_for_status()?
            .json::<Response>()
            .await
            .context("failed to decode Suwayomi chapters response")?;

        Ok(response
            .data
            .series
            .and_then(|s| s.chapters)
            .map(|c| c.nodes.into_iter().map(|n| n.id).collect())
            .unwrap_or_default())
    }
}

fn require_url(settings: &Settings) -> Result<&str> {
    settings
        .suwayomi
        .url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Suwayomi URL is not configured")
}

fn require_api_key(settings: &Settings) -> Result<&str> {
    settings
        .suwayomi
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Suwayomi API key is not configured")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_inference() {
        assert_eq!(infer_status(0, 10), Some(TrackingStatus::Planning));
        assert_eq!(infer_status(5, 10), Some(TrackingStatus::Current));
        assert_eq!(infer_status(10, 10), Some(TrackingStatus::Completed));
        assert_eq!(infer_status(0, 0), None);
    }

    fn infer_status(read: i64, total: i64) -> Option<TrackingStatus> {
        if total == 0 {
            None
        } else if read == total {
            Some(TrackingStatus::Completed)
        } else if read > 0 {
            Some(TrackingStatus::Current)
        } else {
            Some(TrackingStatus::Planning)
        }
    }
}
