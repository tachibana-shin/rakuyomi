use axum::extract::State as StateExtractor;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use shared::cookie_store;

use crate::state::State;
use crate::AppError;

pub fn routes() -> Router<State> {
    Router::new()
        .route("/cookie-sync/status", get(status))
        .route("/cookie-sync/generate-code", post(generate_code))
        .route("/cookie-sync/poll-pairing", post(poll_pairing))
        .route("/cookie-sync/sync", post(sync_all))
        .route("/cookie-sync/unpair", post(unpair))
        .route("/cookie-sync/cookies", get(list_cookies))
}

#[derive(Serialize)]
struct StatusResponse {
    paired: bool,
    server_url: Option<String>,
    device_name: Option<String>,
    chat_id: Option<i64>,
    cookie_count: usize,
}

async fn status(
    StateExtractor(State { settings, .. }): StateExtractor<State>,
) -> Json<StatusResponse> {
    let settings = settings.lock().await;
    let cookie_count = cookie_store::global_cookie_store()
        .and_then(|s| s.read().ok())
        .map(|s| s.domain_count())
        .unwrap_or(0);
    Json(StatusResponse {
        paired: settings.cookie_sync_chat_id.is_some(),
        server_url: settings.cookie_sync_server_url.clone(),
        device_name: settings.cookie_sync_device_name.clone(),
        chat_id: settings.cookie_sync_chat_id,
        cookie_count,
    })
}

#[derive(Deserialize)]
struct GenerateCodeRequest {
    server_url: String,
}

#[derive(Serialize)]
struct GenerateCodeResponse {
    pairing_code: String,
}

async fn generate_code(
    Json(req): Json<GenerateCodeRequest>,
) -> Result<Json<GenerateCodeResponse>, AppError> {
    let pairing_code = cookie_store::generate_pairing_code(&req.server_url)
        .await
        .map_err(|e| AppError::from(e))?;
    Ok(Json(GenerateCodeResponse { pairing_code }))
}

#[derive(Deserialize)]
struct PollPairingRequest {
    server_url: String,
    pairing_code: String,
}

#[derive(Serialize)]
struct PollPairingResponse {
    paired: bool,
    chat_id: Option<i64>,
    device_name: Option<String>,
}

async fn poll_pairing(
    StateExtractor(State {
        settings, settings_path, ..
    }): StateExtractor<State>,
    Json(req): Json<PollPairingRequest>,
) -> Result<Json<PollPairingResponse>, AppError> {
    let status = cookie_store::poll_pairing_status(&req.server_url, &req.pairing_code)
        .await
        .map_err(|e| AppError::from(e))?;

    if status.paired {
        let mut settings = settings.lock().await;
        settings.cookie_sync_server_url = Some(req.server_url);
        settings.cookie_sync_chat_id = status.chat_id;
        settings.cookie_sync_device_name = status.device_name.clone();
        settings.cookie_sync_api_token = status.api_token.clone();
        settings.save_to_file(&settings_path)?;
    }

    Ok(Json(PollPairingResponse {
        paired: status.paired,
        chat_id: status.chat_id,
        device_name: status.device_name,
    }))
}

#[derive(Serialize)]
struct SyncResponse {
    status: String,
    domains: Vec<String>,
}

async fn sync_all(
    StateExtractor(State { settings, .. }): StateExtractor<State>,
) -> Result<Json<SyncResponse>, AppError> {
    let (server_url, chat_id, device_name, api_token) = {
        let s = settings.lock().await;
        (
            s.cookie_sync_server_url.clone(),
            s.cookie_sync_chat_id,
            s.cookie_sync_device_name.clone(),
            s.cookie_sync_api_token.clone(),
        )
    };

    let server_url =
        server_url.ok_or_else(|| AppError::from(anyhow::anyhow!("not paired: no server URL")))?;
    let chat_id =
        chat_id.ok_or_else(|| AppError::from(anyhow::anyhow!("not paired: no chat_id")))?;
    let device_name = device_name
        .ok_or_else(|| AppError::from(anyhow::anyhow!("not paired: no device name")))?;

    let data = cookie_store::sync_all_cookies(&server_url, chat_id, &device_name, api_token.as_deref())
        .await
        .map_err(|e| AppError::from(e))?;

    let domains: Vec<String> = data.iter().map(|d| d.domain.clone()).collect();
    cookie_store::apply_synced_cookies(&data);

    Ok(Json(SyncResponse {
        status: "success".to_string(),
        domains,
    }))
}

#[derive(Serialize)]
struct UnpairResponse {
    status: String,
}

async fn unpair(
    StateExtractor(State {
        settings, settings_path, ..
    }): StateExtractor<State>,
) -> Result<Json<UnpairResponse>, AppError> {
    {
        let mut s = settings.lock().await;
        s.cookie_sync_server_url = None;
        s.cookie_sync_device_name = None;
        s.cookie_sync_chat_id = None;
        s.save_to_file(&settings_path)?;
    }
    if let Some(store) = cookie_store::global_cookie_store() {
        if let Ok(mut s) = store.write() {
            s.clear();
        }
        cookie_store::recompute_sync_hash();
    }
    cookie_store::save_cookies_to_disk();
    Ok(Json(UnpairResponse {
        status: "unpaired".to_string(),
    }))
}

#[derive(Serialize)]
struct CookieInfo {
    name: String,
    value: String,
    domain: String,
}

#[derive(Serialize)]
struct DomainCookies {
    cookies: Vec<CookieInfo>,
    user_agent: Option<String>,
}

#[derive(Serialize)]
struct ListCookiesResponse {
    domains: Vec<(String, DomainCookies)>,
}

async fn list_cookies() -> Json<ListCookiesResponse> {
    let domains = cookie_store::global_cookie_store()
        .and_then(|s| s.read().ok())
        .map(|s| {
            s.domains
                .iter()
                .map(|(domain, cookies)| {
                    let ua = s.user_agents.get(domain).cloned();
                    (
                        domain.clone(),
                        DomainCookies {
                            cookies: cookies
                                .iter()
                                .map(|c| CookieInfo {
                                    name: c.name.clone(),
                                    value: c.value.clone(),
                                    domain: c.domain.clone(),
                                })
                                .collect(),
                            user_agent: ua,
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Json(ListCookiesResponse { domains })
}
