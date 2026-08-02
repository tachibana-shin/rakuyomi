//! Maps a plugin's `filters`/`pluginSettings` (both a
//! `Record<string, FilterInput>` — confirmed against a real plugin,
//! `freewebnovel.js`'s `filters: {type: {type:"Picker", label:"Novel Type",
//! value:"sort/most-popular", options:[{label,value},...]}, genres: {...}}`
//! — the only one of the 5 vendored manual-validation fixtures that defines
//! any) into Rakuyomi's `SettingDefinition` list (`Payload/settings.json`).
//!
//! `FilterTypes` values, per the shim (`sdk_lnreader/js_runtime.rs`'s
//! `@libs/filterInputs` polyfill) that resolves them at plugin-load time:
//! `TextInput -> "Text"`, `Picker -> "Picker"`, `CheckboxGroup -> "Checkbox"`,
//! `Switch -> "Switch"`, `ExcludableCheckboxGroup -> "XCheckbox"`.
//!
//! Documented simplifying assumption (only one real sample to go on, per
//! `PHASE3_HANDOFF.md`'s non-negotiable (a)): `pluginSettings` is assumed to
//! share the same per-entry shape as `filters` (both are typed through the
//! same `@libs/filterInputs` helper in `lnreader-plugins` itself), even
//! though none of the 5 vendored fixtures happen to set `pluginSettings` to
//! confirm it directly.

use serde_json::{Map, Value};
use shared::source::model::SettingDefinition;

/// Merges `filters` and `pluginSettings` into one flat list — Rakuyomi has
/// no separate "search filters" screen (see `faisabilite-v2...md` §10.1: no
/// browse/popular UI at all), so both just become source settings.
/// Unrecognized filter `type`s are skipped (not guessed at) and reported by
/// the caller.
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
        // "exclude" half of `ExcludableCheckboxGroup`, and adding one is
        // explicitly out of scope here (Phase 4 UI work, and only as a last
        // resort per the project's non-negotiable on new Lua widgets). This
        // is a deliberately lossy mapping, not a bug: an excludable filter
        // still works as a plain "include" multi-select, it just can't
        // express exclusion until/unless that widget is ever added.
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

fn string_options(obj: &Map<String, Value>) -> Option<Vec<(String, String)>> {
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
