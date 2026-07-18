use anyhow::{Context, Result};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, Tracker};

const BANGUMI_API_URL: &str = "https://api.bgm.tv";

pub struct BangumiTracker;

impl Tracker for BangumiTracker {
    fn service(&self) -> TrackingService {
        TrackingService::Bangumi
    }

    async fn search(&self, _settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Serialize)]
        struct SearchRequest<'a> {
            keyword: &'a str,
            sort: &'a str,
            filter: SearchFilter,
        }

        #[derive(Serialize)]
        struct SearchFilter {
            #[serde(rename = "type")]
            types: Vec<i32>,
            nsfw: bool,
        }

        #[derive(Deserialize)]
        struct SearchResponse {
            data: Vec<Subject>,
        }

        #[derive(Deserialize)]
        struct Subject {
            id: i64,
            name: Option<String>,
            name_cn: Option<String>,
            eps: Option<i64>,
            volumes: Option<i64>,
            #[allow(dead_code)]
            date: Option<String>,
        }

        let client = build_client();
        let body = SearchRequest {
            keyword: query,
            sort: "match",
            filter: SearchFilter {
                types: vec![1], // Book/Manga
                nsfw: false,
            },
        };

        let response: SearchResponse = client
            .post(format!("{BANGUMI_API_URL}/v0/search/subjects"))
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to decode Bangumi search response")?;

        Ok(response
            .data
            .into_iter()
            .map(|subject| TrackingCandidate {
                service: TrackingService::Bangumi,
                remote_media_id: subject.id,
                title: subject
                    .name_cn
                    .or(subject.name)
                    .unwrap_or_else(|| "Unknown".to_owned()),
                url: Some(
                    Url::parse(&format!("https://bgm.tv/subject/{}", subject.id))
                        .expect("valid URL"),
                ),
                total_chapters: subject.eps,
                total_volumes: subject.volumes,
            })
            .collect())
    }

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        struct CollectionResponse {
            #[serde(rename = "type")]
            collection_type: Option<i32>,
            #[serde(rename = "ep_status")]
            ep_status: Option<i64>,
            #[serde(rename = "vol_status")]
            vol_status: Option<i64>,
            #[serde(rename = "updated_at")]
            updated_at: Option<String>,
        }

        let token = require_token(settings)?;
        let client = build_client();
        let response = client
            .get(format!(
                "{BANGUMI_API_URL}/v0/users/-/collections/{media_id}"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;

        // 404 means not in collection yet
        if response.status() == 404 {
            return Ok(TrackingProgressSnapshot::default());
        }

        let entry: CollectionResponse = response
            .error_for_status()?
            .json()
            .await
            .context("failed to decode Bangumi collection response")?;

        Ok(TrackingProgressSnapshot {
            status: entry.collection_type.and_then(parse_status),
            chapter_progress: entry.ep_status,
            volume_progress: entry.vol_status,
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
        struct CollectionBody {
            #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
            collection_type: Option<i32>,
            #[serde(rename = "ep_status", skip_serializing_if = "Option::is_none")]
            ep_status: Option<i64>,
            #[serde(rename = "vol_status", skip_serializing_if = "Option::is_none")]
            vol_status: Option<i64>,
        }

        let token = require_token(settings)?;
        let client = build_client();
        let body = CollectionBody {
            collection_type: snapshot.status.map(format_status),
            ep_status: snapshot.chapter_progress,
            vol_status: snapshot.volume_progress,
        };

        // POST is upsert (create or update)
        client
            .post(format!(
                "{BANGUMI_API_URL}/v0/users/-/collections/{media_id}"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        // Re-fetch to return updated state
        self.fetch_progress(settings, media_id).await
    }

    async fn get_user(&self, settings: &Settings) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct UserResponse {
            username: Option<String>,
        }

        let token = match require_token(settings) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };
        let client = build_client();
        let request = client
            .get(format!("{BANGUMI_API_URL}/v2/users/me"))
            .header("Authorization", format!("Bearer {token}"));
        let response: UserResponse = request
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("failed to decode Bangumi user info")?;

        Ok(response.username)
    }
}

impl BangumiTracker {
    pub async fn refresh_access_token(
        &self,
        settings: &Settings,
    ) -> Result<(String, Option<String>)> {
        let client_id = settings
            .bangumi
            .client_id
            .as_deref()
            .context("Bangumi client ID is not configured")?;
        let client_secret = settings
            .bangumi
            .client_secret
            .as_deref()
            .context("Bangumi client secret is not configured")?;
        let refresh_token = settings
            .bangumi
            .refresh_token
            .as_deref()
            .context("Bangumi refresh token is not configured")?;

        let client = build_client();
        let response = client
            .post("https://bgm.tv/oauth/access_token")
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

fn require_token(settings: &Settings) -> Result<&str> {
    settings
        .bangumi
        .access_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Bangumi access token is not configured")
}

fn parse_status(status: i32) -> Option<TrackingStatus> {
    match status {
        3 => Some(TrackingStatus::Current),   // 在看 (Doing)
        2 => Some(TrackingStatus::Completed), // 看过 (Done)
        4 => Some(TrackingStatus::Paused),    // 搁置 (OnHold)
        5 => Some(TrackingStatus::Dropped),   // 抛弃 (Dropped)
        1 => Some(TrackingStatus::Planning),  // 想看 (Wish)
        _ => None,
    }
}

fn format_status(status: TrackingStatus) -> i32 {
    match status {
        TrackingStatus::Current => 3,
        TrackingStatus::Completed => 2,
        TrackingStatus::Paused => 4,
        TrackingStatus::Dropped => 5,
        TrackingStatus::Planning => 1,
        TrackingStatus::Repeating => 3, // No direct mapping, use Doing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_all_variants() {
        assert_eq!(parse_status(1), Some(TrackingStatus::Planning));
        assert_eq!(parse_status(2), Some(TrackingStatus::Completed));
        assert_eq!(parse_status(3), Some(TrackingStatus::Current));
        assert_eq!(parse_status(4), Some(TrackingStatus::Paused));
        assert_eq!(parse_status(5), Some(TrackingStatus::Dropped));
        assert_eq!(parse_status(0), None);
        assert_eq!(parse_status(6), None);
    }

    #[test]
    fn format_status_roundtrip() {
        let statuses = [
            TrackingStatus::Current,
            TrackingStatus::Completed,
            TrackingStatus::Paused,
            TrackingStatus::Dropped,
            TrackingStatus::Planning,
        ];
        for status in statuses {
            let formatted = format_status(status);
            assert_eq!(parse_status(formatted), Some(status));
        }
    }

    #[test]
    fn format_status_repeating_maps_to_current() {
        assert_eq!(format_status(TrackingStatus::Repeating), 3);
    }
}
