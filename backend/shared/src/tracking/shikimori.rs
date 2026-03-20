use anyhow::{Context, Result};
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, get_json, post_json};

const SHIKIMORI_API_URL: &str = "https://shikimori.one/api";

pub struct ShikimoriTracker;

impl ShikimoriTracker {
    pub async fn search(&self, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Deserialize)]
        struct MangaNode {
            id: i64,
            name: String,
            russian: Option<String>,
            chapters: Option<i64>,
            volumes: Option<i64>,
            url: String,
        }

        let client = build_client();
        let request = client
            .get(format!("{SHIKIMORI_API_URL}/mangas"))
            .query(&[("search", query), ("limit", "5")]);
        let response: Vec<MangaNode> = get_json(request, "failed to decode Shikimori search results").await?;

        Ok(response
            .into_iter()
            .map(|media| TrackingCandidate {
                service: TrackingService::Shikimori,
                remote_media_id: media.id,
                title: media.russian.unwrap_or(media.name),
                url: Url::parse(&format!("https://shikimori.one{}", media.url)).ok(),
                total_chapters: media.chapters.filter(|&v| v > 0),
                total_volumes: media.volumes.filter(|&v| v > 0),
            })
            .collect())
    }

    pub async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        struct UserRate {
            status: Option<String>,
            chapters: Option<i64>,
            volumes: Option<i64>,
            updated_at: Option<String>,
        }

        let token = require_access_token(settings)?;
        let client = build_client();
        
        // We need to find the user rate for this manga. 
        // Unfortunately, Shikimori doesn't have a direct "get my rate for this media" by media_id alone without user_id or listing all rates.
        // Actually, we can use /api/v2/user_rates with target_id and target_type.
        let target_id_str = media_id.to_string();
        let request = client
            .get(format!("{SHIKIMORI_API_URL}/v2/user_rates"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .query(&[
                ("target_id", target_id_str.as_str()),
                ("target_type", "Manga"),
            ]);
        
        let response: Vec<UserRate> = get_json(request, "failed to decode Shikimori user rates").await?;
        let entry = response.into_iter().next();

        Ok(entry
            .map(|e| TrackingProgressSnapshot {
                status: e.status.as_deref().and_then(parse_status),
                chapter_progress: e.chapters,
                volume_progress: e.volumes,
                updated_at: e.updated_at.as_deref().and_then(parse_timestamp),
            })
            .unwrap_or_default())
    }

    pub async fn push_progress(
        &self,
        settings: &Settings,
        media_id: i64,
        snapshot: &TrackingProgressSnapshot,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        struct UserRate {
            id: i64,
        }

        let token = require_access_token(settings)?;
        let client = build_client();

        // Check if rate exists
        let target_id_str = media_id.to_string();
        let check_request = client
            .get(format!("{SHIKIMORI_API_URL}/v2/user_rates"))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .query(&[
                ("target_id", target_id_str.as_str()),
                ("target_type", "Manga"),
            ]);
        let existing: Vec<UserRate> = get_json(check_request, "failed to check Shikimori user rate").await?;

        let status_str = snapshot.status.map(format_status);
        
        if let Some(rate) = existing.into_iter().next() {
            // PATCH
            let mut body = serde_json::json!({});
            if let Some(s) = status_str {
                body["user_rate"]["status"] = serde_json::json!(s);
            }
            if let Some(c) = snapshot.chapter_progress {
                body["user_rate"]["chapters"] = serde_json::json!(c);
            }
            if let Some(v) = snapshot.volume_progress {
                body["user_rate"]["volumes"] = serde_json::json!(v);
            }

            let patch_request = client
                .patch(format!("{SHIKIMORI_API_URL}/v2/user_rates/{}", rate.id))
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .json(&serde_json::json!({ "user_rate": body }));
            
            patch_request.send().await?.error_for_status()?;
        } else {
            // POST
            let mut body = serde_json::json!({
                "target_id": media_id,
                "target_type": "Manga",
            });
            if let Some(s) = status_str {
                body["status"] = serde_json::json!(s);
            }
            if let Some(c) = snapshot.chapter_progress {
                body["chapters"] = serde_json::json!(c);
            }
            if let Some(v) = snapshot.volume_progress {
                body["volumes"] = serde_json::json!(v);
            }

            let post_request = client
                .post(format!("{SHIKIMORI_API_URL}/v2/user_rates"))
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .json(&serde_json::json!({ "user_rate": body }));
            
            post_request.send().await?.error_for_status()?;
        }

        self.fetch_progress(settings, media_id).await
    }

    pub async fn refresh_access_token(&self, settings: &Settings) -> Result<(String, Option<String>)> {
        let client_id = settings.shikimori_client_id.as_deref().context("Shikimori client ID is not configured")?;
        let client_secret = settings.shikimori_client_secret.as_deref().context("Shikimori client secret is not configured")?;
        let refresh_token = settings.shikimori_refresh_token.as_deref().context("Shikimori refresh token is not configured")?;

        let client = build_client();
        let response = client
            .post("https://shikimori.one/oauth/token")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            }))
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

fn require_access_token(settings: &Settings) -> Result<&str> {
    settings
        .shikimori_access_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Shikimori access token is not configured")
}

fn parse_status(status: &str) -> Option<TrackingStatus> {
    match status {
        "planned" => Some(TrackingStatus::Planning),
        "watching" => Some(TrackingStatus::Current),
        "rewatching" => Some(TrackingStatus::Repeating),
        "completed" => Some(TrackingStatus::Completed),
        "on_hold" => Some(TrackingStatus::Paused),
        "dropped" => Some(TrackingStatus::Dropped),
        _ => None,
    }
}

fn format_status(status: TrackingStatus) -> &'static str {
    match status {
        TrackingStatus::Planning => "planned",
        TrackingStatus::Current => "watching",
        TrackingStatus::Repeating => "rewatching",
        TrackingStatus::Completed => "completed",
        TrackingStatus::Paused => "on_hold",
        TrackingStatus::Dropped => "dropped",
    }
}

fn parse_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
}
