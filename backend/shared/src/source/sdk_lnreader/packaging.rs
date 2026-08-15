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

fn display_version(version: &Value) -> Option<String> {
    version
        .as_str()
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
}

/// Same encoding as [`encode_version`], for callers that already have a
/// plain version string (e.g. `plugins.min.json`'s own `version` field —
/// see `UpstreamIndexEntry`) rather than a `serde_json::Value`.
pub fn encode_version_str(version: Option<&str>) -> usize {
    const FALLBACK: usize = 1;

    let Some(s) = version else {
        return FALLBACK;
    };

    // Reject the whole version on any invalid/empty component or more than
    // 3 of them, rather than silently dropping just the bad ones -- e.g.
    // "2.x.3" used to encode as if it were "2.3" (the invalid middle
    // component vanishing shifts "3" into the minor slot), a plausible but
    // wrong version rather than the documented fallback for something that
    // doesn't parse.
    let Ok(parts) = s
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<u32>, _>>()
    else {
        return FALLBACK;
    };
    if parts.is_empty() || parts.len() > 3 {
        return FALLBACK;
    }

    let component = |i: usize| parts.get(i).copied().unwrap_or(0).min(999) as usize;

    component(0) * 1_000_000 + component(1) * 1_000 + component(2)
}

/// Converts supported `pluginSettings` into source settings. Standard
/// browse/popular `filters` are intentionally ignored because Rakuyomi uses
/// `searchNovels` and does not apply those controls.
pub fn settings_from_plugin(
    _filters: &Value,
    plugin_settings: &Value,
) -> (Vec<SettingDefinition>, Vec<String>) {
    let mut definitions = Vec::new();
    let mut skipped = Vec::new();

    if let Some(map) = plugin_settings.as_object() {
        for (key, setting) in map {
            let defs = filter_to_setting(key, setting);
            if defs.is_empty() {
                skipped.push(key.clone());
            } else {
                definitions.extend(defs);
            }
        }
    }

    (definitions, skipped)
}

fn filter_to_setting(key: &str, filter: &Value) -> Vec<SettingDefinition> {
    let Some(obj) = filter.as_object() else {
        return Vec::new();
    };
    let Some(filter_type) = obj.get("type").and_then(Value::as_str) else {
        return Vec::new();
    };
    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_string();

    match filter_type {
        "Switch" => vec![SettingDefinition::Switch {
            title: label,
            key: key.to_string(),
            default: obj.get("value").and_then(Value::as_bool).unwrap_or(false),
        }],
        "Text" => vec![SettingDefinition::Text {
            placeholder: Some(label),
            key: key.to_string(),
            default: obj.get("value").and_then(Value::as_str).map(str::to_owned),
        }],
        "Picker" => {
            let Some(options) = string_options(obj) else {
                return Vec::new();
            };
            vec![SettingDefinition::Select {
                title: label,
                key: key.to_string(),
                default: obj.get("value").and_then(Value::as_str).map(str::to_owned),
                values: options.iter().map(|(value, _)| value.clone()).collect(),
                titles: Some(options.iter().map(|(_, title)| title.clone()).collect()),
            }]
        }
        // `CheckboxGroup` becomes a single `MultiSelect` (include-only).
        "Checkbox" => {
            let Some(options) = string_options(obj) else {
                return Vec::new();
            };
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
            vec![SettingDefinition::MultiSelect {
                title: label,
                key: key.to_string(),
                values: options.iter().map(|(value, _)| value.clone()).collect(),
                titles: Some(options.iter().map(|(_, title)| title.clone()).collect()),
                default,
            }]
        }
        // `ExcludableCheckboxGroup` (`XCheckbox`): tri-state filter.
        // Real encoding is `{include: [], exclude: []}`, not 0/1/2
        // (corpus-verified: lightnovelworld, novelbuddy, scribblehub, ...).
        // Produces two `MultiSelect` settings; the JS storage polyfill
        // recombines them — see `RUNTIME_PRELUDE` in `js_runtime.rs`.
        // Each half's default comes from its own side of `value`, so the
        // recombination in `JsRuntime::apply_settings_filters` reproduces the
        // plugin's declared initial include/exclude instead of replacing it
        // with empty lists on the first (unmodified) run.
        "XCheckbox" => {
            let Some(options) = string_options(obj) else {
                return Vec::new();
            };
            let values: Vec<String> = options.iter().map(|(value, _)| value.clone()).collect();
            let titles: Option<Vec<String>> =
                Some(options.iter().map(|(_, title)| title.clone()).collect());
            vec![
                SettingDefinition::MultiSelect {
                    title: format!("{label} (include)"),
                    key: format!("{key}__include"),
                    values: values.clone(),
                    titles: titles.clone(),
                    default: xcheckbox_value_default(obj, "include"),
                },
                SettingDefinition::MultiSelect {
                    title: format!("{label} (exclude)"),
                    key: format!("{key}__exclude"),
                    values,
                    titles,
                    default: xcheckbox_value_default(obj, "exclude"),
                },
            ]
        }
        _ => Vec::new(),
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

/// Reads one half (`include` or `exclude`) of an `ExcludableCheckboxGroup`
/// (`XCheckbox`) filter's real `value: { include: [...], exclude: [...] }`
/// shape as the `MultiSelect` default for that half. The halves propagate
/// independently and defensively: a missing `value` object, a missing side,
/// or non-string entries all read as "empty" rather than inventing a default
/// the plugin didn't declare. This is what keeps
/// `JsRuntime::apply_settings_filters` from erasing the plugin's initial
/// values — its recombination reads the settings snapshot, which always
/// carries the `MultiSelect` defaults, so those defaults must match the
/// plugin's declared `value` halves.
fn xcheckbox_value_default(obj: &serde_json::Map<String, Value>, field: &str) -> Vec<String> {
    obj.get("value")
        .and_then(Value::as_object)
        .and_then(|value| value.get(field))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The manifest fields [`write_aix`] needs to assemble `Payload/source.json`
/// -- a plain data carrier, not tied to any one caller's source shape
/// (`package_plugin_js`'s freshly extracted metadata, or a caller supplying
/// its own values directly).
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
    pub display_version: Option<String>,
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
            display_version: params.display_version.clone(),
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
    /// it ("English", "中文, 汉语, 漢語", "العربية" — the last preceded in
    /// the real upstream index by a stray U+200E LEFT-TO-RIGHT MARK).
    /// Fine as an eventual UI display value, but
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
/// `"multi"` remains explicit rather than being conflated with an unknown
/// language, so callers may deliberately treat it differently.
const LANG_FOLDERS: &[(&str, &str)] = &[
    ("albanian", "sq"),
    ("arabic", "ar"),
    ("azerbaijani", "az"),
    ("bengali", "bn"),
    ("bulgarian", "bg"),
    ("burmese", "my"),
    ("catalan", "ca"),
    ("cebuano", "ceb"),
    ("chinese", "zh"),
    ("croatian", "hr"),
    ("czech", "cs"),
    ("danish", "da"),
    ("dutch", "nl"),
    ("english", "en"),
    ("esperanto", "eo"),
    ("estonian", "et"),
    ("filipino", "fil"),
    ("finnish", "fi"),
    ("french", "fr"),
    ("georgian", "ka"),
    ("german", "de"),
    ("greek", "el"),
    ("hebrew", "he"),
    ("hindi", "hi"),
    ("hungarian", "hu"),
    ("indonesian", "id"),
    ("italian", "it"),
    ("japanese", "ja"),
    ("javanese", "jv"),
    ("kazakh", "kk"),
    ("korean", "ko"),
    ("latin", "la"),
    ("lithuanian", "lt"),
    ("malay", "ms"),
    ("mongolian", "mn"),
    ("nepali", "ne"),
    ("norwegian", "no"),
    ("persian", "fa"),
    ("polish", "pl"),
    ("portuguese", "pt"),
    ("romanian", "ro"),
    ("russian", "ru"),
    ("serbian", "sr"),
    ("slovak", "sk"),
    ("slovenian", "sl"),
    ("spanish", "es"),
    ("swedish", "sv"),
    ("tamil", "ta"),
    ("telugu", "te"),
    ("thai", "th"),
    ("turkish", "tr"),
    ("ukrainian", "uk"),
    ("vietnamese", "vi"),
    ("multi", "multi"),
];

/// Extracts the `<folder>` segment from a `.../src/plugins/<folder>/...`
/// URL and maps it through [`LANG_FOLDERS`]. Returns `None` for a URL that
/// doesn't match that shape at all, or for a folder not in the table (a new
/// language folder `lnreader-plugins` adds after
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
/// `Serialize`/`Deserialize` exist for `lnreader_packager`'s own
/// self-re-exec metadata-extraction subprocess (see that crate's
/// `package_plugin_js_with_timeout`), not for any on-disk format -- `bytes`
/// round-trips through plain JSON as a number array there, not base64,
/// since that IPC boundary isn't performance-sensitive.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PackagedPlugin {
    pub bytes: Vec<u8>,
    pub id: String,
    pub name: String,
    pub site: Option<String>,
    pub lang: Option<String>,
    pub version: usize,
    pub settings_count: usize,
    pub skipped_plugin_settings: Vec<String>,
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

    let (setting_definitions, skipped_plugin_settings) =
        settings_from_plugin(&raw.filters, &raw.plugin_settings);

    let version = encode_version(&raw.version);
    let params = SourceParams {
        id: raw.id.clone(),
        name: raw.name.clone(),
        lang: raw.lang.clone(),
        site: raw.site.clone(),
        version,
        display_version: display_version(&raw.version),
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
        skipped_plugin_settings,
    })
}

/// On-demand install counterpart to [`package_plugin_js`]: downloads a
/// compiled LNReader plugin `.js` from `url`, packages it, and returns the
/// `.aix` bytes ready for `SourceManager::install_source`. Used by
/// `usecases::install_source` for its `SourceListItem::LnReaderRaw` case —
/// the single call site this exists for, folding the runtime-toggle check,
/// the download, and the `skipped_plugin_settings` warning together so that call
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

    // `package_plugin_js` runs the plugin's top-level JS in-process to read
    // its metadata (see its own doc comment) -- unlike every other LNReader
    // JS execution path, this one isn't isolated in the worker subprocess
    // (that isolation exists specifically because a large/malicious catalog
    // can crash the process running it, see this module's own callers), so
    // a plugin with a pathological/infinite top-level loop could otherwise
    // hang this install request forever on a live server. `spawn_blocking`
    // at least keeps that off the async runtime's worker threads (onto the
    // already-capped blocking pool, see `server::main`'s
    // `max_blocking_threads`), and the timeout turns an indefinite hang
    // into a catchable error -- it does not protect against a native
    // crash/OOM inside `boa_engine` during extraction, which still takes
    // the whole server down; full subprocess isolation for this path is a
    // real gap, just too large a change for this pass.
    //
    // The timeout also does NOT cancel the `spawn_blocking` closure itself
    // -- Tokio has no way to interrupt a blocking OS thread from outside --
    // so a plugin that hangs past the timeout permanently occupies one
    // blocking-pool slot for as long as the server process runs, not just
    // for this one request. `METADATA_EXTRACTION_PERMITS` bounds how many
    // such leaked extractions can accumulate before *new* install attempts
    // start queuing on the semaphore instead of leaking further slots -- it
    // doesn't fix the leak, but it keeps a string of bad installs from
    // eventually exhausting the whole blocking pool (which every other
    // `Source` operation also shares). The permit is moved into the
    // `spawn_blocking` closure itself (an owned permit off an `Arc`, not a
    // borrowed one held by this `async fn`'s own scope) specifically so it's
    // only released when that closure actually returns -- however long that
    // takes -- not when this function gives up waiting on it at the
    // timeout.
    const METADATA_EXTRACTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
    const METADATA_EXTRACTION_PERMITS: usize = 2;
    static METADATA_EXTRACTION_SEMAPHORE: std::sync::LazyLock<
        std::sync::Arc<tokio::sync::Semaphore>,
    > = std::sync::LazyLock::new(|| {
        std::sync::Arc::new(tokio::sync::Semaphore::new(METADATA_EXTRACTION_PERMITS))
    });
    let permit = METADATA_EXTRACTION_SEMAPHORE
        .clone()
        .acquire_owned()
        .await
        .context("metadata extraction semaphore closed unexpectedly")?;

    let index_url = url.to_string();
    let packaged = tokio::time::timeout(
        METADATA_EXTRACTION_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            package_plugin_js(&main_js, Some(&index_url))
        }),
    )
    .await
    .with_context(|| {
        format!(
            "timed out packaging LNReader plugin from {url} after {METADATA_EXTRACTION_TIMEOUT:?}"
        )
    })?
    .context("packaging task panicked")?
    .with_context(|| format!("couldn't package LNReader plugin from {url}"))?;

    if !packaged.skipped_plugin_settings.is_empty() {
        log::warn!(
            "{}: unsupported pluginSetting type(s), skipped: {}",
            packaged.id,
            packaged.skipped_plugin_settings.join(", ")
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
    fn lang_from_index_url_maps_multi_explicitly() {
        assert_eq!(
            lang_from_index_url(
                "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.js/src/plugins/multi/komga.js"
            ),
            Some("multi")
        );
    }

    #[test]
    fn lang_from_index_url_maps_extended_latin_names() {
        assert_eq!(
            lang_from_index_url("https://example.com/src/plugins/german/example.js"),
            Some("de")
        );
        assert_eq!(
            lang_from_index_url("https://example.com/src/plugins/filipino/example.js"),
            Some("fil")
        );
    }

    #[test]
    fn ignored_filters_do_not_create_settings_or_warnings() {
        let filters = serde_json::json!({
            "status": {
                "type": "Checkbox",
                "options": [{"label": "Done", "value": "done"}]
            }
        });
        let (definitions, skipped) = settings_from_plugin(&filters, &serde_json::json!({}));
        assert!(definitions.is_empty());
        assert!(skipped.is_empty());
    }

    #[test]
    fn supported_plugin_settings_are_packaged() {
        let settings = serde_json::json!({
            "token": {"type": "Text", "label": "Token", "value": ""}
        });
        let (definitions, skipped) = settings_from_plugin(&serde_json::json!({}), &settings);
        assert_eq!(definitions.len(), 1);
        assert!(skipped.is_empty());
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

    #[test]
    fn encode_version_str_falls_back_on_partially_invalid_or_overlong_versions() {
        // Regression: an invalid component used to be silently dropped
        // instead of rejecting the whole version, shifting later
        // components into the wrong slot ("2.x.3" encoding as if it were
        // "2.3", not the documented fallback).
        assert_eq!(encode_version_str(Some("2.x.3")), 1);
        assert_eq!(encode_version_str(Some("1.2.3.4")), 1);
        assert_eq!(encode_version_str(Some("")), 1);
        assert_eq!(encode_version_str(Some(".")), 1);
    }

    #[test]
    fn raw_display_version_is_preserved_separately_from_numeric_version() {
        let raw = serde_json::json!("2.1.3");
        assert_eq!(encode_version(&raw), 2_001_003);
        assert_eq!(display_version(&raw).as_deref(), Some("2.1.3"));
        assert_eq!(display_version(&serde_json::json!("")), None);
    }

    #[test]
    fn xcheckbox_produces_two_multiselect_settings() {
        let filter = serde_json::json!({
            "label": "Genres",
            "type": "XCheckbox",
            "value": {"include": ["Action", "Romance"], "exclude": ["Comedy"]},
            "options": [
                {"label": "Action", "value": "Action"},
                {"label": "Romance", "value": "Romance"},
                {"label": "Comedy", "value": "Comedy"}
            ]
        });

        let defs = filter_to_setting("genres", &filter);
        assert_eq!(defs.len(), 2);

        // Each half's default propagates from its own side of `value` — the
        // settings snapshot then feeds those back through
        // `JsRuntime::apply_settings_filters`, so the plugin's declared
        // initial include/exclude survive an unmodified run.
        match &defs[0] {
            SettingDefinition::MultiSelect {
                title,
                key,
                values,
                titles,
                default,
            } => {
                assert_eq!(title, "Genres (include)");
                assert_eq!(key, "genres__include");
                assert_eq!(values, &["Action", "Romance", "Comedy"]);
                assert_eq!(titles.as_ref().unwrap(), &["Action", "Romance", "Comedy"]);
                assert_eq!(default, &["Action", "Romance"]);
            }
            other => panic!("expected MultiSelect, got {other:?}"),
        }

        match &defs[1] {
            SettingDefinition::MultiSelect {
                title,
                key,
                values,
                titles,
                default,
            } => {
                assert_eq!(title, "Genres (exclude)");
                assert_eq!(key, "genres__exclude");
                assert_eq!(values, &["Action", "Romance", "Comedy"]);
                assert_eq!(titles.as_ref().unwrap(), &["Action", "Romance", "Comedy"]);
                assert_eq!(default, &["Comedy"]);
            }
            other => panic!("expected MultiSelect, got {other:?}"),
        }
    }

    #[test]
    fn checkbox_produces_single_multiselect_unchanged() {
        let filter = serde_json::json!({
            "label": "Status",
            "type": "Checkbox",
            "value": ["ongoing", "completed"],
            "options": [
                {"label": "Ongoing", "value": "ongoing"},
                {"label": "Completed", "value": "completed"}
            ]
        });

        let defs = filter_to_setting("status", &filter);
        assert_eq!(defs.len(), 1);

        match &defs[0] {
            SettingDefinition::MultiSelect {
                title,
                key,
                values,
                default,
                ..
            } => {
                assert_eq!(title, "Status");
                assert_eq!(key, "status");
                assert_eq!(values, &["ongoing", "completed"]);
                assert_eq!(default, &["ongoing", "completed"]);
            }
            other => panic!("expected MultiSelect, got {other:?}"),
        }
    }

    #[test]
    fn xcheckbox_with_real_world_options() {
        // Real shape from lightnovelworld.js corpus
        let filter = serde_json::json!({
            "label": "Genres",
            "value": {"include": [], "exclude": []},
            "options": [
                {"label": "Action", "value": "Action"},
                {"label": "Adult", "value": "Adult"},
                {"label": "Adventure", "value": "Adventure"},
                {"label": "Comedy", "value": "Comedy"},
                {"label": "Drama", "value": "Drama"},
                {"label": "Fantasy", "value": "Fantasy"},
                {"label": "Harem", "value": "Harem"},
                {"label": "Romance", "value": "Romance"}
            ],
            "type": "XCheckbox"
        });

        let defs = filter_to_setting("genres", &filter);
        assert_eq!(defs.len(), 2);

        // Both should share the same options list
        let include_values = match &defs[0] {
            SettingDefinition::MultiSelect { values, .. } => values.clone(),
            other => panic!("expected MultiSelect, got {other:?}"),
        };
        let exclude_values = match &defs[1] {
            SettingDefinition::MultiSelect { values, .. } => values.clone(),
            other => panic!("expected MultiSelect, got {other:?}"),
        };
        assert_eq!(include_values, exclude_values);
        assert_eq!(include_values.len(), 8);
    }

    #[test]
    fn xcheckbox_partial_defaults_propagate_independently() {
        // Partial include/exclude: each half reads only its own side of
        // `value`, and a missing side (or a missing `value` object) stays
        // empty rather than being invented.
        let include_only = serde_json::json!({
            "label": "Genres",
            "type": "XCheckbox",
            "value": {"include": ["Action"]},
            "options": [{"label": "Action", "value": "Action"}]
        });
        let defs = filter_to_setting("genres", &include_only);
        assert_eq!(defs.len(), 2);
        match &defs[0] {
            SettingDefinition::MultiSelect { default, .. } => {
                assert_eq!(default, &["Action"]);
            }
            other => panic!("expected MultiSelect, got {other:?}"),
        }
        match &defs[1] {
            SettingDefinition::MultiSelect { default, .. } => {
                assert!(default.is_empty());
            }
            other => panic!("expected MultiSelect, got {other:?}"),
        }

        let exclude_only = serde_json::json!({
            "label": "Genres",
            "type": "XCheckbox",
            "value": {"exclude": ["Adult"]},
            "options": [{"label": "Adult", "value": "Adult"}]
        });
        let defs = filter_to_setting("genres", &exclude_only);
        match &defs[0] {
            SettingDefinition::MultiSelect { default, .. } => {
                assert!(default.is_empty());
            }
            other => panic!("expected MultiSelect, got {other:?}"),
        }
        match &defs[1] {
            SettingDefinition::MultiSelect { default, .. } => {
                assert_eq!(default, &["Adult"]);
            }
            other => panic!("expected MultiSelect, got {other:?}"),
        }

        // No `value` at all (or a non-object value): both halves stay empty.
        let no_value = serde_json::json!({
            "label": "Genres",
            "type": "XCheckbox",
            "options": [{"label": "Action", "value": "Action"}]
        });
        let defs = filter_to_setting("genres", &no_value);
        assert_eq!(defs.len(), 2);
        for def in &defs {
            match def {
                SettingDefinition::MultiSelect { default, .. } => {
                    assert!(default.is_empty());
                }
                other => panic!("expected MultiSelect, got {other:?}"),
            }
        }
    }

    #[test]
    fn xcheckbox_unmodified_settings_recombine_to_plugin_defaults() {
        // The two `MultiSelect` defaults are exactly what the settings
        // snapshot hands `JsRuntime::apply_settings_filters` on a run where
        // the user changed nothing — they must reproduce the plugin's
        // declared `value.include`/`value.exclude` (so recombination is a
        // no-op), not the empty lists that used to erase them.
        let filters = serde_json::json!({
            "genres": {
                "label": "Genres",
                "type": "XCheckbox",
                "value": {"include": ["Action", "Fantasy"], "exclude": ["Adult"]},
                "options": [
                    {"label": "Action", "value": "Action"},
                    {"label": "Adult", "value": "Adult"},
                    {"label": "Fantasy", "value": "Fantasy"}
                ]
            }
        });

        let (defs, skipped) = settings_from_plugin(&filters, &serde_json::json!({}));
        assert!(skipped.is_empty());
        assert!(defs.is_empty());
    }

    #[test]
    fn settings_from_plugin_mixed_checkbox_and_xcheckbox() {
        let filters = serde_json::json!({
            "status": {
                "label": "Status",
                "type": "Checkbox",
                "value": [],
                "options": [
                    {"label": "Ongoing", "value": "ongoing"},
                    {"label": "Completed", "value": "completed"}
                ]
            },
            "genres": {
                "label": "Genres",
                "type": "XCheckbox",
                "value": {"include": [], "exclude": []},
                "options": [
                    {"label": "Action", "value": "Action"},
                    {"label": "Romance", "value": "Romance"}
                ]
            }
        });
        let plugin_settings = serde_json::json!({});

        let (defs, skipped) = settings_from_plugin(&filters, &plugin_settings);
        assert!(skipped.is_empty());
        assert!(defs.is_empty());
    }
}
