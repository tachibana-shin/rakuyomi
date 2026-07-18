use anyhow::{Context, Result};
use serde::Deserialize;
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, get_json, Tracker};

pub struct KomgaTracker;

impl Tracker for KomgaTracker {
    fn service(&self) -> TrackingService {
        TrackingService::Komga
    }

    async fn search(&self, settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct SearchResponse {
            content: Vec<SeriesNode>,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        #[allow(non_snake_case)]
        struct SeriesNode {
            id: String,
            name: String,
            #[serde(default)]
            booksCount: Option<i64>,
        }

        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();

        let request = client
            .get(format!("{base_url}/api/v1/series"))
            .basic_auth(&api_key, None::<&str>)
            .query(&[("search", query)]);

        let response: SearchResponse =
            get_json(request, "failed to decode Komga search results").await?;

        Ok(response
            .content
            .into_iter()
            .map(|series| TrackingCandidate {
                service: TrackingService::Komga,
                remote_media_id: series.id.parse().unwrap_or(0),
                title: series.name,
                url: Url::parse(&format!("{}/series/{}", base_url, series.id)).ok(),
                total_chapters: series.booksCount,
                total_volumes: None,
            })
            .collect())
    }

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();

        // Use Tachiyomi compatibility endpoint for chapter-level progress
        #[derive(Deserialize)]
        #[allow(dead_code)]
        #[allow(non_snake_case)]
        struct TachiyomiProgress {
            booksCount: Option<i64>,
            booksReadCount: Option<i64>,
            lastReadContinuousNumberSort: Option<f64>,
        }

        let series_id = media_id_to_string(media_id);

        let request = client
            .get(format!(
                "{base_url}/api/v2/series/{series_id}/read-progress/tachiyomi"
            ))
            .basic_auth(&api_key, None::<&str>);

        let response = request.send().await?;

        // If 404, no reading progress yet
        if response.status() == 404 {
            // Fallback: get readStatus from series endpoint
            return self
                .fetch_status_only(base_url, api_key, &client, &series_id)
                .await;
        }

        let progress: TachiyomiProgress = response
            .error_for_status()?
            .json()
            .await
            .context("failed to decode Komga Tachiyomi progress")?;

        let total = progress.booksCount.unwrap_or(0);
        let read = progress.booksReadCount.unwrap_or(0);

        // Derive status from read progress
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
        let series_id = media_id_to_string(media_id);

        if let Some(status) = snapshot.status {
            // Always sync readStatus metadata on the series
            let status_str = format_status(status);
            let request = client
                .patch(format!("{base_url}/api/v1/series/{series_id}"))
                .basic_auth(&api_key, None::<&str>)
                .json(&serde_json::json!({
                    "readStatus": status_str,
                }));
            request.send().await?.error_for_status()?;
        }

        self.fetch_progress(settings, media_id).await
    }
}

impl KomgaTracker {
    async fn fetch_status_only(
        &self,
        base_url: &str,
        api_key: &str,
        client: &reqwest::Client,
        series_id: &str,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct SeriesDetail {
            readStatus: Option<String>,
        }

        let request = client
            .get(format!("{base_url}/api/v1/series/{series_id}"))
            .basic_auth(api_key, None::<&str>);

        let response: SeriesDetail =
            get_json(request, "failed to decode Komga series details").await?;

        Ok(TrackingProgressSnapshot {
            status: response.readStatus.as_deref().and_then(parse_status),
            chapter_progress: None,
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        })
    }
}

fn require_url(settings: &Settings) -> Result<&str> {
    settings
        .komga
        .url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Komga URL is not configured")
}

fn require_api_key(settings: &Settings) -> Result<&str> {
    settings
        .komga
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Komga API key is not configured")
}

fn media_id_to_string(media_id: i64) -> String {
    media_id.to_string()
}

fn parse_status(status: &str) -> Option<TrackingStatus> {
    match status {
        "UNREAD" => Some(TrackingStatus::Planning),
        "READING" => Some(TrackingStatus::Current),
        "COMPLETED" => Some(TrackingStatus::Completed),
        "ON_HOLD" => Some(TrackingStatus::Paused),
        "DROPPED" => Some(TrackingStatus::Dropped),
        _ => None,
    }
}

fn format_status(status: TrackingStatus) -> &'static str {
    match status {
        TrackingStatus::Planning => "UNREAD",
        TrackingStatus::Current | TrackingStatus::Repeating => "READING",
        TrackingStatus::Completed => "COMPLETED",
        TrackingStatus::Paused => "ON_HOLD",
        TrackingStatus::Dropped => "DROPPED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_all_variants() {
        assert_eq!(parse_status("UNREAD"), Some(TrackingStatus::Planning));
        assert_eq!(parse_status("READING"), Some(TrackingStatus::Current));
        assert_eq!(parse_status("COMPLETED"), Some(TrackingStatus::Completed));
        assert_eq!(parse_status("ON_HOLD"), Some(TrackingStatus::Paused));
        assert_eq!(parse_status("DROPPED"), Some(TrackingStatus::Dropped));
        assert_eq!(parse_status("UNKNOWN"), None);
    }

    #[test]
    fn format_status_values() {
        assert_eq!(format_status(TrackingStatus::Planning), "UNREAD");
        assert_eq!(format_status(TrackingStatus::Current), "READING");
        assert_eq!(format_status(TrackingStatus::Repeating), "READING");
        assert_eq!(format_status(TrackingStatus::Completed), "COMPLETED");
        assert_eq!(format_status(TrackingStatus::Paused), "ON_HOLD");
        assert_eq!(format_status(TrackingStatus::Dropped), "DROPPED");
    }
}
