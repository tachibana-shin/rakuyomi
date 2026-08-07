//! Client for LNReader's own upstream plugin discovery index
//! (`plugins.min.json`) — see `docs/lnreader/REFERENCE.md` §5 for the full
//! format/behavior writeup.
//!
//! Its own minimal `reqwest::blocking::Client` (this crate has no Tokio
//! runtime, unlike `shared`/`lnreader_worker` — `shared::tls`'s
//! `client_builder()` returns an async `reqwest::ClientBuilder`, not
//! directly usable here), reusing only the shared `DEFAULT_USER_AGENT`
//! string constant rather than `shared::tls`'s full TLS/proxy
//! configuration — see REFERENCE.md §5.4 for why that's an accepted
//! tradeoff for a maintainer-run offline tool, not an inconsistency to fix.
//!
//! RECONSTRUCTED after an accidental `git checkout` discarded this
//! file's uncommitted content — see `docs/lnreader/REFERENCE.md`'s
//! "File-loss incident" section for the full account. High confidence:
//! `fetch_index`/`fetch_plugin_js` and their signatures are named
//! explicitly in REFERENCE.md §3.3, `UpstreamIndexEntry`'s exact shape
//! survived intact in `shared::source::sdk_lnreader::packaging` (never
//! lost), and the "no hardcoded index URL" / "reuses only
//! `DEFAULT_USER_AGENT`" behaviors are both independently documented in
//! REFERENCE.md §5.1/§5.4. Not verified against the original byte-for-byte.

use anyhow::{Context, Result};
use shared::source::packaging::UpstreamIndexEntry;
use shared::source::wasm_imports::net::DEFAULT_USER_AGENT;

fn client() -> Result<reqwest::blocking::Client> {
    // `rustls-no-provider` (see Cargo.toml) means reqwest needs a crypto
    // provider installed before it can build a client; harmless to call
    // more than once (Err just means one's already installed).
    let _ = rustls::crypto::ring::default_provider().install_default();

    reqwest::blocking::Client::builder()
        .user_agent(DEFAULT_USER_AGENT)
        .build()
        .context("failed to build HTTP client")
}

/// Fetches and parses `plugins.min.json` at `index_url` into its list of
/// entries. No caller in this crate hardcodes `index_url` anywhere — see
/// `docs/lnreader/REFERENCE.md` §5.1's "no hardcoded URL, at either layer".
pub fn fetch_index(index_url: &str) -> Result<Vec<UpstreamIndexEntry>> {
    client()?
        .get(index_url)
        .send()
        .with_context(|| format!("failed to fetch index at {index_url}"))?
        .error_for_status()
        .with_context(|| format!("index request to {index_url} failed"))?
        .json()
        .with_context(|| format!("couldn't parse index at {index_url}"))
}

/// Downloads one entry's compiled `.js` by its own `url` field.
pub fn fetch_plugin_js(url: &str) -> Result<String> {
    client()?
        .get(url)
        .send()
        .with_context(|| format!("failed to download plugin source from {url}"))?
        .error_for_status()
        .with_context(|| format!("request to {url} failed"))?
        .text()
        .with_context(|| format!("couldn't read plugin source from {url}"))
}
