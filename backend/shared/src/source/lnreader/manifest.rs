use anyhow::{Context, Result};
use serde_json::Value;

use crate::source::{
    model::SettingDefinition,
    {SourceConfig, SourceInfo, SourceManifest},
};

/// Parsed `props` of an LNReader plugin.
#[derive(Debug, Clone, Default)]
pub struct PluginProps {
    pub id: String,
    pub name: String,
    pub site: String,
    pub version: String,
    pub icon: Option<String>,
    pub image_request_init: Option<ImageRequestInit>,
    pub has_parse_page: bool,
    pub has_resolve_url: bool,
    pub web_storage_utilized: bool,
    /// The raw `filters` object (search/popular list filters).
    pub filters: Value,
    /// The raw `pluginSettings` object (the plugin's own settings page).
    pub plugin_settings: Value,
}

/// The `imageRequestInit` object of a plugin.
#[derive(Debug, Clone, Default)]
pub struct ImageRequestInit {
    pub method: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

fn as_string(value: &Value, key: &str) -> Result<String> {
    match value.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(other) => anyhow::bail!("field `{}` is not a string: {}", key, other),
        None => anyhow::bail!("missing field `{}` in {}", key, value),
    }
}

fn parse_image_request_init(value: &Value) -> Result<Option<ImageRequestInit>> {
    let Value::Object(map) = value else {
        return Ok(None);
    };
    if map.is_empty() {
        return Ok(None);
    }
    let mut headers = Vec::new();
    if let Some(Value::Object(h)) = value.get("headers") {
        for (k, v) in h {
            if let Value::String(s) = v {
                headers.push((k.clone(), s.clone()));
            }
        }
    }
    Ok(Some(ImageRequestInit {
        method: value
            .get("method")
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
        headers,
        body: value
            .get("body")
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
    }))
}

/// Parses the JSON string returned by the JS `props` method.
pub fn parse_props(json: &str) -> Result<PluginProps> {
    let value: Value = serde_json::from_str(json).context("failed to parse plugin props")?;
    Ok(PluginProps {
        id: as_string(&value, "id")?,
        name: as_string(&value, "name")?,
        site: as_string(&value, "site")?,
        version: as_string(&value, "version")?,
        icon: value
            .get("icon")
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
        image_request_init: parse_image_request_init(
            value.get("imageRequestInit").unwrap_or(&Value::Null),
        )?,
        has_parse_page: value
            .get("hasParsePage")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_resolve_url: value
            .get("hasResolveUrl")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        web_storage_utilized: value
            .get("webStorageUtilized")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        filters: value.get("filters").cloned().unwrap_or(Value::Null),
        plugin_settings: value.get("pluginSettings").cloned().unwrap_or(Value::Null),
    })
}

/// Builds a `SourceManifest` from the plugin props. LNReader plugins have no
/// notion of base URLs, nsfw rating, or min app version.
pub fn manifest_from_props(
    props: &PluginProps,
    source_of_source: Option<String>,
) -> SourceManifest {
    SourceManifest {
        info: SourceInfo {
            id: props.id.clone(),
            lang: None,
            name: props.name.clone(),
            version: props.version.clone(),
            url: Some(props.site.clone()),
            urls: None,
            min_app_version: None,
        },
        config: Some(SourceConfig {
            allows_base_url_select: Some(false),
        }),
        source_of_source,
    }
}

/// Converts the plugin `filters` / `pluginSettings` objects into RakuYomi
/// setting definitions, so the existing settings UI can display them.
///
/// The `from_filters` flag controls whether the plugin's filters (which are
/// also used to filter the popular novels list) or the plugin's own settings
/// are converted. The shapes are identical, so the conversion is shared.
pub fn setting_definitions(value: &Value) -> Result<Vec<SettingDefinition>> {
    let Value::Object(map) = value else {
        return Ok(Vec::new());
    };
    let mut definitions = Vec::new();
    for (key, item) in map {
        definitions.push(definition(key, item)?);
    }
    Ok(definitions)
}

fn definition(key: &str, item: &Value) -> Result<SettingDefinition> {
    let type_ = item.get("type").and_then(Value::as_str).unwrap_or("Text");
    let label = item.get("label").and_then(Value::as_str).unwrap_or(key);

    match type_ {
        "Group" => {
            let items = item
                .get("items")
                .and_then(Value::as_object)
                .map(|items| {
                    items
                        .iter()
                        .map(|(k, v)| definition(k, v))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            Ok(SettingDefinition::Group {
                title: Some(label.to_string()),
                items,
                footer: None,
            })
        }
        "Text" | "TextInput" => Ok(SettingDefinition::Text {
            placeholder: None,
            key: key.to_string(),
            default: item
                .get("value")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
        }),
        "Picker" => {
            let options = option_values(item);
            Ok(SettingDefinition::Select {
                title: label.to_string(),
                key: key.to_string(),
                default: item
                    .get("value")
                    .and_then(Value::as_u64)
                    .and_then(|i| options.get(i as usize).cloned()),
                values: options,
                titles: None,
            })
        }
        "Switch" => Ok(SettingDefinition::Switch {
            title: label.to_string(),
            key: key.to_string(),
            default: item.get("value").and_then(Value::as_bool).unwrap_or(false),
        }),
        "Checkbox" | "CheckboxGroup" | "XCheckbox" | "ExcludableCheckboxGroup" => {
            Ok(SettingDefinition::MultiSelect {
                title: label.to_string(),
                key: key.to_string(),
                default: item
                    .get("value")
                    .and_then(Value::as_array)
                    .map(|v| {
                        v.iter()
                            .filter_map(Value::as_str)
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                values: option_values(item),
                titles: None,
            })
        }
        other => anyhow::bail!("unsupported LNReader filter type `{}`", other),
    }
}

/// The selectable values of a checkbox/picker filter. Prefers `options[].value`,
/// falls back to `keys` / `values` arrays.
fn option_values(item: &Value) -> Vec<String> {
    if let Some(options) = item.get("options").and_then(Value::as_array) {
        if !options.is_empty() {
            return options
                .iter()
                .map(|o| {
                    o.get("value")
                        .or_else(|| o.get("label"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                })
                .collect();
        }
    }
    if let Some(keys) = item.get("keys").and_then(Value::as_array) {
        return keys
            .iter()
            .filter_map(Value::as_str)
            .map(|s| s.to_string())
            .collect();
    }
    if let Some(values) = item.get("values").and_then(Value::as_array) {
        return values
            .iter()
            .filter_map(Value::as_str)
            .map(|s| s.to_string())
            .collect();
    }
    Vec::new()
}
