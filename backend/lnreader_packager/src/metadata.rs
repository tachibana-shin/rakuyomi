//! Turns the raw JSON blob returned by
//! `shared::source::lnreader_extract_plugin_metadata` into the pieces
//! [`crate::package`] needs, plus the one semver -> `usize` conversion
//! Rakuyomi's `SourceInfo::version` needs that the shared crate has no
//! opinion on.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

/// Mirrors exactly what `sdk_lnreader::metadata::extract` asks the plugin
/// object for — see that function's doc comment for why this is read off
/// the live instance rather than parsed from source text (constructor-
/// parameterized plugins like `ranobes.js` don't have these as literal
/// string properties anywhere in their source).
#[derive(Debug, Deserialize)]
pub struct RawMetadata {
    pub id: String,
    pub name: String,
    pub site: Option<String>,
    pub lang: Option<String>,
    /// Plugins declare this as a semver string (`"2.1.3"`) in every real
    /// sample seen so far, but read as `Value` defensively in case a plugin
    /// ever has a numeric or missing version — see [`encode_version`].
    #[serde(default)]
    pub version: Value,
    #[serde(default)]
    pub filters: Value,
    #[serde(default, rename = "pluginSettings")]
    pub plugin_settings: Value,
}

impl RawMetadata {
    pub fn parse(raw: Value) -> Result<Self> {
        serde_json::from_value(raw).context("plugin metadata did not match the expected shape")
    }
}

/// Rakuyomi's `SourceInfo::version` is a plain `usize` used only to detect
/// updates (a newer package has a strictly greater number) — but LNReader
/// plugins declare a semver string. Encodes `major*1_000_000 +
/// minor*1_000 + patch` (each component capped at 999, arbitrary but
/// monotonic with real semver ordering for any sane version number), and
/// falls back to `1` for anything that isn't a recognizable `N`, `N.N`, or
/// `N.N.N` string — a plugin with a missing/odd version shouldn't block
/// packaging a source that otherwise works fine, it just won't compare
/// meaningfully against a later re-package until upstream fixes it.
pub fn encode_version(version: &Value) -> usize {
    let as_str = match version {
        Value::String(s) => Some(s.as_str()),
        Value::Null => None,
        _ => None,
    };

    let Some(s) = as_str else {
        return 1;
    };

    let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse::<u32>().ok()).collect();

    let component = |i: usize| parts.get(i).copied().unwrap_or(0).min(999) as usize;

    if parts.is_empty() {
        1
    } else {
        component(0) * 1_000_000 + component(1) * 1_000 + component(2)
    }
}
