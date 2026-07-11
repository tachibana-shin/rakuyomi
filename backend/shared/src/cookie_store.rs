use std::collections::HashMap;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::tls::client_builder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntry {
    pub name: String,
    pub value: String,
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCookieEntry {
    pub name: String,
    pub value: String,
    pub domain: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CookieStoreData {
    pub domains: HashMap<String, Vec<CookieEntry>>,
    pub user_agents: HashMap<String, String>,
}

impl CookieStoreData {
    pub fn get_cookies_for_domain(&self, domain: &str) -> Option<&Vec<CookieEntry>> {
        let domain = domain.strip_prefix('.').unwrap_or(domain);
        self.domains
            .get(domain)
            .or_else(|| self.domains.get(&format!(".{}", domain)))
            .or_else(|| {
                domain
                    .split_once('.')
                    .and_then(|(_, parent)| {
                        self.domains
                            .get(parent)
                            .or_else(|| self.domains.get(&format!(".{}", parent)))
                    })
            })
    }

    pub fn get_user_agent(&self, domain: &str) -> Option<&str> {
        let domain = domain.strip_prefix('.').unwrap_or(domain);
        self.user_agents
            .get(domain)
            .map(String::as_str)
            .or_else(|| {
                self.user_agents
                    .get(&format!(".{}", domain))
                    .map(String::as_str)
            })
            .or_else(|| {
                domain.split_once('.').and_then(|(_, parent)| {
                    self.user_agents
                        .get(parent)
                        .map(String::as_str)
                        .or_else(|| {
                            self.user_agents
                                .get(&format!(".{}", parent))
                                .map(String::as_str)
                        })
                })
            })
    }

    pub fn set_cookies_for_domain(&mut self, domain: String, cookies: Vec<CookieEntry>) {
        let clean_domain = domain.strip_prefix('.').unwrap_or(&domain).to_string();
        self.domains.insert(clean_domain, cookies);
    }

    pub fn set_user_agent(&mut self, domain: String, user_agent: String) {
        let clean_domain = domain.strip_prefix('.').unwrap_or(&domain).to_string();
        self.user_agents.insert(clean_domain, user_agent);
    }

    pub fn clear(&mut self) {
        self.domains.clear();
        self.user_agents.clear();
    }

    pub fn domain_count(&self) -> usize {
        self.domains.len()
    }
}

static COOKIE_STORE: OnceLock<RwLock<CookieStoreData>> = OnceLock::new();
static COOKIE_STORE_PATH: OnceLock<String> = OnceLock::new();

pub fn init_cookie_store() {
    COOKIE_STORE.get_or_init(|| RwLock::new(CookieStoreData::default()));
}

pub fn init_cookie_store_with_path(path: &Path) -> Result<()> {
    let store = CookieStoreData::load_from_file(path).unwrap_or_default();
    COOKIE_STORE
        .set(RwLock::new(store))
        .map_err(|_| anyhow::anyhow!("cookie store already initialized"))?;
    COOKIE_STORE_PATH
        .set(path.to_string_lossy().to_string())
        .map_err(|_| anyhow::anyhow!("cookie store path already set"))?;
    Ok(())
}

pub fn global_cookie_store() -> Option<&'static RwLock<CookieStoreData>> {
    COOKIE_STORE.get()
}

pub fn save_cookies_to_disk() {
    let Some(path) = COOKIE_STORE_PATH.get() else { return };
    let Some(Ok(store)) = global_cookie_store().map(|s| s.read()) else { return };
    let _ = store.save_to_file(Path::new(path));
}

impl CookieStoreData {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("couldn't open cookie file {}", path.display()))?;
        let store: CookieStoreData = serde_json_lenient::from_reader(file)
            .with_context(|| format!("couldn't parse cookie file {}", path.display()))?;
        Ok(store)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let file = std::fs::File::create(path)
            .with_context(|| format!("couldn't create cookie file {}", path.display()))?;
        serde_json_lenient::to_writer_pretty(file, self)
            .with_context(|| format!("couldn't write cookie file {}", path.display()))?;
        Ok(())
    }
}

pub async fn generate_pairing_code(server_url: &str) -> Result<String> {
    let url = format!("{}/api/pairing/generate", server_url.trim_end_matches('/'));
    let client = client_builder().build()?;
    let resp = client.get(&url).send().await?;
    let data: serde_json::Value = resp
        .json()
        .await
        .with_context(|| format!("failed to parse pairing response from {url}"))?;
    data["pairing_code"]
        .as_str()
        .map(String::from)
        .context("no pairing_code in response")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingStatus {
    pub paired: bool,
    pub chat_id: Option<i64>,
    pub device_name: Option<String>,
    pub token: Option<String>,
}

pub async fn poll_pairing_status(server_url: &str, code: &str) -> Result<PairingStatus> {
    let base = server_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base}/api/pairing/status"))?;
    url.query_pairs_mut().append_pair("code", code);
    let client = client_builder().build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to poll pairing status at {base}/api/pairing/status"))?;
    let data: serde_json::Value = resp
        .json()
        .await
        .with_context(|| "failed to parse pairing status response")?;
    Ok(PairingStatus {
        paired: data["paired"].as_bool().unwrap_or(false),
        chat_id: data["chat_id"].as_i64(),
        device_name: data["device_name"].as_str().map(String::from),
        token: data["token"].as_str().map(String::from),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCookieData {
    pub domain: String,
    pub cookies: Vec<SyncCookieEntry>,
    pub user_agent: Option<String>,
}

pub async fn sync_all_cookies(
    server_url: &str,
    chat_id: i64,
    device_name: &str,
    token: &str,
) -> Result<Vec<SyncCookieData>> {
    let base = server_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base}/api/cookie/sync-all"))?;
    url.query_pairs_mut()
        .append_pair("chat_id", &chat_id.to_string())
        .append_pair("device", device_name)
        .append_pair("token", token);
    let client = client_builder().build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to sync cookies from {base}/api/cookie/sync-all"))?;
    let data: serde_json::Value = resp
        .json()
        .await
        .with_context(|| "failed to parse sync-all response")?;

    let mut results = Vec::new();
    if let Some(payload) = data["payload"].as_object() {
        for (domain, info) in payload {
            let cookies = info["cookies"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            Some(SyncCookieEntry {
                                name: c["name"].as_str()?.to_string(),
                                value: c["value"].as_str()?.to_string(),
                                domain: c["domain"]
                                    .as_str()
                                    .unwrap_or(domain)
                                    .to_string(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let user_agent = info["user_agent"].as_str().map(String::from);
            results.push(SyncCookieData {
                domain: domain.clone(),
                cookies,
                user_agent,
            });
        }
    }
    Ok(results)
}

/// Notify the user via Telegram bot that cookies need to be refreshed.
///
/// Calls `{server_url}/api/cookie/notify-needs-update` — the Deno proxy server
/// handles forwarding the message to the user's Telegram chat.
pub async fn notify_cookie_needs_update(
    server_url: &str,
    chat_id: i64,
    device_name: &str,
    token: &str,
    request_url: &str,
) -> Result<()> {
    let base = server_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base}/api/cookie/notify-needs-update"))?;
    url.query_pairs_mut()
        .append_pair("chat_id", &chat_id.to_string())
        .append_pair("device", device_name)
        .append_pair("token", token)
        .append_pair("url", request_url);
    let client = client_builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    client.get(url).send().await?;
    Ok(())
}

pub fn apply_synced_cookies(data: &[SyncCookieData]) {
    let mut domain_count = 0;
    let mut cookie_count = 0;
    let mut ua_count = 0;
    {
        let Some(Ok(mut store)) = global_cookie_store().map(|s| s.write()) else {
            return;
        };
        for entry in data {
            let cookies: Vec<CookieEntry> = entry
                .cookies
                .iter()
                .map(|c| CookieEntry {
                    name: c.name.clone(),
                    value: c.value.clone(),
                    domain: c.domain.clone(),
                    path: None,
                })
                .collect();
            cookie_count += cookies.len();
            domain_count += 1;
            store.set_cookies_for_domain(entry.domain.clone(), cookies);
            if let Some(ref ua) = entry.user_agent {
                store.set_user_agent(entry.domain.clone(), ua.clone());
                ua_count += 1;
            }
        }
    }
    info!(
        "[cookie] applied sync: {} domains, {} cookies, {} user agents",
        domain_count, cookie_count, ua_count
    );
    save_cookies_to_disk();
}
