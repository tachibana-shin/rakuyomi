use anyhow::{bail, Context, Result};
use serde::Deserialize;
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, get_json};

pub struct KavitaTracker;

impl KavitaTracker {
    pub async fn search(&self, settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Deserialize)]
        struct SearchResponse {
            series: Vec<SeriesNode>,
        }

        #[derive(Deserialize)]
        struct SeriesNode {
            id: i64,
            name: String,
            pages_read: i64,
            total_pages: i64,
        }

        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();
        
        let request = client
            .get(format!("{base_url}/api/Search"))
            .header("x-api-key", api_key)
            .query(&[("term", query)]);
        
        let response: SearchResponse = get_json(request, "failed to decode Kavita search results").await?;

        Ok(response
            .series
            .into_iter()
            .map(|series| TrackingCandidate {
                service: TrackingService::Kavita,
                remote_media_id: series.id,
                title: series.name,
                url: Url::parse(&format!("{}/Series/{}", base_url, series.id)).ok(),
                total_chapters: None, // Kavita tracks pages/volumes mostly
                total_volumes: None,
            })
            .collect())
    }

    pub async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Deserialize)]
        struct SeriesDetail {
            pages_read: i64,
            total_pages: i64,
            user_review: Option<UserReview>,
        }

        #[derive(Deserialize)]
        struct UserReview {
            status: i64, // 0: Want to Read, 1: Reading, 2: Read, 3: On Hold, 4: Dropped
        }

        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();

        let request = client
            .get(format!("{base_url}/api/Series/{}", media_id))
            .header("x-api-key", api_key);
        
        let response: SeriesDetail = get_json(request, "failed to decode Kavita series details").await?;

        Ok(TrackingProgressSnapshot {
            status: response.user_review.and_then(|r| parse_status(r.status)),
            chapter_progress: Some(response.pages_read), // Using pages as "chapters" proxy or just raw value
            volume_progress: None,
            updated_at: None,
        })
    }

    pub async fn push_progress(
        &self,
        settings: &Settings,
        media_id: i64,
        snapshot: &TrackingProgressSnapshot,
    ) -> Result<TrackingProgressSnapshot> {
        // Kavita progress push is often done per chapter/volume. 
        // For a generic "push", we might want to use the /api/Reader/mark-as-read or similar.
        // Or /api/Series/update-user-review for status.
        
        let base_url = require_url(settings)?;
        let api_key = require_api_key(settings)?;
        let client = build_client();

        if let Some(status) = snapshot.status {
            let status_code = format_status(status);
            let request = client
                .post(format!("{base_url}/api/Series/update-user-review"))
                .header("x-api-key", api_key)
                .json(&serde_json::json!({
                    "seriesId": media_id,
                    "status": status_code,
                }));
            request.send().await?.error_for_status()?;
        }

        // Kavita doesn't easily allow pushing raw page counts for a whole series via a single API call if not via OPDS/KOReader sync.
        // We'll stick to status for now.

        self.fetch_progress(settings, media_id).await
    }
}

fn require_url(settings: &Settings) -> Result<&str> {
    settings
        .kavita_url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Kavita URL is not configured")
}

fn require_api_key(settings: &Settings) -> Result<&str> {
    settings
        .kavita_api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("Kavita API key is not configured")
}

fn parse_status(status: i64) -> Option<TrackingStatus> {
    match status {
        0 => Some(TrackingStatus::Planning),
        1 => Some(TrackingStatus::Current),
        2 => Some(TrackingStatus::Completed),
        3 => Some(TrackingStatus::Paused),
        4 => Some(TrackingStatus::Dropped),
        _ => None,
    }
}

fn format_status(status: TrackingStatus) -> i64 {
    match status {
        TrackingStatus::Planning => 0,
        TrackingStatus::Current | TrackingStatus::Repeating => 1,
        TrackingStatus::Completed => 2,
        TrackingStatus::Paused => 3,
        TrackingStatus::Dropped => 4,
    }
}
