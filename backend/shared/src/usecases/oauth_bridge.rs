use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::tls::client_builder;

/// The OAuth service type sent to the bridge server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OAuthService {
    Anilist,
    MyAnimeList,
    Shikimori,
    Bangumi,
    Mangabaka,
}

impl std::fmt::Display for OAuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthService::Anilist => write!(f, "anilist"),
            OAuthService::MyAnimeList => write!(f, "myanimelist"),
            OAuthService::Shikimori => write!(f, "shikimori"),
            OAuthService::Bangumi => write!(f, "bangumi"),
            OAuthService::Mangabaka => write!(f, "mangabaka"),
        }
    }
}

#[derive(Debug, Serialize)]
struct CreateSessionRequest {
    service: OAuthService,
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub bridge_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Pending,
    Completed,
    Error,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PollSessionResponse {
    Pending {
        service: OAuthService,
    },
    Completed {
        service: OAuthService,
        tokens: Option<OAuthTokens>,
    },
    Error {
        service: OAuthService,
        message: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OAuthTokens {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
}

/// Create an OAuth session on the bridge server.
///
/// Returns the session ID and the URL the user should open on their phone.
pub async fn start_oauth_session(
    server_url: &str,
    service: OAuthService,
    chat_id: Option<i64>,
    device_name: Option<String>,
) -> anyhow::Result<CreateSessionResponse> {
    let url = format!("{}/api/oauth/session", server_url.trim_end_matches('/'));
    let body = CreateSessionRequest {
        service,
        chat_id,
        device_name,
    };

    let client = client_builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client")?;

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .body(serde_json::to_string(&body)?)
        .send()
        .await
        .with_context(|| format!("failed to create OAuth session at {url}"))?
        .error_for_status()
        .with_context(|| format!("OAuth session creation failed at {url}"))?;

    let data: CreateSessionResponse = resp
        .json()
        .await
        .with_context(|| "failed to parse OAuth session response")?;

    Ok(data)
}

/// Poll the OAuth session status on the bridge server.
pub async fn poll_oauth_status(
    server_url: &str,
    session_id: &str,
    api_token: Option<&str>,
) -> anyhow::Result<PollSessionResponse> {
    let url = format!(
        "{}/api/oauth/status/{}",
        server_url.trim_end_matches('/'),
        session_id
    );

    let mut client_builder = client_builder().timeout(std::time::Duration::from_secs(10));
    if let Some(token) = api_token {
        client_builder =
            client_builder.default_headers(reqwest::header::HeaderMap::from_iter(vec![(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .context("invalid API token")?,
            )]));
    }
    let client = client_builder
        .build()
        .context("failed to build HTTP client")?;

    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to poll OAuth status at {url}"))?
        .error_for_status()
        .with_context(|| format!("OAuth status poll failed at {url}"))?;

    let data: PollSessionResponse = resp
        .json()
        .await
        .with_context(|| "failed to parse OAuth status response")?;

    Ok(data)
}
