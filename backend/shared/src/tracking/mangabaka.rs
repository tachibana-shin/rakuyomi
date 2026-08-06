use anyhow::{Context, Result};
use reqwest::header::CONTENT_TYPE;
use serde::{de, Deserialize, Deserializer, Serialize};
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, Tracker};

const MANGABAKA_API_URL: &str = "https://api.mangabaka.org";
const MANGABAKA_SITE_URL: &str = "https://mangabaka.org";

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
            #[serde(rename = "native_title")]
            native_title: Option<String>,
            #[serde(rename = "romanized_title")]
            romanized_title: Option<String>,
            // The API returns these as JSON strings (e.g. "700"), not numbers.
            #[serde(
                rename = "total_chapters",
                default,
                deserialize_with = "deserialize_number_from_string_opt"
            )]
            total_chapters: Option<i64>,
            // There is no "total volumes" field in the API; `final_volume` (the
            // last volume number, once known) is the closest equivalent.
            #[serde(
                rename = "final_volume",
                default,
                deserialize_with = "deserialize_number_from_string_opt"
            )]
            total_volumes: Option<i64>,
        }

        let client = build_client();
        let response: SearchResponse = client
            .get(format!("{MANGABAKA_API_URL}/v1/series/search"))
            .query(&[("q", query), ("limit", "5")])
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
                    .or(series.romanized_title)
                    .or(series.native_title)
                    .unwrap_or_else(|| "Unknown".to_owned()),
                url: Some(
                    Url::parse(&format!("{MANGABAKA_SITE_URL}/{}", series.id))
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
        struct LibraryResponse {
            data: LibraryEntry,
        }

        #[derive(Deserialize)]
        struct LibraryEntry {
            state: Option<String>,
            #[serde(rename = "progress_chapter")]
            progress_chapter: Option<i64>,
            #[serde(rename = "progress_volume")]
            progress_volume: Option<i64>,
            #[serde(rename = "start_date")]
            start_date: Option<String>,
            #[serde(rename = "finish_date")]
            finish_date: Option<String>,
        }

        let api_key = require_api_key(settings)?;
        let client = build_client();
        let response = client
            .get(format!("{MANGABAKA_API_URL}/v1/my/library/{media_id}"))
            .header("x-api-key", api_key)
            .send()
            .await?;

        // 404 means not in library yet
        if response.status() == 404 {
            return Ok(TrackingProgressSnapshot::default());
        }

        let entry: LibraryEntry = response
            .error_for_status()?
            .json::<LibraryResponse>()
            .await
            .context("failed to decode MangaBaka library response")?
            .data;

        Ok(TrackingProgressSnapshot {
            status: entry.state.as_deref().and_then(parse_status),
            chapter_progress: entry.progress_chapter,
            volume_progress: entry.progress_volume,
            updated_at: None,
            started_at: entry.start_date.as_deref().and_then(parse_date),
            completed_at: entry.finish_date.as_deref().and_then(parse_date),
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
            // The API accepts (and prefers) a bare `YYYY-MM-DD` date on writes.
            #[serde(rename = "start_date", skip_serializing_if = "Option::is_none")]
            start_date: Option<String>,
            #[serde(rename = "finish_date", skip_serializing_if = "Option::is_none")]
            finish_date: Option<String>,
        }

        let api_key = require_api_key(settings)?;
        let client = build_client();
        let body = LibraryBody {
            state: snapshot.status.map(|s| format_status(s).to_owned()),
            progress_chapter: snapshot.chapter_progress,
            progress_volume: snapshot.volume_progress,
            start_date: snapshot.started_at.and_then(format_date),
            finish_date: snapshot.completed_at.and_then(format_date),
        };

        // PATCH to update existing, or POST to create
        let response = client
            .patch(format!("{MANGABAKA_API_URL}/v1/my/library/{media_id}"))
            .header("x-api-key", api_key)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        // If 404, the entry doesn't exist yet — create it (the create endpoint
        // accepts the same fields as the update one).
        if response.status() == 404 {
            client
                .post(format!("{MANGABAKA_API_URL}/v1/my/library/{media_id}"))
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

    async fn get_user(&self, settings: &Settings) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct ProfileResponse {
            data: Profile,
        }

        #[derive(Deserialize)]
        struct Profile {
            id: String,
            #[serde(rename = "preferred_username")]
            preferred_username: Option<String>,
            nickname: Option<String>,
        }

        let api_key = require_api_key(settings)?;
        let client = build_client();
        let profile: Profile = client
            .get(format!("{MANGABAKA_API_URL}/v1/my/profile"))
            .header("x-api-key", api_key)
            .send()
            .await?
            .error_for_status()?
            .json::<ProfileResponse>()
            .await
            .context("failed to decode MangaBaka profile response")?
            .data;

        Ok(Some(
            profile
                .preferred_username
                .or(profile.nickname)
                .unwrap_or(profile.id),
        ))
    }
}

/// The MangaBaka API returns numeric fields like `total_chapters` as JSON strings
/// (e.g. `"700"`) rather than numbers, so they need a custom deserializer.
fn deserialize_number_from_string_opt<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<String> = Option::deserialize(deserializer)?;
    value
        .map(|s| s.parse::<i64>().map_err(de::Error::custom))
        .transpose()
}

/// Parses a MangaBaka date field, which may be a full RFC3339 timestamp or a
/// bare `YYYY-MM-DD` date (as returned for `start_date`/`finish_date`).
fn parse_date(value: &str) -> Option<i64> {
    super::parse_iso8601_timestamp(value).or_else(|| {
        chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|datetime| datetime.and_utc().timestamp())
    })
}

/// Formats a unix timestamp as the bare `YYYY-MM-DD` date the API prefers on writes.
fn format_date(timestamp: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(timestamp, 0).map(|dt| dt.format("%Y-%m-%d").to_string())
}

fn require_api_key(settings: &Settings) -> Result<&str> {
    settings
        .mangabaka
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
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
