use anyhow::{bail, Context, Result};
use chrono::Utc;
use reqwest::header::AUTHORIZATION;
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    model::{
        TrackingBinding, TrackingCandidate, TrackingProgressSnapshot, TrackingService,
        TrackingStatus,
    },
    settings::Settings,
};

pub mod anilist;
pub mod bangumi;
pub mod kavita;
pub mod komga;
pub mod mangabaka;
pub mod myanimelist;
pub mod shikimori;
pub mod suwayomi;

/// Trait that all tracking service implementations must satisfy.
///
/// To add a new tracker:
/// 1. Add a variant to `TrackingService` in `model.rs`
/// 2. Create a new file `your_service.rs` in this directory
/// 3. Implement `Tracker` for your struct
/// 4. Add a variant to `TrackerImpl` and wire it up in `tracker_for()`
#[allow(async_fn_in_trait)]
pub trait Tracker: Send + Sync {
    fn service(&self) -> TrackingService;

    async fn search(&self, settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>>;

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot>;

    async fn push_progress(
        &self,
        settings: &Settings,
        media_id: i64,
        snapshot: &TrackingProgressSnapshot,
    ) -> Result<TrackingProgressSnapshot>;

    /// Returns the username for the authenticated user, if the service supports it.
    async fn get_user(&self, _settings: &Settings) -> Result<Option<String>> {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Enum dispatch — works around async-fn-in-trait not being object-safe.
// ---------------------------------------------------------------------------

use anilist::AnilistTracker;
use bangumi::BangumiTracker;
use kavita::KavitaTracker;
use komga::KomgaTracker;
use mangabaka::MangaBakaTracker;
use myanimelist::MalTracker;
use shikimori::ShikimoriTracker;
use suwayomi::SuwayomiTracker;

/// Concrete dispatch type that wraps every tracker variant.
/// This exists because `async fn` in traits is not dyn-safe in stable Rust.
pub(crate) enum TrackerImpl {
    Anilist(AnilistTracker),
    MyAnimeList(MalTracker),
    Shikimori(ShikimoriTracker),
    Kavita(KavitaTracker),
    Bangumi(BangumiTracker),
    Mangabaka(MangaBakaTracker),
    Komga(KomgaTracker),
    Suwayomi(SuwayomiTracker),
}

impl Tracker for TrackerImpl {
    fn service(&self) -> TrackingService {
        match self {
            Self::Anilist(t) => t.service(),
            Self::MyAnimeList(t) => t.service(),
            Self::Shikimori(t) => t.service(),
            Self::Kavita(t) => t.service(),
            Self::Bangumi(t) => t.service(),
            Self::Mangabaka(t) => t.service(),
            Self::Komga(t) => t.service(),
            Self::Suwayomi(t) => t.service(),
        }
    }

    async fn search(&self, settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        match self {
            Self::Anilist(t) => t.search(settings, query).await,
            Self::MyAnimeList(t) => t.search(settings, query).await,
            Self::Shikimori(t) => t.search(settings, query).await,
            Self::Kavita(t) => t.search(settings, query).await,
            Self::Bangumi(t) => t.search(settings, query).await,
            Self::Mangabaka(t) => t.search(settings, query).await,
            Self::Komga(t) => t.search(settings, query).await,
            Self::Suwayomi(t) => t.search(settings, query).await,
        }
    }

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        match self {
            Self::Anilist(t) => t.fetch_progress(settings, media_id).await,
            Self::MyAnimeList(t) => t.fetch_progress(settings, media_id).await,
            Self::Shikimori(t) => t.fetch_progress(settings, media_id).await,
            Self::Kavita(t) => t.fetch_progress(settings, media_id).await,
            Self::Bangumi(t) => t.fetch_progress(settings, media_id).await,
            Self::Mangabaka(t) => t.fetch_progress(settings, media_id).await,
            Self::Komga(t) => t.fetch_progress(settings, media_id).await,
            Self::Suwayomi(t) => t.fetch_progress(settings, media_id).await,
        }
    }

    async fn push_progress(
        &self,
        settings: &Settings,
        media_id: i64,
        snapshot: &TrackingProgressSnapshot,
    ) -> Result<TrackingProgressSnapshot> {
        match self {
            Self::Anilist(t) => t.push_progress(settings, media_id, snapshot).await,
            Self::MyAnimeList(t) => t.push_progress(settings, media_id, snapshot).await,
            Self::Shikimori(t) => t.push_progress(settings, media_id, snapshot).await,
            Self::Kavita(t) => t.push_progress(settings, media_id, snapshot).await,
            Self::Bangumi(t) => t.push_progress(settings, media_id, snapshot).await,
            Self::Mangabaka(t) => t.push_progress(settings, media_id, snapshot).await,
            Self::Komga(t) => t.push_progress(settings, media_id, snapshot).await,
            Self::Suwayomi(t) => t.push_progress(settings, media_id, snapshot).await,
        }
    }

    async fn get_user(&self, settings: &Settings) -> Result<Option<String>> {
        match self {
            Self::Anilist(t) => t.get_user(settings).await,
            Self::MyAnimeList(t) => t.get_user(settings).await,
            Self::Shikimori(t) => t.get_user(settings).await,
            Self::Kavita(t) => t.get_user(settings).await,
            Self::Bangumi(t) => t.get_user(settings).await,
            Self::Mangabaka(t) => t.get_user(settings).await,
            Self::Komga(t) => t.get_user(settings).await,
            Self::Suwayomi(t) => t.get_user(settings).await,
        }
    }
}

fn tracker_for(service: TrackingService) -> TrackerImpl {
    match service {
        TrackingService::Anilist => TrackerImpl::Anilist(AnilistTracker),
        TrackingService::MyAnimeList => TrackerImpl::MyAnimeList(MalTracker),
        TrackingService::Shikimori => TrackerImpl::Shikimori(ShikimoriTracker),
        TrackingService::Kavita => TrackerImpl::Kavita(KavitaTracker),
        TrackingService::Bangumi => TrackerImpl::Bangumi(BangumiTracker),
        TrackingService::Mangabaka => TrackerImpl::Mangabaka(MangaBakaTracker),
        TrackingService::Komga => TrackerImpl::Komga(KomgaTracker),
        TrackingService::Suwayomi => TrackerImpl::Suwayomi(SuwayomiTracker),
    }
}

// ---------------------------------------------------------------------------
// Public API — thin wrappers around tracker_for()
// ---------------------------------------------------------------------------

pub async fn search_candidates(
    settings: &mut Settings,
    service: TrackingService,
    query: &str,
) -> Result<Vec<TrackingCandidate>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    tracker_for(service).search(settings, query).await
}

pub async fn fetch_remote_progress(
    settings: &mut Settings,
    binding: &TrackingBinding,
) -> Result<TrackingProgressSnapshot> {
    ensure_access_token(settings, binding.service).await?;

    tracker_for(binding.service)
        .fetch_progress(settings, binding.remote_media_id)
        .await
}

pub async fn push_progress(
    settings: &mut Settings,
    binding: &TrackingBinding,
    snapshot: &TrackingProgressSnapshot,
) -> Result<TrackingProgressSnapshot> {
    ensure_access_token(settings, binding.service).await?;

    tracker_for(binding.service)
        .push_progress(settings, binding.remote_media_id, snapshot)
        .await
}

pub async fn validate_credentials(settings: &mut Settings, service: TrackingService) -> Result<()> {
    ensure_access_token(settings, service).await?;

    tracker_for(service)
        .get_user(settings)
        .await?
        .context("invalid credentials")?;
    Ok(())
}

pub async fn get_tracking_user(
    settings: &mut Settings,
    service: TrackingService,
) -> Result<Option<String>> {
    ensure_access_token(settings, service).await?;
    tracker_for(service).get_user(settings).await
}

// ---------------------------------------------------------------------------
// OAuth token refresh (only services with OAuth need this)
// ---------------------------------------------------------------------------

pub async fn ensure_access_token(settings: &mut Settings, service: TrackingService) -> Result<()> {
    match service {
        TrackingService::MyAnimeList => {
            if settings.myanimelist.access_token.is_none()
                && settings.myanimelist.refresh_token.is_some()
            {
                let (access, refresh) = MalTracker.refresh_access_token(settings).await?;
                settings.myanimelist.access_token = Some(access);
                if let Some(r) = refresh {
                    settings.myanimelist.refresh_token = Some(r);
                }
            }
        }
        TrackingService::Shikimori => {
            if settings.shikimori.access_token.is_none()
                && settings.shikimori.refresh_token.is_some()
            {
                let (access, refresh) = ShikimoriTracker.refresh_access_token(settings).await?;
                settings.shikimori.access_token = Some(access);
                if let Some(r) = refresh {
                    settings.shikimori.refresh_token = Some(r);
                }
            }
        }
        TrackingService::Bangumi => {
            if settings.bangumi.access_token.is_none() && settings.bangumi.refresh_token.is_some() {
                let (access, refresh) = BangumiTracker.refresh_access_token(settings).await?;
                settings.bangumi.access_token = Some(access);
                if let Some(r) = refresh {
                    settings.bangumi.refresh_token = Some(r);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub(crate) fn title_candidates(
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

pub(crate) fn build_client() -> reqwest::Client {
    crate::tls::client_builder()
        .user_agent("rakuyomi/1.0")
        .build()
        .expect("tracking HTTP client should build")
}

pub(crate) async fn post_json<TReq: Serialize, TResp: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: &TReq,
    bearer_token: Option<&str>,
) -> Result<TResp> {
    let mut request = client.post(url).json(body);
    if let Some(token) = bearer_token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {status}: {body_text}");
    }
    response
        .json::<TResp>()
        .await
        .context("failed to decode tracking response")
}

pub(crate) async fn get_json<TResp: DeserializeOwned>(
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

pub(crate) fn parse_iso8601_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
}

pub(crate) fn default_status_for_progress(progress: i64) -> TrackingStatus {
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

    let mut result = snapshot.clone();

    // Auto-set started_at when status becomes CURRENT and started_at is not yet set.
    if matches!(result.status, Some(TrackingStatus::Current)) && result.started_at.is_none() {
        result.started_at = Some(Utc::now().timestamp());
    }

    // Auto-set completed_at when status becomes COMPLETED and completed_at is not yet set.
    if matches!(result.status, Some(TrackingStatus::Completed)) && result.completed_at.is_none() {
        result.completed_at = Some(Utc::now().timestamp());
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_progress_rejects_negative_chapter() {
        let snapshot = TrackingProgressSnapshot {
            status: None,
            chapter_progress: Some(-1),
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        assert!(sanitize_progress(&snapshot).is_err());
    }

    #[test]
    fn sanitize_progress_rejects_negative_volume() {
        let snapshot = TrackingProgressSnapshot {
            status: None,
            chapter_progress: None,
            volume_progress: Some(-5),
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        assert!(sanitize_progress(&snapshot).is_err());
    }

    #[test]
    fn sanitize_progress_allows_zero() {
        let snapshot = TrackingProgressSnapshot {
            status: None,
            chapter_progress: Some(0),
            volume_progress: Some(0),
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        assert!(sanitize_progress(&snapshot).is_ok());
    }

    #[test]
    fn title_candidates_prefers_user_preferred() {
        let result = title_candidates(
            Some("Preferred".into()),
            Some("Romaji".into()),
            Some("English".into()),
            Some("Native".into()),
        );
        assert_eq!(result, "Preferred");
    }

    #[test]
    fn title_candidates_falls_back_to_english() {
        let result = title_candidates(None, Some("Romaji".into()), Some("English".into()), None);
        assert_eq!(result, "English");
    }

    #[test]
    fn title_candidates_falls_back_to_romaji() {
        let result = title_candidates(None, Some("Romaji".into()), None, None);
        assert_eq!(result, "Romaji");
    }

    #[test]
    fn title_candidates_falls_back_to_native() {
        let result = title_candidates(None, None, None, Some("Native".into()));
        assert_eq!(result, "Native");
    }

    #[test]
    fn title_candidates_defaults_to_unknown() {
        let result = title_candidates(None, None, None, None);
        assert_eq!(result, "Unknown title");
    }

    #[test]
    fn default_status_for_progress_positive() {
        assert_eq!(default_status_for_progress(5), TrackingStatus::Current);
    }

    #[test]
    fn default_status_for_progress_zero() {
        assert_eq!(default_status_for_progress(0), TrackingStatus::Planning);
    }
}
