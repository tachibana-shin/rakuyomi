use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use regex::Regex;
use schemars::JsonSchema;
use serde::{
    de::{Unexpected, Visitor},
    Deserialize, Serialize,
};
use size::{Base, Size};
use url::Url;

#[derive(Clone, Debug, PartialEq)]
pub struct StorageSizeLimit(pub Size);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SourceSettingValue {
    Data(Vec<u8>),
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Vec(Vec<String>),
    Null,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChapterSortingMode {
    ChapterAscending,
    #[default]
    ChapterDescending,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySortingMode {
    #[default]
    Ascending,
    Descending,
    TitleAsc,
    TitleDesc,
    UnreadAsc,
    UnreadDesc,
    LastReadAsc,
    LastReadDesc,
    SourceAsc,
    SourceDesc,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibraryViewMode {
    Base,
    #[default]
    Cover,
    Grid,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchViewMode {
    #[default]
    Base,
    Cover,
    Grid,
}

/// Settings used to configure rakuyomi's behavior.
#[derive(Serialize, Deserialize, Default, Clone, Debug, JsonSchema)]
pub struct Settings {
    /// A list of URLs containing Aidoku-compatible source lists, which will be available
    /// for installation from inside the plugin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_lists: Vec<Url>,

    /// If set, only chapters translated to those languages will be shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,

    /// The size of the storage used to store download chapters. Defaults to 2 GB.
    /// Should be in the format: [positive real number] [GB|MB].
    #[serde(
        default = "default_storage_size_limit",
        skip_serializing_if = "is_default_storage_size_limit"
    )]
    pub storage_size_limit: StorageSizeLimit,

    /// The path to the folder where downloaded chapters will be stored.
    /// If not set, the default downloads folder will be used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<PathBuf>,

    /// Source-specific settings.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub source_settings: HashMap<String, HashMap<String, SourceSettingValue>>,

    /// The order in which chapters will be displayed in the chapter listing. Defaults to
    /// `volume_descending`.
    #[serde(default)]
    pub chapter_sorting_mode: ChapterSortingMode,

    #[serde(default)]
    pub library_sorting_mode: LibrarySortingMode,

    #[serde(default)]
    pub concurrent_requests_pages: Option<usize>,

    #[serde(default)]
    pub api_sync: Option<String>,

    #[serde(default = "default_false")]
    pub enabled_cron_check_mangas_update: bool,

    #[serde(default)]
    pub source_skip_cron: Option<String>,

    #[serde(default)]
    pub preload_chapters: usize,

    #[serde(default)]
    pub optimize_image: bool,

    #[serde(default)]
    pub library_view_mode: LibraryViewMode,

    #[serde(default)]
    pub search_view_mode: SearchViewMode,

    /// When enabled, downloaded chapters are stored in a RAM-backed tmpfs.
    /// Data is lost on power off.
    #[serde(default)]
    pub ram_storage_enabled: bool,

    /// Size of the RAM storage in MB.
    #[serde(default = "default_ram_storage_size_mb")]
    pub ram_storage_size_mb: usize,

    /// Cookie sync server URL (Telegram Bot Deno server).
    #[serde(default)]
    pub cookie_sync_server_url: Option<String>,

    /// Device name after pairing with the cookie sync bot.
    #[serde(default)]
    pub cookie_sync_device_name: Option<String>,

    /// Telegram chat_id after pairing.
    #[serde(default)]
    pub cookie_sync_chat_id: Option<i64>,

    /// API token for authenticating with the cookie sync bot.
    #[serde(default)]
    pub cookie_sync_api_token: Option<String>,

    /// An optional HTTP/HTTPS/SOCKS5 proxy URL used for all outgoing requests.
    /// Examples: `http://proxy.local:8080`, `socks5://127.0.0.1:1080`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
}

fn default_ram_storage_size_mb() -> usize {
    32
}

fn default_storage_size_limit() -> StorageSizeLimit {
    StorageSizeLimit(Size::from_megabytes(2000))
}

fn is_default_storage_size_limit(size: &StorageSizeLimit) -> bool {
    *size == default_storage_size_limit()
}

fn default_false() -> bool {
    false
}

impl Default for StorageSizeLimit {
    fn default() -> Self {
        Self(Size::from_bytes(0))
    }
}

impl Serialize for StorageSizeLimit {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.format().with_base(Base::Base10).to_string())
    }
}

const STORAGE_SIZE_LIMIT_REGEX: &str = r"(?<value>[\d.]+) *(?<dimension>GB|MB)";

impl<'de> Deserialize<'de> for StorageSizeLimit {
    fn deserialize<D>(deserializer: D) -> std::prelude::v1::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as SerdeDeserialziationError;

        struct StorageSizeLimitVisitor;

        impl<'de> Visitor<'de> for StorageSizeLimitVisitor {
            type Value = StorageSizeLimit;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid size with dimensions (e.g. 2 GB, 2048 MB)")
            }

            fn visit_str<E>(self, v: &str) -> std::prelude::v1::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                // FIXME this might be supported by `Size` eventually, but for now we just use a regex
                let regex = Regex::new(STORAGE_SIZE_LIMIT_REGEX).unwrap();
                let capture = regex.captures(v).ok_or_else(|| {
                    SerdeDeserialziationError::invalid_value(
                        Unexpected::Str(v),
                        &"a valid size with dimensions (e.g. 2 GB, 2048 MB)",
                    )
                })?;

                let value: f64 = capture["value"].parse().map_err(|_| {
                    SerdeDeserialziationError::invalid_value(
                        Unexpected::Str(v),
                        &"a valid float value as the size",
                    )
                })?;
                let dimension = &capture["dimension"];

                let size = match dimension {
                    "GB" => Size::from_gigabytes(value),
                    "MB" => Size::from_megabytes(value),
                    _ => {
                        return Err(SerdeDeserialziationError::custom(format!(
                            "unexpected dimension: {dimension}"
                        )))
                    }
                };

                Ok(StorageSizeLimit(size))
            }
        }

        deserializer.deserialize_str(StorageSizeLimitVisitor {})
    }
}

impl JsonSchema for StorageSizeLimit {
    fn schema_name() -> Cow<'static, str> {
        "StorageSizeLimit".to_owned().into()
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed(concat!(module_path!(), "::StorageSizeLimit"))
    }

    fn json_schema(gen: &mut schemars::generate::SchemaGenerator) -> schemars::Schema {
        let mut binding = gen.subschema_for::<String>();
        if let Some(schema_object) = binding.as_object_mut() {
            schema_object.insert(
                "pattern".to_owned(),
                Some(STORAGE_SIZE_LIMIT_REGEX.to_owned()).into(),
            );
        }

        binding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_size_limit_deserialize_gb() {
        let json = r#""2 GB""#;
        let size: StorageSizeLimit = serde_json::from_str(json).unwrap();
        assert_eq!(size.0, Size::from_gigabytes(2.0));
    }

    #[test]
    fn test_storage_size_limit_deserialize_mb() {
        let json = r#""2048 MB""#;
        let size: StorageSizeLimit = serde_json::from_str(json).unwrap();
        assert_eq!(size.0, Size::from_megabytes(2048.0));
    }

    #[test]
    fn test_storage_size_limit_deserialize_float() {
        let json = r#""1.5 GB""#;
        let size: StorageSizeLimit = serde_json::from_str(json).unwrap();
        assert_eq!(size.0, Size::from_gigabytes(1.5));
    }

    #[test]
    fn test_storage_size_limit_deserialize_no_space() {
        let json = r#""500MB""#;
        let size: StorageSizeLimit = serde_json::from_str(json).unwrap();
        assert_eq!(size.0, Size::from_megabytes(500.0));
    }

    #[test]
    fn test_storage_size_limit_deserialize_invalid_format() {
        let json = r#""500""#;
        let result: Result<StorageSizeLimit, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_storage_size_limit_deserialize_invalid_dimension() {
        let json = r#""500 KB""#;
        let result: Result<StorageSizeLimit, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_storage_size_limit_serialize() {
        let size = StorageSizeLimit(Size::from_gigabytes(2.0));
        let json = serde_json::to_string(&size).unwrap();
        assert!(json.contains("2"));
        assert!(json.contains("GB"));
    }

    #[test]
    fn test_storage_size_limit_default() {
        let size = StorageSizeLimit::default();
        assert_eq!(size.0, Size::from_bytes(0));
    }

    #[test]
    fn test_chapter_sorting_mode_default() {
        let mode = ChapterSortingMode::default();
        assert_eq!(mode, ChapterSortingMode::ChapterDescending);
    }

    #[test]
    fn test_library_sorting_mode_default() {
        let mode = LibrarySortingMode::default();
        assert_eq!(mode, LibrarySortingMode::Ascending);
    }

    #[test]
    fn test_library_view_mode_default() {
        let mode = LibraryViewMode::default();
        assert_eq!(mode, LibraryViewMode::Cover);
    }

    #[test]
    fn test_search_view_mode_default() {
        let mode = SearchViewMode::default();
        assert_eq!(mode, SearchViewMode::Base);
    }

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert!(settings.source_lists.is_empty());
        assert!(settings.languages.is_empty());
        // Default derive gives StorageSizeLimit(0 bytes)
        assert_eq!(settings.storage_size_limit.0, Size::from_bytes(0));
        assert!(settings.storage_path.is_none());
        assert!(settings.source_settings.is_empty());
        assert_eq!(
            settings.chapter_sorting_mode,
            ChapterSortingMode::ChapterDescending
        );
        assert_eq!(settings.library_sorting_mode, LibrarySortingMode::Ascending);
        assert!(!settings.enabled_cron_check_mangas_update);
        assert_eq!(settings.preload_chapters, 0);
        assert!(!settings.optimize_image);
        assert_eq!(settings.library_view_mode, LibraryViewMode::Cover);
        assert_eq!(settings.search_view_mode, SearchViewMode::Base);
        assert!(!settings.ram_storage_enabled);
        assert_eq!(settings.ram_storage_size_mb, 0);
    }

    #[test]
    fn test_settings_deserialize_uses_serde_defaults() {
        let json = r#"{}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        // serde(default = ...) uses default_storage_size_limit()
        assert_eq!(
            settings.storage_size_limit,
            default_storage_size_limit()
        );
        assert_eq!(settings.ram_storage_size_mb, 32);
    }

    #[test]
    fn test_settings_serialize_roundtrip() {
        let json = r#"{}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        let serialized = serde_json::to_string(&settings).unwrap();
        let deserialized: Settings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.chapter_sorting_mode, settings.chapter_sorting_mode);
        assert_eq!(deserialized.library_sorting_mode, settings.library_sorting_mode);
        assert_eq!(deserialized.library_view_mode, settings.library_view_mode);
        assert_eq!(deserialized.search_view_mode, settings.search_view_mode);
        assert_eq!(deserialized.ram_storage_size_mb, settings.ram_storage_size_mb);
    }
}
