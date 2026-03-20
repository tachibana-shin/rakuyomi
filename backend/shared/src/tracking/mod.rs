use anyhow::{bail, Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    model::{
        TrackingBinding, TrackingCandidate, TrackingProgressSnapshot, TrackingService,
        TrackingStatus,
    },
    settings::Settings,
};

pub mod anilist;
pub mod mal;
pub mod shikimori;
pub mod kavita;

use anilist::AnilistTracker;
use mal::MalTracker;
use shikimori::ShikimoriTracker;
use kavita::KavitaTracker;

pub async fn search_candidates(
    settings: &mut Settings,
    service: TrackingService,
    query: &str,
) -> Result<Vec<TrackingCandidate>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    match service {
        TrackingService::AniList => AnilistTracker.search(query).await,
        TrackingService::MyAnimeList => {
            // MyAnimeList search can work with CLIENT_ID only, or with access_token.
            // If we have an access token, it's better. 
            // But let's keep it simple for now as it already works with client_id.
            MalTracker.search(settings, query).await
        }
        TrackingService::Shikimori => ShikimoriTracker.search(query).await,
        TrackingService::Kavita => KavitaTracker.search(settings, query).await,
    }
}

pub async fn fetch_remote_progress(
    settings: &mut Settings,
    binding: &TrackingBinding,
) -> Result<TrackingProgressSnapshot> {
    ensure_access_token(settings, binding.service).await?;

    match binding.service {
        TrackingService::AniList => AnilistTracker.fetch_progress(settings, binding.remote_media_id).await,
        TrackingService::MyAnimeList => {
            MalTracker.fetch_progress(settings, binding.remote_media_id).await
        }
        TrackingService::Shikimori => ShikimoriTracker.fetch_progress(settings, binding.remote_media_id).await,
        TrackingService::Kavita => KavitaTracker.fetch_progress(settings, binding.remote_media_id).await,
    }
}

pub async fn push_progress(
    settings: &mut Settings,
    binding: &TrackingBinding,
    snapshot: &TrackingProgressSnapshot,
) -> Result<TrackingProgressSnapshot> {
    ensure_access_token(settings, binding.service).await?;

    match binding.service {
        TrackingService::AniList => {
            AnilistTracker.push_progress(settings, binding.remote_media_id, snapshot).await
        }
        TrackingService::MyAnimeList => {
            MalTracker.push_progress(settings, binding.remote_media_id, snapshot).await
        }
        TrackingService::Shikimori => {
            ShikimoriTracker.push_progress(settings, binding.remote_media_id, snapshot).await
        }
        TrackingService::Kavita => {
            KavitaTracker.push_progress(settings, binding.remote_media_id, snapshot).await
        }
    }
}

pub async fn validate_credentials(
    settings: &mut Settings,
    service: TrackingService,
) -> Result<()> {
    ensure_access_token(settings, service).await?;

    match service {
        TrackingService::AniList => {
            AnilistTracker.fetch_progress(settings, 1).await?; // Dummy call
            Ok(())
        }
        TrackingService::MyAnimeList => {
            MalTracker.fetch_progress(settings, 1).await?; // Dummy call
            Ok(())
        }
        TrackingService::Shikimori => {
            ShikimoriTracker.fetch_progress(settings, 1).await?; // Dummy call
            Ok(())
        }
        TrackingService::Kavita => {
            KavitaTracker.search(settings, "test").await?;
            Ok(())
        }
    }
}

pub async fn ensure_access_token(settings: &mut Settings, service: TrackingService) -> Result<()> {
    match service {
        TrackingService::MyAnimeList => {
            if settings.mal_access_token.is_none() && settings.mal_refresh_token.is_some() {
                let (access, refresh) = MalTracker.refresh_access_token(settings).await?;
                settings.mal_access_token = Some(access);
                if let Some(r) = refresh {
                    settings.mal_refresh_token = Some(r);
                }
            }
        }
        TrackingService::Shikimori => {
            if settings.shikimori_access_token.is_none() && settings.shikimori_refresh_token.is_some() {
                let (access, refresh) = ShikimoriTracker.refresh_access_token(settings).await?;
                settings.shikimori_access_token = Some(access);
                if let Some(r) = refresh {
                    settings.shikimori_refresh_token = Some(r);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn title_candidates(
    preferred: Option<String>,
    romaji: Option<String>,
    english: Option<String>,
    native: Option<String>,
) -> String {
    preferred
        .or(english)
        .or(romaji)
        .or(native)
        .unwrap_or_else(|| "Unknown title".to_owned())
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("rakuyomi/1.0")
        .build()
        .expect("tracking HTTP client should build")
}

async fn post_json<TReq: Serialize, TResp: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: &TReq,
    bearer_token: Option<&str>,
) -> Result<TResp> {
    let mut request = client.post(url).json(body);
    if let Some(token) = bearer_token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    request
        .send()
        .await?
        .error_for_status()?
        .json::<TResp>()
        .await
        .context("failed to decode tracking response")
}

async fn get_json<TResp: DeserializeOwned>(
    request: reqwest::RequestBuilder,
    context_message: &str,
) -> Result<TResp> {
    request
        .send()
        .await?
        .error_for_status()?
        .json::<TResp>()
        .await
        .with_context(|| context_message.to_owned())
}

fn parse_iso8601_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
}

fn default_status_for_progress(progress: i64) -> TrackingStatus {
    if progress > 0 {
        TrackingStatus::Current
    } else {
        TrackingStatus::Planning
    }
}

pub fn sanitize_progress(snapshot: &TrackingProgressSnapshot) -> Result<TrackingProgressSnapshot> {
    if let Some(progress) = snapshot.chapter_progress {
        if progress < 0 {
            bail!("chapter progress cannot be negative");
        }
    }
    if let Some(progress) = snapshot.volume_progress {
        if progress < 0 {
            bail!("volume progress cannot be negative");
        }
    }

    Ok(snapshot.clone())
}
