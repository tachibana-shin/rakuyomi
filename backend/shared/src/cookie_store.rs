use std::collections::{BTreeMap, HashMap};
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
            return true;
        }
        if let Some(parent) = stored.strip_prefix('.') {
            // Domain cookie: match parent itself or any subdomain
            if request == parent {
                return true;
            }
            if request.ends_with(stored) {
                return true;
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
            if Self::domain_matches(stored_domain, clean) && stored_domain.len() > best_len {
                best = Some(ua.as_str());
                best_len = stored_domain.len();
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

/// The effect of one `Set-Cookie` header on the store: upsert an entry under
/// its effective domain key, or drop every entry with a given name.
#[derive(Debug)]
enum CookieAction {
    Set(String, CookieEntry),
    Delete(String, String),
}

/// Parses a single `Set-Cookie` header value per RFC 6265 (name=value plus
/// the Domain/Path/Max-Age/Expires/Secure attributes; quoted values are not
/// supported) into the action to apply to the store.
///
/// `host` is the request host and `secure` whether the request URL was
/// https; both shape the effective storage domain.
fn parse_set_cookie(header: &str, host: &str, secure: bool) -> Option<CookieAction> {
    let mut parts = header.split(';');
    let first = parts.next()?.trim();
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let value = value.trim().to_string();

    let mut domain = host.to_string();
    let mut path: Option<String> = None;
    let mut max_age: Option<i64> = None;
    let mut expires: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut secure_only = false;
    for part in parts {
        let (key, val) = match part.trim().split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), Some(v.trim())),
            None => (part.trim().to_ascii_lowercase(), None),
        };
        match key.as_str() {
            "domain" => {
                if let Some(d) = val {
                    domain = d.strip_prefix('.').unwrap_or(d).to_string();
                }
            }
            "path" => {
                if let Some(p) = val {
                    path = Some(p.to_string());
                }
            }
            "max-age" => {
                if let Some(v) = val.and_then(|v| v.parse().ok()) {
                    max_age = Some(v);
                }
            }
            "expires" => {
                if let Some(v) = val.and_then(|v| chrono::DateTime::parse_from_rfc2822(v).ok()) {
                    expires = Some(v.with_timezone(&chrono::Utc));
                }
            }
            "secure" => secure_only = true,
            _ => {}
        }
    }

    // Max-Age=0 (or negative) and an expiry in the past both mean "delete".
    let deletion = max_age.is_some_and(|a| a <= 0)
        || max_age.is_none() && expires.is_some_and(|e| e <= chrono::Utc::now());
    // Secure cookies must not be stored when the response arrived over http.
    if secure_only && !secure && !deletion {
        return None;
    }

    // Domain attribute => domain cookie keyed with a leading dot; otherwise
    // a host-only cookie keyed by the request host.
    let stored_key = if domain == host {
        host.to_string()
    } else {
        format!(".{domain}")
    };
    if deletion {
        return Some(CookieAction::Delete(stored_key, name.to_string()));
    }
    Some(CookieAction::Set(
        stored_key,
        CookieEntry {
            name: name.to_string(),
            value,
            domain,
            path,
        },
    ))
}

/// Anything that carries a final response URL and `Set-Cookie` headers: the
/// accepted input of [`record_response_cookies`]. Implemented for both
/// reqwest response flavours (async and blocking).
pub trait ResponseCookies {
    /// The final (post-redirect) response URL.
    fn response_url(&self) -> &Url;
    /// The raw `Set-Cookie` header values.
    fn set_cookie_headers(&self) -> Vec<String>;
}

impl ResponseCookies for reqwest::blocking::Response {
    fn response_url(&self) -> &Url {
        self.url()
    }

    fn set_cookie_headers(&self) -> Vec<String> {
        self.headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(String::from))
            .collect()
    }
}

impl ResponseCookies for reqwest::Response {
    fn response_url(&self) -> &Url {
        self.url()
    }

    fn set_cookie_headers(&self) -> Vec<String> {
        self.headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(String::from))
            .collect()
    }
}

/// Records the `Set-Cookie` headers of a response into the global store,
/// mirroring the capture reqwest's `cookie_store` does — but on the shared
/// store, so cookies persist across requests, engine instances and
/// backends. The response's final (post-redirect) URL decides the storage
/// domain.
pub fn record_response_cookies<R: ResponseCookies>(response: &R) {
    record_set_cookie_headers(response.response_url(), &response.set_cookie_headers());
}

/// Records already-extracted `Set-Cookie` header values against an explicit
/// final URL. Lower-level entry point used by tests and callers that do not
/// hold a reqwest response.
pub fn record_set_cookie_headers(url: &Url, set_cookie_headers: &[String]) {
    if set_cookie_headers.is_empty() {
        return;
    }
    let Some(host) = url.host_str() else {
        return;
    };
    let secure = url.scheme() == "https";
    let changed = {
        let Some(Ok(mut store)) = global_cookie_store().map(|s| s.write()) else {
            return;
        };
        let mut changed = false;
        for header in set_cookie_headers {
            match parse_set_cookie(header, host, secure) {
                Some(CookieAction::Set(key, entry)) => {
                    let cookies = store.domains.entry(key).or_default();
                    if let Some(existing) = cookies.iter_mut().find(|c| c.name == entry.name) {
                        *existing = entry;
                    } else {
                        cookies.push(entry);
                    }
                    changed = true;
                }
                Some(CookieAction::Delete(key, name)) => {
                    if let Some(cookies) = store.domains.get_mut(&key) {
                        let before = cookies.len();
                        cookies.retain(|c| c.name != name);
                        changed |= cookies.len() != before;
                    }
                }
                None => {}
            }
        }
        changed
    };
    if changed {
        save_cookies_to_disk();
    }
}

static COOKIE_STORE: OnceLock<RwLock<CookieStoreData>> = OnceLock::new();
static COOKIE_STORE_PATH: OnceLock<String> = OnceLock::new();
static SYNC_HASH: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn sync_hash_from_store(store: &CookieStoreData) -> Option<String> {
    // Convert HashMaps to BTreeMaps for deterministic serialization
    let domains: BTreeMap<_, _> = store.domains.iter().collect();
    let user_agents: BTreeMap<_, _> = store.user_agents.iter().collect();

    let canonical = serde_json::json!({
        "domains": domains,
        "user_agents": user_agents,
    });

    let json = serde_json::to_string(&canonical).ok()?;
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
    #[cfg(target_os = "android")]
    if COOKIE_STORE.get().is_some() {
        let new_path = path.to_string_lossy().to_string();
        match COOKIE_STORE_PATH.get() {
            Some(existing_path) if existing_path != &new_path => {
                return Err(anyhow::anyhow!(
                    "cookie store already initialized with a different path ({existing_path} != {new_path})"
                ));
            }
            Some(_) => {}
            None => {
                let _ = COOKIE_STORE_PATH.set(new_path);
            }
        }
        return Ok(());
    }

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
    let Some(path) = COOKIE_STORE_PATH.get() else {
        return;
    };
    let Some(Ok(store)) = global_cookie_store().map(|s| s.read()) else {
        return;
    };
    let _ = store.save_to_file(Path::new(path));
}

pub fn get_sync_hash() -> Option<String> {
    SYNC_HASH
        .get()
        .and_then(|s| s.read().ok())
        .and_then(|h| h.clone())
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
    let resp =
        client.get(url).send().await.with_context(|| {
            format!("failed to poll pairing status at {base}/api/pairing/status")
        })?;
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
    if let Some(h) = SYNC_HASH
        .get()
        .and_then(|s| s.read().ok())
        .and_then(|h| h.clone())
    {
        url.query_pairs_mut().append_pair("hash", &h);
    }
    let mut client_builder = client_builder().timeout(std::time::Duration::from_secs(30));
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
    let new_hash = data["hash"].as_str().and_then(|h| {
        if h.is_empty() {
            None
        } else {
            Some(h.to_string())
        }
    });
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
                            domain: c["domain"].as_str().unwrap_or(domain).to_string(),
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
    let mut client_builder = client_builder().timeout(std::time::Duration::from_secs(10));
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
        assert!(CookieStoreData::domain_matches(
            "example.com",
            "example.com"
        ));
        assert!(CookieStoreData::domain_matches(
            ".example.com",
            "example.com"
        ));
        assert!(CookieStoreData::domain_matches(
            ".example.com",
            "sub.example.com"
        ));
    }

    #[test]
    fn test_domain_matches_no_match() {
        assert!(!CookieStoreData::domain_matches("example.com", "other.com"));
        assert!(!CookieStoreData::domain_matches(
            "example.com",
            "sub.example.com"
        ));
        assert!(!CookieStoreData::domain_matches(
            "anotherexample.com",
            "example.com"
        ));
    }

    #[test]
    fn test_domain_matches_deep_subdomain() {
        // Domain cookie .example.com should match a.b.c.d.example.com
        assert!(CookieStoreData::domain_matches(
            ".example.com",
            "a.b.c.d.example.com"
        ));
        // Host-only cookie example.com should NOT match subdomain
        assert!(!CookieStoreData::domain_matches(
            "example.com",
            "a.b.c.d.example.com"
        ));
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
        store.set_cookies_for_domain(
            "exact.com".into(),
            vec![CookieEntry {
                name: "sess".into(),
                value: "abc".into(),
                domain: "exact.com".into(),
                path: None,
            }],
        );
        // Host-only cookie matches exact domain
        assert_eq!(store.get_cookies_for_domain("exact.com").len(), 1);
        // Host-only cookie does NOT match subdomain
        assert_eq!(store.get_cookies_for_domain("sub.exact.com").len(), 0);
    }

    #[test]
    fn test_get_cookies_for_domain_domain_cookie() {
        let mut store = CookieStoreData::default();
        store.set_cookies_for_domain(
            ".example.com".into(),
            vec![CookieEntry {
                name: "cf".into(),
                value: "clearance".into(),
                domain: ".example.com".into(),
                path: None,
            }],
        );
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
        store.set_cookies_for_domain(
            ".example.com".into(),
            vec![CookieEntry {
                name: "cf".into(),
                value: "clr".into(),
                domain: ".example.com".into(),
                path: None,
            }],
        );
        store.set_cookies_for_domain(
            "sub.example.com".into(),
            vec![CookieEntry {
                name: "session".into(),
                value: "tok".into(),
                domain: "sub.example.com".into(),
                path: None,
            }],
        );
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
        assert_eq!(
            store.get_user_agent("sub.example.com"),
            Some("Mozilla/5.0 Specific")
        );
        assert_eq!(
            store.get_user_agent("other.example.com"),
            Some("Mozilla/5.0 Generic")
        );
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

    #[test]
    fn test_parse_set_cookie_host_only() {
        let action = parse_set_cookie("session=abc123; Path=/; HttpOnly", "example.com", true)
            .expect("parseable");
        match action {
            CookieAction::Set(key, entry) => {
                assert_eq!(key, "example.com");
                assert_eq!(entry.name, "session");
                assert_eq!(entry.value, "abc123");
                assert_eq!(entry.path.as_deref(), Some("/"));
                assert_eq!(entry.domain, "example.com");
            }
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn test_parse_set_cookie_domain_attribute() {
        let action = parse_set_cookie(
            "cf=clearance; Domain=.example.com; Path=/; Secure",
            "sub.example.com",
            true,
        )
        .expect("parseable");
        match action {
            CookieAction::Set(key, entry) => {
                assert_eq!(key, ".example.com");
                assert_eq!(entry.name, "cf");
            }
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn test_parse_set_cookie_secure_over_http_ignored() {
        assert!(parse_set_cookie("tok=x; Secure", "example.com", false).is_none());
        assert!(parse_set_cookie("tok=x; Secure", "example.com", true).is_some());
    }

    #[test]
    fn test_parse_set_cookie_max_age_zero_deletes() {
        match parse_set_cookie("session=gone; Max-Age=0", "example.com", true) {
            Some(CookieAction::Delete(key, name)) => {
                assert_eq!(key, "example.com");
                assert_eq!(name, "session");
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_set_cookie_expired_deletes() {
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        let date = past.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        match parse_set_cookie(&format!("session=x; Expires={date}"), "example.com", true) {
            Some(CookieAction::Delete(key, name)) => {
                assert_eq!(key, "example.com");
                assert_eq!(name, "session");
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_set_cookie_future_expires_stores() {
        let future = chrono::Utc::now() + chrono::Duration::days(1);
        let date = future.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let action = parse_set_cookie(&format!("tok=y; Expires={date}"), "example.com", true)
            .expect("parseable");
        assert!(matches!(action, CookieAction::Set(..)));
    }

    #[test]
    fn test_record_response_cookies_upsert_and_delete() {
        init_cookie_store();
        let url = Url::parse("https://example.com/page").unwrap();

        record_set_cookie_headers(
            &url,
            &["a=1; Path=/".to_string(), "b=2; Path=/".to_string()],
        );
        let cookies = global_cookie_store()
            .and_then(|s| s.read().ok())
            .map(|s| s.get_cookies_for_domain("example.com").len())
            .unwrap();
        assert_eq!(cookies, 2);

        // Same-name cookie replaces the stored value.
        record_set_cookie_headers(&url, &["a=updated".to_string()]);
        let guard = global_cookie_store().and_then(|s| s.read().ok()).unwrap();
        let matched = guard.get_cookies_for_domain("example.com");
        let a = matched.iter().find(|c| c.name == "a").expect("cookie a");
        assert_eq!(a.value, "updated");
        drop(guard);
        record_set_cookie_headers(&url, &["b=x; Max-Age=0".to_string()]);
        let cookies = global_cookie_store()
            .and_then(|s| s.read().ok())
            .map(|s| s.get_cookies_for_domain("example.com").len())
            .unwrap();
        assert_eq!(cookies, 1);
        global_cookie_store()
            .and_then(|s| s.write().ok())
            .map(|mut s| s.clear());
    }
}
