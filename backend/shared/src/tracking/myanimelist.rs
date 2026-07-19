use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{
    build_client, default_status_for_progress, get_json, parse_iso8601_timestamp, Tracker,
};

const MAL_API_URL: &str = "https://api.myanimelist.net/v2";

pub struct MalTracker;

impl Tracker for MalTracker {
    fn service(&self) -> TrackingService {
        TrackingService::MyAnimeList
    }

    async fn search(&self, settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Deserialize)]
        struct SearchResponse {
            data: Vec<SearchEntry>,
        }

        #[derive(Deserialize)]
        struct SearchEntry {
            node: MangaNode,
        }

        #[derive(Deserialize)]
        struct MangaNode {
            id: i64,
            title: String,
            num_chapters: Option<i64>,
            num_volumes: Option<i64>,
        }

        let client_id = require_client_id(settings)?;
        let client = build_client();
        let request = client
            .get(format!("{MAL_API_URL}/manga"))
            .header("X-MAL-CLIENT-ID", client_id)
            .query(&[
                ("q", query),
                ("limit", "5"),
                ("fields", "id,title,num_chapters,num_volumes"),
            ]);
        let response: SearchResponse =
            get_json(request, "failed to decode MyAnimeList search results").await?;

        Ok(response
            .data
            .into_iter()
            .map(|entry| TrackingCandidate {
                service: TrackingService::MyAnimeList,
                remote_media_id: entry.node.id,
                title: entry.node.title,
                url: Url::parse(&format!("https://myanimelist.net/manga/{}", entry.node.id)).ok(),
                total_chapters: entry.node.num_chapters,
                total_volumes: entry.node.num_volumes,
            })
            .collect())
    }

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        struct MangaResponse {
            #[serde(rename = "my_list_status")]
            my_list_status: Option<MyListStatus>,
        }

        #[derive(Deserialize)]
        struct MyListStatus {
            status: Option<String>,
            #[serde(rename = "num_chapters_read")]
            num_chapters_read: Option<i64>,
            #[serde(rename = "num_volumes_read")]
            num_volumes_read: Option<i64>,
            #[serde(rename = "updated_at")]
            updated_at: Option<String>,
        }

        let client_id = require_client_id(settings)?;
        let access_token = require_access_token(settings)?;
        let client = build_client();
        let request = client
            .get(format!("{MAL_API_URL}/manga/{media_id}"))
            .header("X-MAL-CLIENT-ID", client_id)
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .query(&[("fields", "my_list_status")]);
        let response: MangaResponse =
            get_json(request, "failed to decode MyAnimeList list entry").await?;

        Ok(response
            .my_list_status
            .map(|entry| TrackingProgressSnapshot {
                status: entry.status.as_deref().and_then(parse_status),
                chapter_progress: entry.num_chapters_read,
                volume_progress: entry.num_volumes_read,
                updated_at: entry
                    .updated_at
                    .as_deref()
                    .and_then(parse_iso8601_timestamp),
                started_at: None,
                completed_at: None,
            })
            .unwrap_or_default())
    }

    async fn push_progress(
        &self,
        settings: &Settings,
        media_id: i64,
        snapshot: &TrackingProgressSnapshot,
    ) -> Result<TrackingProgressSnapshot> {
        let client_id = require_client_id(settings)?;
        let access_token = require_access_token(settings)?;
        let status = snapshot
            .status
            .or_else(|| snapshot.chapter_progress.map(default_status_for_progress))
            .context("MyAnimeList sync requires a status or chapter progress")?;

        let mut form = vec![("status".to_owned(), format_status(status).to_owned())];
        if let Some(progress) = snapshot.chapter_progress {
            form.push(("num_chapters_read".to_owned(), progress.to_string()));
        }
        if let Some(progress) = snapshot.volume_progress {
            form.push(("num_volumes_read".to_owned(), progress.to_string()));
        }

        let client = build_client();
        client
            .put(format!("{MAL_API_URL}/manga/{media_id}/my_list_status"))
            .header("X-MAL-CLIENT-ID", client_id)
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&form)
            .send()
            .await?
            .error_for_status()?;

        self.fetch_progress(settings, media_id).await
    }

    async fn get_user(&self, settings: &Settings) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct UserResponse {
            name: Option<String>,
        }

        let client_id = match require_client_id(settings) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };
        let access_token = match require_access_token(settings) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };
        let client = build_client();
        let request = client
            .get(format!("{MAL_API_URL}/users/@me"))
            .header("X-MAL-CLIENT-ID", client_id)
            .header(AUTHORIZATION, format!("Bearer {access_token}"));
        let response: UserResponse =
            get_json(request, "failed to decode MyAnimeList user info").await?;

        Ok(response.name)
    }
}

impl MalTracker {
    pub async fn refresh_access_token(
        &self,
        settings: &Settings,
    ) -> Result<(String, Option<String>)> {
        let client_id = settings
            .myanimelist
            .client_id
            .as_deref()
            .context("MyAnimeList client ID is not configured")?;
        let refresh_token = settings
            .myanimelist
            .refresh_token
            .as_deref()
            .context("MyAnimeList refresh token is not configured")?;

        let client = build_client();
        let mut form = vec![
            ("grant_type".to_owned(), "refresh_token".to_owned()),
            ("refresh_token".to_owned(), refresh_token.to_owned()),
            ("client_id".to_owned(), client_id.to_owned()),
        ];
        if let Some(secret) = settings
            .myanimelist
            .client_secret
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            form.push(("client_secret".to_owned(), secret.to_owned()));
        }
        let response = client
            .post("https://myanimelist.net/v1/oauth2/token")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(&form)
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

fn require_client_id(settings: &Settings) -> Result<&str> {
    settings
        .myanimelist
        .client_id
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("MyAnimeList client ID is not configured")
}

fn require_access_token(settings: &Settings) -> Result<&str> {
    settings
        .myanimelist
        .access_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("MyAnimeList access token is not configured")
}

fn parse_status(status: &str) -> Option<TrackingStatus> {
    match status {
        "reading" => Some(TrackingStatus::Current),
        "completed" => Some(TrackingStatus::Completed),
        "on_hold" => Some(TrackingStatus::Paused),
        "dropped" => Some(TrackingStatus::Dropped),
        "plan_to_read" => Some(TrackingStatus::Planning),
        _ => None,
    }
}

fn format_status(status: TrackingStatus) -> &'static str {
    match status {
        TrackingStatus::Current | TrackingStatus::Repeating => "reading",
        TrackingStatus::Completed => "completed",
        TrackingStatus::Paused => "on_hold",
        TrackingStatus::Dropped => "dropped",
        TrackingStatus::Planning => "plan_to_read",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_all_variants() {
        assert_eq!(parse_status("reading"), Some(TrackingStatus::Current));
        assert_eq!(parse_status("completed"), Some(TrackingStatus::Completed));
        assert_eq!(parse_status("on_hold"), Some(TrackingStatus::Paused));
        assert_eq!(parse_status("dropped"), Some(TrackingStatus::Dropped));
        assert_eq!(parse_status("plan_to_read"), Some(TrackingStatus::Planning));
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
            assert!(
                parse_status(formatted).is_some(),
                "roundtrip failed for {:?} -> {}",
                status,
                formatted
            );
        }
    }
}
