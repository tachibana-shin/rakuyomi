use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{de, Deserialize, Deserializer, Serialize};
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, Tracker};

const MANGABAKA_API_URL: &str = "https://api.mangabaka.org";
const MANGABAKA_SITE_URL: &str = "https://mangabaka.org";
// From https://mangabaka.org/.well-known/openid-configuration
const MANGABAKA_TOKEN_URL: &str = "https://mangabaka.org/auth/oauth2/token";

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
                    Url::parse(&format!("{MANGABAKA_SITE_URL}/{}", series.id)).expect("valid URL"),
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

        let auth = resolve_auth(settings)?;
        let client = build_client();
        let response = apply_auth(
            client.get(format!("{MANGABAKA_API_URL}/v1/my/library/{media_id}")),
            &auth,
        )
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

        let auth = resolve_auth(settings)?;
        let client = build_client();
        let body = LibraryBody {
            state: snapshot.status.map(|s| format_status(s).to_owned()),
            progress_chapter: snapshot.chapter_progress,
            progress_volume: snapshot.volume_progress,
            start_date: snapshot.started_at.and_then(format_date),
            finish_date: snapshot.completed_at.and_then(format_date),
        };

        // PATCH to update existing, or POST to create
        let response = apply_auth(
            client.patch(format!("{MANGABAKA_API_URL}/v1/my/library/{media_id}")),
            &auth,
        )
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await?;

        // If 404, the entry doesn't exist yet — create it (the create endpoint
        // accepts the same fields as the update one).
        if response.status() == 404 {
            apply_auth(
                client.post(format!("{MANGABAKA_API_URL}/v1/my/library/{media_id}")),
                &auth,
            )
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

        let auth = resolve_auth(settings)?;
        let client = build_client();
        let profile: Profile = apply_auth(
            client.get(format!("{MANGABAKA_API_URL}/v1/my/profile")),
            &auth,
        )
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

impl MangaBakaTracker {
    /// Exchanges a refresh token for a new access token. MangaBaka's OAuth token
    /// endpoint requires client authentication (no public/"none" auth method per
    /// its `.well-known/openid-configuration`), so both client_id and
    /// client_secret must be present — they're stored on sign-in alongside the
    /// tokens (see `OAuthFlowView.lua`).
    pub async fn refresh_access_token(
        &self,
        settings: &Settings,
    ) -> Result<(String, Option<String>)> {
        let client_id = settings
            .mangabaka
            .client_id
            .as_deref()
            .context("MangaBaka client ID is not configured")?;
        let client_secret = settings
            .mangabaka
            .client_secret
            .as_deref()
            .context("MangaBaka client secret is not configured")?;
        let refresh_token = settings
            .mangabaka
            .refresh_token
            .as_deref()
            .context("MangaBaka refresh token is not configured")?;

        let client = build_client();
        let response = client
            .post(MANGABAKA_TOKEN_URL)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?
            .error_for_status()?;

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: Option<String>,
        }

        let tokens: TokenResponse = response.json().await?;
        Ok((tokens.access_token, tokens.refresh_token))
    }
}

enum Auth<'a> {
    Bearer(&'a str),
    ApiKey(&'a str),
}

/// Resolves MangaBaka credentials, preferring an OAuth access token (sent as
/// `Authorization: Bearer`) over a Personal Access Token (sent as `x-api-key`),
/// matching the two auth methods documented in MangaBaka's OpenAPI spec.
fn resolve_auth(settings: &Settings) -> Result<Auth<'_>> {
    if let Some(token) = settings
        .mangabaka
        .access_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        return Ok(Auth::Bearer(token));
    }
    if let Some(key) = settings
        .mangabaka
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        return Ok(Auth::ApiKey(key));
    }
    anyhow::bail!("MangaBaka credentials are not configured")
}

fn apply_auth(request: reqwest::RequestBuilder, auth: &Auth) -> reqwest::RequestBuilder {
    match auth {
        Auth::Bearer(token) => request.header(AUTHORIZATION, format!("Bearer {token}")),
        Auth::ApiKey(key) => request.header("x-api-key", *key),
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
