use std::collections::HashMap;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    /// RFC 6265 §5.1.3 domain matching.
    /// `stored` is the key in self.domains (e.g. `.example.com` or `exact.com`).
    /// `request` is the host from the URL (without leading dot).
    fn domain_matches(stored: &str, request: &str) -> bool {
        if stored == request {
            return true
        }
        if stored.starts_with('.') {
            let parent = &stored[1..];
            // Domain cookie: match parent itself or any subdomain
            if request == parent {
                return true
            }
            if request.ends_with(stored) {
                return true
            }
        }
        false
    }

    /// Collect all cookies whose domain matches `domain` per RFC 6265.
    /// Returns all matching cookies from all applicable domain entries.
    pub fn get_cookies_for_domain(&self, domain: &str) -> Vec<&CookieEntry> {
        let clean = domain.strip_prefix('.').unwrap_or(domain);
        let mut result = Vec::new();
        for (stored_domain, cookies) in &self.domains {
            if Self::domain_matches(stored_domain, clean) {
                result.extend(cookies.iter());
            }
        }
        result
    }

    /// Find the most specific User-Agent for `domain` per RFC 6265 domain matching.
    /// Prefers the longest matching stored domain.
    pub fn get_user_agent(&self, domain: &str) -> Option<&str> {
        let clean = domain.strip_prefix('.').unwrap_or(domain);
        let mut best: Option<&str> = None;
        let mut best_len: usize = 0;
        for (stored_domain, ua) in &self.user_agents {
            if Self::domain_matches(stored_domain, clean) {
                if stored_domain.len() > best_len {
                    best = Some(ua.as_str());
                    best_len = stored_domain.len();
                }
            }
        }
        best
    }

    pub fn set_cookies_for_domain(&mut self, domain: String, cookies: Vec<CookieEntry>) {
        self.domains.insert(domain, cookies);
    }

    pub fn set_user_agent(&mut self, domain: String, user_agent: String) {
        self.user_agents.insert(domain, user_agent);
    }

    pub fn clear(&mut self) {
        self.domains.clear();
        self.user_agents.clear();
    }

    pub fn domain_count(&self) -> usize {
        self.domains.len()
    }
}

/// Helper to get the User-Agent and Cookie header value for a given host from the global store.
pub fn get_user_agent_and_cookie_header(host: &str) -> (Option<String>, Option<String>) {
    global_cookie_store()
        .and_then(|s| s.read().ok())
        .map(|store| {
            let ua = store.get_user_agent(host).map(String::from);
            let cookies = store.get_cookies_for_domain(host);
            let cookie_val = if cookies.is_empty() {
                None
            } else {
                Some(
                    cookies
                        .iter()
                        .map(|c| format!("{}={}", c.name, c.value))
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            };
            (ua, cookie_val)
        })
        .unwrap_or((None, None))
}

static COOKIE_STORE: OnceLock<RwLock<CookieStoreData>> = OnceLock::new();
static COOKIE_STORE_PATH: OnceLock<String> = OnceLock::new();
static SYNC_HASH: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn sync_hash_from_store(store: &CookieStoreData) -> Option<String> {
    let json = serde_json::to_string(store).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let result = hasher.finalize();
    let bytes: [u8; 32] = result.into();
    Some(bytes.iter().map(|b| format!("{:02x}", b)).collect())
}

pub fn init_cookie_store() {
    let store = CookieStoreData::default();
    SYNC_HASH.get_or_init(|| RwLock::new(sync_hash_from_store(&store)));
    COOKIE_STORE.get_or_init(|| RwLock::new(store));
}

pub fn init_cookie_store_with_path(path: &Path) -> Result<()> {
    let store = CookieStoreData::load_from_file(path).unwrap_or_default();
    SYNC_HASH.get_or_init(|| RwLock::new(sync_hash_from_store(&store)));
    COOKIE_STORE
        .set(RwLock::new(store))
        .map_err(|_| anyhow::anyhow!("cookie store already initialized"))?;
    COOKIE_STORE_PATH
        .set(path.to_string_lossy().to_string())
        .map_err(|_| anyhow::anyhow!("cookie store path already set"))?;
    Ok(())
}

pub fn recompute_sync_hash() {
    if let Some(hash_lock) = SYNC_HASH.get() {
        if let Some(Ok(store)) = COOKIE_STORE.get().map(|s| s.read()) {
            if let Ok(mut h) = hash_lock.write() {
                *h = sync_hash_from_store(&store);
            }
        }
    }
}

pub fn global_cookie_store() -> Option<&'static RwLock<CookieStoreData>> {
    COOKIE_STORE.get()
}

pub fn save_cookies_to_disk() {
    let Some(path) = COOKIE_STORE_PATH.get() else { return };
    let Some(Ok(store)) = global_cookie_store().map(|s| s.read()) else { return };
    let _ = store.save_to_file(Path::new(path));
}

pub fn get_sync_hash() -> Option<String> {
    SYNC_HASH.get().and_then(|s| s.read().ok()).and_then(|h| h.clone())
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
    pub api_token: Option<String>,
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
        api_token: data["api_token"].as_str().map(String::from),
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
    api_token: Option<&str>,
) -> Result<Vec<SyncCookieData>> {
    let base = server_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base}/api/cookie/sync-all"))?;
    url.query_pairs_mut()
        .append_pair("chat_id", &chat_id.to_string())
        .append_pair("device", device_name);
    if let Some(h) = SYNC_HASH.get().and_then(|s| s.read().ok()).and_then(|h| h.clone()) {
        url.query_pairs_mut().append_pair("hash", &h);
    }
    let mut client_builder = client_builder();
    if let Some(token) = api_token {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .context("invalid API token")?,
        );
        client_builder = client_builder.default_headers(headers);
    }
    let client = client_builder.build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to sync cookies from {base}/api/cookie/sync-all"))?
        .error_for_status()
        .with_context(|| format!("sync-all request failed for {base}"))?;
    let data: serde_json::Value = resp
        .json()
        .await
        .with_context(|| "failed to parse sync-all response")?;

    let changed = data["changed"].as_bool().unwrap_or(true);
    let new_hash = data["hash"].as_str().and_then(|h| if h.is_empty() { None } else { Some(h.to_string()) });
    if let Some(ref h) = new_hash {
        if let Some(Ok(mut hash_stored)) = SYNC_HASH.get().map(|s| s.write()) {
            *hash_stored = Some(h.clone());
        }
    }

    if !changed {
        return Ok(Vec::new());
    }

    let payload = data["payload"]
        .as_object()
        .context("missing 'payload' object in sync-all response")?;

    let mut results = Vec::new();
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
    request_url: &str,
    api_token: Option<&str>,
) -> Result<()> {
    let base = server_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base}/api/cookie/notify-needs-update"))?;
    url.query_pairs_mut()
        .append_pair("chat_id", &chat_id.to_string())
        .append_pair("device", device_name)
        .append_pair("url", request_url);
    let mut client_builder = client_builder()
        .timeout(std::time::Duration::from_secs(10));
    if let Some(token) = api_token {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .context("invalid API token")?,
        );
        client_builder = client_builder.default_headers(headers);
    }
    let client = client_builder.build()?;
    client.get(url).send().await?.error_for_status()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_matches_exact() {
        assert!(CookieStoreData::domain_matches("example.com", "example.com"));
        assert!(CookieStoreData::domain_matches(".example.com", "example.com"));
        assert!(CookieStoreData::domain_matches(".example.com", "sub.example.com"));
    }

    #[test]
    fn test_domain_matches_no_match() {
        assert!(!CookieStoreData::domain_matches("example.com", "other.com"));
        assert!(!CookieStoreData::domain_matches("example.com", "sub.example.com"));
        assert!(!CookieStoreData::domain_matches("anotherexample.com", "example.com"));
    }

    #[test]
    fn test_domain_matches_deep_subdomain() {
        // Domain cookie .example.com should match a.b.c.d.example.com
        assert!(CookieStoreData::domain_matches(".example.com", "a.b.c.d.example.com"));
        // Host-only cookie example.com should NOT match subdomain
        assert!(!CookieStoreData::domain_matches("example.com", "a.b.c.d.example.com"));
    }

    #[test]
    fn test_domain_matches_same_suffix() {
        // .com should match stuff.com (permissive, but RFC-compliant if we don't have PSL)
        assert!(CookieStoreData::domain_matches(".com", "example.com"));
        assert!(!CookieStoreData::domain_matches("com", "example.com"));
    }

    #[test]
    fn test_get_cookies_for_domain_host_only() {
        let mut store = CookieStoreData::default();
        store.set_cookies_for_domain("exact.com".into(), vec![
            CookieEntry { name: "sess".into(), value: "abc".into(), domain: "exact.com".into(), path: None },
        ]);
        // Host-only cookie matches exact domain
        assert_eq!(store.get_cookies_for_domain("exact.com").len(), 1);
        // Host-only cookie does NOT match subdomain
        assert_eq!(store.get_cookies_for_domain("sub.exact.com").len(), 0);
    }

    #[test]
    fn test_get_cookies_for_domain_domain_cookie() {
        let mut store = CookieStoreData::default();
        store.set_cookies_for_domain(".example.com".into(), vec![
            CookieEntry { name: "cf".into(), value: "clearance".into(), domain: ".example.com".into(), path: None },
        ]);
        // Domain cookie matches the parent domain itself
        assert_eq!(store.get_cookies_for_domain("example.com").len(), 1);
        // Domain cookie matches subdomain
        assert_eq!(store.get_cookies_for_domain("sub.example.com").len(), 1);
        // Domain cookie matches deep subdomain
        assert_eq!(store.get_cookies_for_domain("a.b.c.example.com").len(), 1);
    }

    #[test]
    fn test_get_cookies_for_domain_merges_multiple_match() {
        let mut store = CookieStoreData::default();
        store.set_cookies_for_domain(".example.com".into(), vec![
            CookieEntry { name: "cf".into(), value: "clr".into(), domain: ".example.com".into(), path: None },
        ]);
        store.set_cookies_for_domain("sub.example.com".into(), vec![
            CookieEntry { name: "session".into(), value: "tok".into(), domain: "sub.example.com".into(), path: None },
        ]);
        let cookies = store.get_cookies_for_domain("sub.example.com");
        assert_eq!(cookies.len(), 2);
        let names: Vec<&str> = cookies.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"cf"));
        assert!(names.contains(&"session"));
    }

    #[test]
    fn test_get_user_agent_most_specific() {
        let mut store = CookieStoreData::default();
        store.set_user_agent(".example.com".into(), "Mozilla/5.0 Generic".into());
        store.set_user_agent("sub.example.com".into(), "Mozilla/5.0 Specific".into());
        // Should pick the most specific (longest stored domain)
        assert_eq!(store.get_user_agent("sub.example.com"), Some("Mozilla/5.0 Specific"));
        assert_eq!(store.get_user_agent("other.example.com"), Some("Mozilla/5.0 Generic"));
    }

    #[test]
    fn test_get_user_agent_host_only() {
        let mut store = CookieStoreData::default();
        store.set_user_agent("exact.com".into(), "Mozilla/5.0 Exact".into());
        assert_eq!(store.get_user_agent("exact.com"), Some("Mozilla/5.0 Exact"));
        assert_eq!(store.get_user_agent("sub.exact.com"), None);
    }

    #[test]
    fn test_preserves_leading_dot() {
        let mut store = CookieStoreData::default();
        store.set_cookies_for_domain(".example.com".into(), vec![]);
        assert!(store.domains.contains_key(".example.com"));
        assert!(!store.domains.contains_key("example.com"));

        store.set_user_agent(".example.com".into(), "UA".into());
        assert!(store.user_agents.contains_key(".example.com"));
    }
}
