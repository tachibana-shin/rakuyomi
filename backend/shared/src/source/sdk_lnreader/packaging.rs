//! Packaging core: turns a compiled `lnreader-plugins` `.js` file into an
//! `.aix`-shaped archive. Shared by `lnreader_packager` (the standalone CLI
//! that pre-packages the upstream corpus for static hosting) and the
//! server's own on-demand install path (`usecases::install_source`), so a
//! plugin installed straight from `plugins.min.json` goes through the exact
//! same pipeline a `lnreader_packager fetch` run would — see
//! `docs/lnreader/REFERENCE.md` §5 for the end-to-end design.

use std::io::{Seek, Write};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use zip::write::SimpleFileOptions;

use super::super::model::SettingDefinition;
use super::super::{SourceInfo, SourceManifest};

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
        _ => None,
    };

    encode_version_str(as_str)
}

/// Same encoding as [`encode_version`], for callers that already have a
/// plain version string (e.g. `plugins.min.json`'s own `version` field —
/// see `UpstreamIndexEntry`) rather than a `serde_json::Value`.
pub fn encode_version_str(version: Option<&str>) -> usize {
    let Some(s) = version else {
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

/// Merges `filters` and `pluginSettings` into one flat list — Rakuyomi has
/// no separate "search filters" screen, so both just become source
/// settings. Unrecognized filter `type`s are skipped (not guessed at) and
/// reported back to the caller.
pub fn settings_from_plugin(
    filters: &Value,
    plugin_settings: &Value,
) -> (Vec<SettingDefinition>, Vec<String>) {
    let mut definitions = Vec::new();
    let mut skipped = Vec::new();

    for source in [filters, plugin_settings] {
        if let Some(map) = source.as_object() {
            for (key, filter) in map {
                match filter_to_setting(key, filter) {
                    Some(def) => definitions.push(def),
                    None => skipped.push(key.clone()),
                }
            }
        }
    }

    (definitions, skipped)
}

fn filter_to_setting(key: &str, filter: &Value) -> Option<SettingDefinition> {
    let obj = filter.as_object()?;
    let filter_type = obj.get("type")?.as_str()?;
    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_string();

    match filter_type {
        "Switch" => Some(SettingDefinition::Switch {
            title: label,
            key: key.to_string(),
            default: obj.get("value").and_then(Value::as_bool).unwrap_or(false),
        }),
        "Text" => Some(SettingDefinition::Text {
            placeholder: Some(label),
            key: key.to_string(),
            default: obj.get("value").and_then(Value::as_str).map(str::to_owned),
        }),
        "Picker" => {
            let options = string_options(obj)?;
            Some(SettingDefinition::Select {
                title: label,
                key: key.to_string(),
                default: obj.get("value").and_then(Value::as_str).map(str::to_owned),
                values: options.iter().map(|(value, _)| value.clone()).collect(),
                titles: Some(options.iter().map(|(_, title)| title.clone()).collect()),
            })
        }
        // `Checkbox`/`XCheckbox` (`CheckboxGroup`/`ExcludableCheckboxGroup`)
        // both become `MultiSelect` — Rakuyomi has no widget for the
        // "exclude" half of `ExcludableCheckboxGroup`; a deliberately lossy
        // mapping, not a bug (see `docs/lnreader/REFERENCE.md` §3.3).
        "Checkbox" | "XCheckbox" => {
            let options = string_options(obj)?;
            let default = obj
                .get("value")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Some(SettingDefinition::MultiSelect {
                title: label,
                key: key.to_string(),
                values: options.iter().map(|(value, _)| value.clone()).collect(),
                titles: Some(options.iter().map(|(_, title)| title.clone()).collect()),
                default,
            })
        }
        _ => None,
    }
}

fn string_options(obj: &serde_json::Map<String, Value>) -> Option<Vec<(String, String)>> {
    let options = obj.get("options")?.as_array()?;
    Some(
        options
            .iter()
            .filter_map(|option| {
                let label = option.get("label")?.as_str()?.to_string();
                let value = option.get("value")?.as_str()?.to_string();
                Some((value, label))
            })
            .collect(),
    )
}

pub struct SourceParams {
    pub id: String,
    pub name: String,
    pub lang: Option<String>,
    /// The plugin's base URL (`site`) — mapped to `SourceInfo::url`
    /// (singular). Multi-base-URL sources (`SourceInfo::urls`, which
    /// triggers a synthetic "URL" select setting in
    /// `LnReaderSource::from_aix_file`) aren't produced here: detecting
    /// that a plugin supports more than one site isn't something the
    /// metadata probe can tell generically.
    pub site: Option<String>,
    pub version: usize,
}

/// Writes an `.aix`-shaped zip to `writer`: `Payload/source.json` (always),
/// `Payload/settings.json` (only when `settings` is non-empty —
/// `LnReaderSource::from_aix_file` already treats a missing file as "no
/// settings"), and `Payload/main.js` (the input file, copied verbatim).
/// Generic over `Write + Seek` so callers can target either a file
/// (`lnreader_packager`, writing straight to disk) or an in-memory buffer
/// (the server's on-demand install path, which needs the bytes, not a
/// file).
pub fn write_aix<W: Write + Seek>(
    params: &SourceParams,
    settings: &[SettingDefinition],
    main_js: &str,
    writer: W,
) -> Result<W> {
    let manifest = SourceManifest {
        info: SourceInfo {
            id: params.id.clone(),
            lang: params.lang.clone(),
            name: params.name.clone(),
            version: params.version,
            url: params.site.clone(),
            urls: None,
            min_app_version: None,
        },
        config: None,
        source_of_source: None,
    };

    let mut zip = zip::ZipWriter::new(writer);
    let options = SimpleFileOptions::default();

    zip.start_file("Payload/source.json", options)
        .context("failed to start source.json entry")?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())
        .context("failed to write source.json")?;

    if !settings.is_empty() {
        zip.start_file("Payload/settings.json", options)
            .context("failed to start settings.json entry")?;
        zip.write_all(serde_json::to_string_pretty(settings)?.as_bytes())
            .context("failed to write settings.json")?;
    }

    zip.start_file("Payload/main.js", options)
        .context("failed to start main.js entry")?;
    zip.write_all(main_js.as_bytes())
        .context("failed to write main.js")?;

    zip.finish().context("failed to finalize .aix file")
}

/// One entry of `lnreader-plugins`' own upstream discovery index
/// (`plugins.min.json`) — the LNReader-side equivalent of Aidoku's
/// `index.min.json`. Real shape confirmed by fetching the live index (see
/// `docs/lnreader/REFERENCE.md` §5.2); unknown/absent fields deserialize as
/// `None` rather than failing the whole fetch, since this is third-party
/// data this project doesn't control the shape of.
#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamIndexEntry {
    pub id: String,
    pub name: String,
    pub site: Option<String>,
    /// Free-text, human-readable language name as the index author typed
    /// it ("English", "中文, 汉语, 漢語", "‎العربية" — the last with a stray
    /// Unicode direction-mark). Fine as an eventual UI display value, but
    /// **not** used as a mapping key anywhere in this module — see
    /// [`lang_from_index_url`] for why the URL's own folder segment is used
    /// instead.
    pub lang: Option<String>,
    pub version: Option<String>,
    /// Direct download link for this plugin's compiled `.js` — the one
    /// field this whole module exists to obtain, and also the input to
    /// [`lang_from_index_url`].
    pub url: String,
    /// Present upstream, but Rakuyomi has nowhere to put it: neither
    /// `SourceInfo`/`SourceManifest` nor `shared::model::SourceInformation`
    /// models a source icon at all today (Aidoku sources have no icon
    /// field either). Kept for completeness, not read by anything yet.
    #[allow(dead_code)]
    #[serde(rename = "iconUrl")]
    pub icon_url: Option<String>,
}

/// Maps the language-folder path segment `lnreader-plugins` groups its own
/// plugin sources under (`.../src/plugins/<folder>/PluginName.js`) to an
/// ISO-639-1 code. Built from every folder actually observed in the live
/// 261-entry index at the time this was written (see
/// `docs/lnreader/REFERENCE.md` §5.3) — used instead of
/// `UpstreamIndexEntry::lang` because that field is a free-text display
/// name (sometimes compound, e.g. "中文, 汉语, 漢語", and at least one entry
/// carries a stray Unicode direction-mark character), not a stable key
/// fit for exact-match lookup.
///
/// `"multi"` (currently just `komga`, a genuinely multi-language plugin) is
/// deliberately absent: no single ISO code applies, so a plugin under that
/// folder is left with no language fallback rather than forcing a wrong
/// one.
const LANG_FOLDERS: &[(&str, &str)] = &[
    ("arabic", "ar"),
    ("chinese", "zh"),
    ("english", "en"),
    ("french", "fr"),
    ("indonesian", "id"),
    ("japanese", "ja"),
    ("korean", "ko"),
    ("polish", "pl"),
    ("portuguese", "pt"),
    ("russian", "ru"),
    ("spanish", "es"),
    ("thai", "th"),
    ("turkish", "tr"),
    ("ukrainian", "uk"),
    ("vietnamese", "vi"),
];

/// Extracts the `<folder>` segment from a `.../src/plugins/<folder>/...`
/// URL and maps it through [`LANG_FOLDERS`]. Returns `None` for a URL that
/// doesn't match that shape at all, for the `multi` folder, or for a folder
/// not in the table (a new language folder `lnreader-plugins` adds after
/// this was written) — all three are meant to be handled the same way by
/// callers: skip the fallback, don't guess.
pub fn lang_from_index_url(url: &str) -> Option<&'static str> {
    let segments: Vec<&str> = url.split('/').collect();
    let folder = segments
        .windows(2)
        .position(|pair| pair[0] == "src" && pair[1] == "plugins")
        .and_then(|i| segments.get(i + 2))?;

    LANG_FOLDERS
        .iter()
        .find(|(f, _)| f == folder)
        .map(|(_, code)| *code)
}

/// Result of running a plugin's `.js` through the full packaging pipeline —
/// everything a caller needs to write the `.aix` to disk (`bytes`) and to
/// report what was packaged, without re-deriving it from the archive.
pub struct PackagedPlugin {
    pub bytes: Vec<u8>,
    pub id: String,
    pub name: String,
    pub site: Option<String>,
    pub lang: Option<String>,
    pub version: usize,
    pub settings_count: usize,
    pub skipped_filters: Vec<String>,
}

/// Executes `main_js` to read its own declared metadata (authoritative over
/// anything an index claims — see `docs/lnreader/REFERENCE.md` §5.2's
/// field-coverage table), builds its settings, and assembles the `.aix`
/// bytes. Shared by `lnreader_packager`'s `package`/`fetch` commands and the
/// server's on-demand install path (`usecases::install_source`), so both
/// produce byte-identical output for the same input.
///
/// `index_url` is the entry's own `UpstreamIndexEntry::url` when packaging
/// was triggered by an index (the `fetch` CLI command, or an on-demand
/// server install) — `None` for a bare local-file `package` invocation with
/// no index context. When given, and the plugin doesn't declare its own
/// `lang`, [`lang_from_index_url`] is used as a fallback (see
/// `docs/lnreader/REFERENCE.md` §5.3).
pub fn package_plugin_js(main_js: &str, index_url: Option<&str>) -> Result<PackagedPlugin> {
    let raw_json = super::metadata::extract(main_js)
        .context("couldn't execute plugin to read its metadata")?;
    let mut raw = RawMetadata::parse(raw_json)?;

    if raw.lang.as_deref().is_none_or(str::is_empty) {
        raw.lang = index_url.and_then(lang_from_index_url).map(str::to_owned);
    }

    let (setting_definitions, skipped_filters) =
        settings_from_plugin(&raw.filters, &raw.plugin_settings);

    let version = encode_version(&raw.version);
    let params = SourceParams {
        id: raw.id.clone(),
        name: raw.name.clone(),
        lang: raw.lang.clone(),
        site: raw.site.clone(),
        version,
    };

    let bytes = write_aix(
        &params,
        &setting_definitions,
        main_js,
        std::io::Cursor::new(Vec::new()),
    )?
    .into_inner();

    Ok(PackagedPlugin {
        bytes,
        id: raw.id,
        name: raw.name,
        site: raw.site,
        lang: raw.lang,
        version,
        settings_count: setting_definitions.len(),
        skipped_filters,
    })
}

/// On-demand install counterpart to [`package_plugin_js`]: downloads a
/// compiled LNReader plugin `.js` from `url`, packages it, and returns the
/// `.aix` bytes ready for `SourceManager::install_source`. Used by
/// `usecases::install_source` for its `SourceListItem::LnReaderRaw` case —
/// the single call site this exists for, folding the runtime-toggle check,
/// the download, and the `skipped_filters` warning together so that call
/// site doesn't carry any of this orchestration inline.
pub async fn install_from_url(url: &str, lnreader_enabled: bool) -> Result<Vec<u8>> {
    if !crate::source::lnreader_mode_enabled(lnreader_enabled) {
        anyhow::bail!(
            "LNReader support is disabled (set `lnreader_enabled: true` in the server settings and restart to enable it)"
        );
    }

    let client = crate::tls::client_builder().build()?;
    let main_js = client
        .get(url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("failed to download plugin source from {url}"))?
        .text()
        .await?;

    let packaged =
        package_plugin_js(&main_js, Some(url)).with_context(|| format!("couldn't package LNReader plugin from {url}"))?;

    if !packaged.skipped_filters.is_empty() {
        log::warn!(
            "{}: unrecognized filter/setting type(s), skipped: {}",
            packaged.id,
            packaged.skipped_filters.join(", ")
        );
    }

    Ok(packaged.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_from_index_url_maps_known_folders() {
        assert_eq!(
            lang_from_index_url(
                "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/arabic/ArNovel[madara].js"
            ),
            Some("ar")
        );
        assert_eq!(
            lang_from_index_url(
                "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/english/novelbuddy.js"
            ),
            Some("en")
        );
    }

    #[test]
    fn lang_from_index_url_has_no_entry_for_multi() {
        assert_eq!(
            lang_from_index_url(
                "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/multi/komga.js"
            ),
            None
        );
    }

    #[test]
    fn lang_from_index_url_returns_none_for_unrecognized_shape() {
        assert_eq!(lang_from_index_url("https://example.com/plugin.js"), None);
    }

    #[test]
    fn encode_version_str_parses_semver() {
        assert_eq!(encode_version_str(Some("2.1.3")), 2_001_003);
        assert_eq!(encode_version_str(Some("1")), 1_000_000);
        assert_eq!(encode_version_str(None), 1);
        assert_eq!(encode_version_str(Some("not-a-version")), 1);
    }
}
