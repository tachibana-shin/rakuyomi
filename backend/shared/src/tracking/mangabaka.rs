use anyhow::{Context, Result};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, Tracker};

const MANGABAKA_API_URL: &str = "https://api.mangabaka.org";

pub struct MangaBakaTracker;

impl Tracker for MangaBakaTracker {
    fn service(&self) -> TrackingService {
        TrackingService::Mangabaka
    }

    async fn search(&self, _settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Deserialize)]
        struct SearchResponse {
            data: Vec<Series>,
        }

        #[derive(Deserialize)]
        struct Series {
            id: i64,
            title: Option<String>,
            #[serde(rename = "title_ja")]
            title_ja: Option<String>,
            #[serde(rename = "total_chapters")]
            total_chapters: Option<i64>,
            #[serde(rename = "total_volumes")]
            total_volumes: Option<i64>,
        }

        let client = build_client();
        let response: SearchResponse = client
            .get(format!("{MANGABAKA_API_URL}/v1/series/search"))
            .query(&[("query", query), ("limit", "5")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to decode MangaBaka search response")?;

        Ok(response
            .data
            .into_iter()
            .map(|series| TrackingCandidate {
                service: TrackingService::Mangabaka,
                remote_media_id: series.id,
                title: series
                    .title
                    .or(series.title_ja)
                    .unwrap_or_else(|| "Unknown".to_owned()),
                url: Some(
                    Url::parse(&format!("{MANGABAKA_API_URL}/series/{}", series.id))
                        .expect("valid URL"),
                ),
                total_chapters: series.total_chapters,
                total_volumes: series.total_volumes,
            })
            .collect())
    }

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        struct LibraryEntry {
            state: Option<String>,
            #[serde(rename = "progress_chapter")]
            progress_chapter: Option<i64>,
            #[serde(rename = "progress_volume")]
            progress_volume: Option<i64>,
            #[serde(rename = "updated_at")]
            updated_at: Option<String>,
        }

        let api_key = require_api_key(settings)?;
        let client = build_client();
        let response = client
            .get(format!("{MANGABAKA_API_URL}/v0/my/library/{media_id}"))
            .header("x-api-key", api_key)
            .send()
            .await?;

        // 404 means not in library yet
        if response.status() == 404 {
            return Ok(TrackingProgressSnapshot::default());
        }

        let entry: LibraryEntry = response
            .error_for_status()?
            .json()
            .await
            .context("failed to decode MangaBaka library response")?;

        Ok(TrackingProgressSnapshot {
            status: entry.state.as_deref().and_then(parse_status),
            chapter_progress: entry.progress_chapter,
            volume_progress: entry.progress_volume,
            updated_at: entry
                .updated_at
                .as_deref()
                .and_then(super::parse_iso8601_timestamp),
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
        #[derive(Serialize)]
        struct LibraryBody {
            #[serde(rename = "state", skip_serializing_if = "Option::is_none")]
            state: Option<String>,
            #[serde(rename = "progress_chapter", skip_serializing_if = "Option::is_none")]
            progress_chapter: Option<i64>,
            #[serde(rename = "progress_volume", skip_serializing_if = "Option::is_none")]
            progress_volume: Option<i64>,
        }

        let api_key = require_api_key(settings)?;
        let client = build_client();
        let body = LibraryBody {
            state: snapshot.status.map(|s| format_status(s).to_owned()),
            progress_chapter: snapshot.chapter_progress,
            progress_volume: snapshot.volume_progress,
        };

        // PATCH to update existing, or POST to create
        let response = client
            .patch(format!("{MANGABAKA_API_URL}/v0/my/library/{media_id}"))
            .header("x-api-key", api_key)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        // If 404, the entry doesn't exist yet — create it
        if response.status() == 404 {
            client
                .post(format!("{MANGABAKA_API_URL}/v0/my/library/{media_id}"))
                .header("x-api-key", api_key)
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await?
                .error_for_status()?;
        } else {
            response.error_for_status()?;
        }

        // Re-fetch to return updated state
        self.fetch_progress(settings, media_id).await
    }
}

fn require_api_key(settings: &Settings) -> Result<&str> {
    settings
        .mangabaka
        .access_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            settings
                .mangabaka
                .api_key
                .as_deref()
                .filter(|v| !v.trim().is_empty())
        })
        .context("MangaBaka API key is not configured")
}

fn parse_status(status: &str) -> Option<TrackingStatus> {
    match status {
        "reading" => Some(TrackingStatus::Current),
        "completed" => Some(TrackingStatus::Completed),
        "paused" => Some(TrackingStatus::Paused),
        "dropped" => Some(TrackingStatus::Dropped),
        "plan_to_read" => Some(TrackingStatus::Planning),
        "rereading" => Some(TrackingStatus::Repeating),
        _ => None,
    }
}

fn format_status(status: TrackingStatus) -> &'static str {
    match status {
        TrackingStatus::Current => "reading",
        TrackingStatus::Completed => "completed",
        TrackingStatus::Paused => "paused",
        TrackingStatus::Dropped => "dropped",
        TrackingStatus::Planning => "plan_to_read",
        TrackingStatus::Repeating => "rereading",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_all_variants() {
        assert_eq!(parse_status("reading"), Some(TrackingStatus::Current));
        assert_eq!(parse_status("completed"), Some(TrackingStatus::Completed));
        assert_eq!(parse_status("paused"), Some(TrackingStatus::Paused));
        assert_eq!(parse_status("dropped"), Some(TrackingStatus::Dropped));
        assert_eq!(parse_status("plan_to_read"), Some(TrackingStatus::Planning));
        assert_eq!(parse_status("rereading"), Some(TrackingStatus::Repeating));
        assert_eq!(parse_status("unknown"), None);
    }

    #[test]
    fn format_status_roundtrip() {
        let statuses = [
            TrackingStatus::Current,
            TrackingStatus::Completed,
            TrackingStatus::Paused,
            TrackingStatus::Dropped,
            TrackingStatus::Planning,
            TrackingStatus::Repeating,
        ];
        for status in statuses {
            let formatted = format_status(status);
            assert_eq!(parse_status(formatted), Some(status));
        }
    }
}
