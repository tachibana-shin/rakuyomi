use anyhow::Context;
use axum::extract::{Path, State as StateExtractor};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use shared::usecases;
use shared::usecases::update_settings::UpdateableSettings;

use crate::state::State;
use crate::AppError;

pub fn routes() -> Router<State> {
    Router::new()
        .route("/settings", get(get_settings))
        .route("/settings", put(update_settings))
        .route("/settings/mount-tmpfs", post(mount_tmpfs))
        .route("/settings/test-proxy", post(test_proxy))
        .route(
            "/settings/tracking/validate",
            post(validate_tracking_settings),
        )
        .route("/settings/tracking/user/{service}", get(get_tracking_user))
        .route("/settings/oauth/start", post(start_oauth))
        .route(
            "/settings/oauth/status/{session_id}",
            get(poll_oauth_status),
        )
}

async fn get_settings(
    StateExtractor(State { settings, .. }): StateExtractor<State>,
) -> Json<UpdateableSettings> {
    Json(UpdateableSettings::from(&*settings.lock().await))
}

async fn update_settings(
    StateExtractor(State {
        chapter_storage,
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Json(updateable_settings): Json<UpdateableSettings>,
) -> Result<Json<UpdateableSettings>, AppError> {
    let mut chapter_storage = chapter_storage.lock().await;
    let mut settings = settings.lock().await;
    usecases::update_settings(&mut settings, &settings_path, updateable_settings)?;

    shared::tls::set_proxy_url(settings.proxy_url.clone());

    // Update the chapter storage for the new storage path
    if let Some(storage_path) = settings.storage_path.as_ref() {
        chapter_storage
            .set_downloads_folder_path(storage_path.clone())
            .with_context(|| {
                format!(
                    "Couldn't set the new storage path to {}",
                    storage_path.display()
                )
            })?;
    }

    Ok(Json(UpdateableSettings::from(&*settings)))
}
#[derive(serde::Deserialize)]
struct ValidateTrackingRequest {
    service: shared::model::TrackingService,
}

async fn validate_tracking_settings(
    StateExtractor(State {
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Json(body): Json<ValidateTrackingRequest>,
) -> Result<Json<()>, AppError> {
    let mut settings = settings.lock().await;
    usecases::validate_tracking_settings(&mut settings, body.service).await?;

    settings.save_to_file(&settings_path)?;

    Ok(Json(()))
}

#[derive(Serialize)]
struct TrackingUserResponse {
    username: Option<String>,
}

async fn get_tracking_user(
    StateExtractor(State {
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Path(service): Path<shared::model::TrackingService>,
) -> Result<Json<TrackingUserResponse>, AppError> {
    let mut settings = settings.lock().await;
    let username = usecases::get_tracking_user_info(&mut settings, service).await?;

    settings.save_to_file(&settings_path)?;

    Ok(Json(TrackingUserResponse { username }))
}

#[derive(serde::Deserialize)]
struct MountTmpFSBody {
    enabled: bool,
    ram_storage_size_mb: usize,
}

async fn mount_tmpfs(
    StateExtractor(State {
        chapter_storage,
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Json(MountTmpFSBody {
        enabled,
        ram_storage_size_mb,
    }): Json<MountTmpFSBody>,
) -> Result<Json<()>, AppError> {
    let mut chapter_storage = chapter_storage.lock().await;
    let mut settings = settings.lock().await;

    if enabled {
        chapter_storage
            .enable_ram(ram_storage_size_mb)
            .map_err(AppError::MountTmpFs)?;

        let mut updated_settings = settings.clone();
        updated_settings.ram_storage_enabled = enabled;
        updated_settings.ram_storage_size_mb = ram_storage_size_mb;
        updated_settings.save_to_file(&settings_path)?;

        *settings = updated_settings;
    } else {
        chapter_storage.disable_ram();

        let mut updated_settings = settings.clone();
        updated_settings.ram_storage_enabled = false;
        updated_settings.save_to_file(&settings_path)?;

        *settings = updated_settings;
    }

    Ok(Json(()))
}

#[derive(Deserialize)]
struct TestProxyBody {
    proxy_url: String,
}

#[derive(Serialize)]
struct TestProxyResponse {
    ok: bool,
    message: String,
}

async fn test_proxy(
    Json(TestProxyBody { proxy_url }): Json<TestProxyBody>,
) -> Result<Json<TestProxyResponse>, AppError> {
    match shared::tls::test_proxy(&proxy_url).await {
        Ok(()) => Ok(Json(TestProxyResponse {
            ok: true,
            message: "Proxy connection successful".to_string(),
        })),
        Err(e) => Err(AppError::Other(anyhow::anyhow!("proxy test failed: {e}"))),
    }
}

#[derive(Deserialize)]
struct StartOAuthRequest {
    service: shared::usecases::OAuthService,
}

#[derive(Serialize)]
struct StartOAuthResponse {
    session_id: String,
    bridge_url: String,
}

async fn start_oauth(
    StateExtractor(State { settings, .. }): StateExtractor<State>,
    Json(body): Json<StartOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, AppError> {
    let (server_url, chat_id, device_name, _api_token) = {
        let s = settings.lock().await;
        (
            s.oauth_server_url.clone(),
            s.cookie_sync_chat_id,
            s.cookie_sync_device_name.clone(),
            s.cookie_sync_api_token.clone(),
        )
    };

    let session =
        usecases::start_oauth_session(&server_url, body.service, chat_id, device_name).await?;

    let bridge_url = format!("{}{}", server_url, session.bridge_path);

    Ok(Json(StartOAuthResponse {
        session_id: session.session_id,
        bridge_url,
    }))
}

#[derive(Serialize)]
#[serde(tag = "status")]
#[serde(rename_all = "snake_case")]
enum OAuthStatusResponse {
    Pending,
    Completed {
        tokens: Option<usecases::oauth_bridge::OAuthTokens>,
    },
    Error {
        message: String,
    },
}

async fn poll_oauth_status(
    StateExtractor(State {
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Path(session_id): Path<String>,
) -> Result<Json<OAuthStatusResponse>, AppError> {
    let (server_url, api_token) = {
        let s = settings.lock().await;
        (s.oauth_server_url.clone(), s.cookie_sync_api_token.clone())
    };

    let resp = usecases::poll_oauth_status(&server_url, &session_id, api_token.as_deref()).await?;

    match resp {
        usecases::oauth_bridge::PollSessionResponse::Pending { .. } => {
            Ok(Json(OAuthStatusResponse::Pending))
        }
        usecases::oauth_bridge::PollSessionResponse::Completed {
            tokens, service, ..
        } => {
            // Save tokens to settings based on service
            if let Some(tokens) = &tokens {
                let mut settings = settings.lock().await;
                let access = tokens.access_token.clone();
                let refresh = tokens.refresh_token.clone();

                match service {
                    usecases::oauth_bridge::OAuthService::Anilist => {
                        if let Some(t) = access {
                            settings.anilist.access_token = Some(t);
                        }
                    }
                    usecases::oauth_bridge::OAuthService::MyAnimeList => {
                        if let Some(t) = access {
                            settings.myanimelist.access_token = Some(t);
                        }
                        if let Some(t) = refresh {
                            settings.myanimelist.refresh_token = Some(t);
                        }
                    }
                    usecases::oauth_bridge::OAuthService::Shikimori => {
                        if let Some(t) = access {
                            settings.shikimori.access_token = Some(t);
                        }
                        if let Some(t) = refresh {
                            settings.shikimori.refresh_token = Some(t);
                        }
                    }
                    usecases::oauth_bridge::OAuthService::Bangumi => {
                        if let Some(t) = access {
                            settings.bangumi.access_token = Some(t);
                        }
                        if let Some(t) = refresh {
                            settings.bangumi.refresh_token = Some(t);
                        }
                    }
                    usecases::oauth_bridge::OAuthService::Mangabaka => {
                        if let Some(t) = access {
                            settings.mangabaka.access_token = Some(t);
                        }
                        if let Some(t) = refresh {
                            settings.mangabaka.refresh_token = Some(t);
                        }
                    }
                }

                // Fetch username right away while we have the fresh token
                let tracking_service = match service {
                    usecases::oauth_bridge::OAuthService::Anilist => {
                        shared::model::TrackingService::Anilist
                    }
                    usecases::oauth_bridge::OAuthService::MyAnimeList => {
                        shared::model::TrackingService::MyAnimeList
                    }
                    usecases::oauth_bridge::OAuthService::Shikimori => {
                        shared::model::TrackingService::Shikimori
                    }
                    usecases::oauth_bridge::OAuthService::Bangumi => {
                        shared::model::TrackingService::Bangumi
                    }
                    usecases::oauth_bridge::OAuthService::Mangabaka => {
                        shared::model::TrackingService::Mangabaka
                    }
                };
                match usecases::get_tracking_user_info(&mut settings, tracking_service).await {
                    Ok(username) => {
                        log::info!("Fetched username after OAuth: {:?}", username);
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch username after OAuth: {:#}", e);
                    }
                }

                settings.save_to_file(&settings_path)?;
            }

            Ok(Json(OAuthStatusResponse::Completed { tokens }))
        }
        usecases::oauth_bridge::PollSessionResponse::Error { message, .. } => {
            Ok(Json(OAuthStatusResponse::Error { message }))
        }
    }
}
