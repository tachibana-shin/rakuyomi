//! Reads a plugin's own declared metadata (`id`/`name`/`site`/`lang`/
//! `version`/`filters`/`pluginSettings`) by executing `Payload/main.js` in a
//! throwaway [`js_runtime::JsRuntime`] and asking the instantiated plugin
//! object for its properties, rather than pattern-matching the (often
//! minified, sometimes constructor-parameterized — see `ranobes.js`'s
//! `new RanobesPlugin({id: "ranobes", sourceSite: ..., options: {lang: ...}})`
//! pattern) source text directly. Reading the live object after construction
//! is what makes this generalize to arbitrary `lnreader-plugins` sources
//! instead of just the handful used for manual validation — see
//! `docs/lnreader/PHASE3_HANDOFF.md` §2.1 non-negotiable (a).
//!
//! Used by the `lnreader_packager` binary (Phase 3 packaging pipeline) via
//! [`crate::source::lnreader_extract_plugin_metadata`]; not called from the
//! runtime itself.

use std::collections::HashMap;

use anyhow::{Context as _, Result};

use super::js_runtime;

/// Raw metadata read off a plugin instance, still as JSON — deliberately
/// *not* translated into `SourceInfo`/`SettingDefinition` here. That mapping
/// (e.g. how a `filters`/`pluginSettings` entry's `type` becomes a
/// `SettingDefinition` variant) is packaging policy, not something the
/// runtime needs to know in order to execute a plugin, so it lives in
/// `lnreader_packager` instead.
///
/// Fields absent on the plugin object (i.e. not every plugin defines
/// `filters`/`pluginSettings`) are `undefined` in JS, and `JSON.stringify`
/// *omits* an object property whose value is `undefined` rather than
/// serializing it as `null` — so the `serde_json::Value` this function
/// returns can be genuinely missing those keys, not merely `Value::Null` at
/// them. `packaging::RawMetadata`'s `#[serde(default)]` fields turn a
/// missing key into `Value::Null` when deserializing this into that struct,
/// but that normalization happens there, not here — any other caller
/// reading this `Value` directly must treat `filters`/`pluginSettings` (and
/// `version`) as optional keys, not assume their presence.
pub fn extract(main_js: &str) -> Result<serde_json::Value> {
    let mut runtime = js_runtime::new(HashMap::new(), main_js)
        .context("failed to load plugin for metadata extraction")?;
    let context = runtime.context();

    // Round-trips through `JSON.stringify`/`serde_json::from_str` rather
    // than walking `JsValue` by hand — same pattern already used for
    // `storage.set()` values (see `js_runtime::register_storage`'s
    // `set_native`), and it gets `filters`/`pluginSettings` (arbitrarily
    // nested objects/arrays) for free instead of needing a recursive
    // `JsValue` -> `serde_json::Value` converter that doesn't exist
    // elsewhere in this codebase.
    let json_value = js_runtime::eval(
        context,
        "JSON.stringify({\n\
             id: __lnreader_plugin.id,\n\
             name: __lnreader_plugin.name,\n\
             site: __lnreader_plugin.site,\n\
             lang: __lnreader_plugin.lang,\n\
             version: __lnreader_plugin.version,\n\
             filters: __lnreader_plugin.filters,\n\
             pluginSettings: __lnreader_plugin.pluginSettings,\n\
         })",
        "metadata probe",
    )?;

    let json_string = json_value
        .to_string(context)
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to stringify metadata: {}",
                js_runtime::describe_js_error(&e, context)
            )
        })?
        .to_std_string_escaped();

    serde_json::from_str(&json_string).context("plugin metadata was not valid JSON")
}
