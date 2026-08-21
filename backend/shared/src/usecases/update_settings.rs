use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use size::{consts, Size};

use crate::settings::{
    deserialize_source_lists, ChapterSortingMode, ChapterTitleFormat, LibrarySortingMode,
    LibraryViewMode, SearchViewMode, Settings, SourceList, StorageSizeLimit,
    TrackingServiceSettings,
};

pub fn update_settings(
    settings: &mut Settings,
    settings_path: &Path,
    settings_to_update: UpdateableSettings,
) -> Result<()> {
    let mut updated_settings = settings.clone();
    settings_to_update.apply_updates(&mut updated_settings);
    updated_settings.save_to_file(settings_path)?;

    *settings = updated_settings;

    Ok(())
}

/// Deserializes an optional `i64` from a JSON number that a Lua client may
/// have encoded as a floating point value: LuaJIT numbers are doubles and
/// the KOReader rapidjson binding only keeps the integer encoding for
/// values inside the 32-bit range, so chat ids beyond it arrive as e.g.
/// `8820500297.0`.
fn deserialize_chat_id<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                return Ok(Some(int));
            }

            let float = number.as_f64().ok_or_else(|| {
                D::Error::custom("cookie_sync_chat_id must be an integer".to_string())
            })?;
            if float.fract() == 0.0 && float >= i64::MIN as f64 && float <= i64::MAX as f64 {
                Ok(Some(float as i64))
            } else {
                Err(D::Error::custom(format!(
                    "cookie_sync_chat_id must be an integer, got {float}"
                )))
            }
        }
        Some(other) => Err(D::Error::custom(format!(
            "cookie_sync_chat_id must be an integer, got {other}"
        ))),
    }
}

#[derive(Serialize, Deserialize)]
pub struct UpdateableSettings {
    chapter_sorting_mode: ChapterSortingMode,
    library_sorting_mode: LibrarySortingMode,
    storage_size_limit_mb: usize,
    storage_path: Option<PathBuf>,
    concurrent_requests_pages: Option<usize>,
    api_sync: Option<String>,
    tracking_auto_sync: bool,
    anilist: TrackingServiceSettings,
    myanimelist: TrackingServiceSettings,
    shikimori: TrackingServiceSettings,
    kavita: TrackingServiceSettings,
    bangumi: TrackingServiceSettings,
    mangabaka: TrackingServiceSettings,
    komga: TrackingServiceSettings,
    suwayomi: TrackingServiceSettings,
    enabled_cron_check_mangas_update: bool,
    source_skip_cron: Option<String>,
    preload_chapters: usize,
    optimize_image: bool,
    library_view_mode: LibraryViewMode,
    search_view_mode: SearchViewMode,
    ram_storage_enabled: bool,
    ram_storage_size_mb: usize,
    cookie_sync_server_url: Option<String>,
    cookie_sync_device_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_chat_id")]
    cookie_sync_chat_id: Option<i64>,
    proxy_url: Option<String>,
    oauth_server_url: String,
    #[serde(default)]
    chapter_title_format: ChapterTitleFormat,
    delete_downloaded_on_remove: bool,
    delete_downloaded_after_read: bool,
    /// The languages selected in the available sources listing, used to
    /// filter which sources are shown. Reuses the settings field that is
    /// also exposed to sources as the `languages` global.
    #[serde(default)]
    languages: Vec<String>,
    /// The source lists configured by the user, in the same format as
    /// `Settings.source_lists` (also accepts the legacy plain-string format).
    #[serde(default, deserialize_with = "deserialize_source_lists")]
    source_lists: Vec<SourceList>,
}

fn clean_opt(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

impl UpdateableSettings {
    pub fn apply_updates(self, settings: &mut Settings) {
        settings.chapter_sorting_mode = self.chapter_sorting_mode;
        settings.library_sorting_mode = self.library_sorting_mode;
        settings.storage_size_limit =
            StorageSizeLimit(Size::from_megabytes(self.storage_size_limit_mb));
        settings.storage_path = self.storage_path;
        settings.concurrent_requests_pages = self.concurrent_requests_pages;
        settings.api_sync = self.api_sync;
        settings.tracking_auto_sync = self.tracking_auto_sync;

        // Tracking services — update credentials but preserve username (set by backend)
        let update_service = |target: &mut TrackingServiceSettings,
                              src: TrackingServiceSettings| {
            target.client_id = clean_opt(src.client_id);
            target.client_secret = clean_opt(src.client_secret);
            target.access_token = clean_opt(src.access_token);
            target.refresh_token = clean_opt(src.refresh_token);
            target.api_key = clean_opt(src.api_key);
            target.url = clean_opt(src.url);
            // username is read-only — not updated from frontend
        };
        update_service(&mut settings.anilist, self.anilist);
        update_service(&mut settings.myanimelist, self.myanimelist);
        update_service(&mut settings.shikimori, self.shikimori);
        update_service(&mut settings.kavita, self.kavita);
        update_service(&mut settings.bangumi, self.bangumi);
        update_service(&mut settings.mangabaka, self.mangabaka);
        update_service(&mut settings.komga, self.komga);
        update_service(&mut settings.suwayomi, self.suwayomi);

        settings.enabled_cron_check_mangas_update = self.enabled_cron_check_mangas_update;
        settings.source_skip_cron = self.source_skip_cron;
        settings.preload_chapters = self.preload_chapters;
        settings.optimize_image = self.optimize_image;
        settings.library_view_mode = self.library_view_mode;
        settings.search_view_mode = self.search_view_mode;
        settings.cookie_sync_server_url = self.cookie_sync_server_url;
        settings.cookie_sync_device_name = self.cookie_sync_device_name;
        settings.cookie_sync_chat_id = self.cookie_sync_chat_id;
        settings.proxy_url = self.proxy_url.and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        settings.oauth_server_url = self.oauth_server_url;
        settings.chapter_title_format = self.chapter_title_format;
        settings.delete_downloaded_on_remove = self.delete_downloaded_on_remove;
        settings.delete_downloaded_after_read = self.delete_downloaded_after_read;
        settings.languages = self.languages;
        settings.source_lists = self.source_lists;
    }
}

impl From<&Settings> for UpdateableSettings {
    fn from(value: &Settings) -> Self {
        Self {
            chapter_sorting_mode: value.chapter_sorting_mode,
            library_sorting_mode: value.library_sorting_mode,
            storage_size_limit_mb: (value.storage_size_limit.0.bytes() / consts::MB)
                .try_into()
                .unwrap(),
            storage_path: value.storage_path.clone(),
            concurrent_requests_pages: value.concurrent_requests_pages,
            api_sync: value.api_sync.clone(),
            tracking_auto_sync: value.tracking_auto_sync,
            anilist: value.anilist.clone(),
            myanimelist: value.myanimelist.clone(),
            shikimori: value.shikimori.clone(),
            kavita: value.kavita.clone(),
            bangumi: value.bangumi.clone(),
            mangabaka: value.mangabaka.clone(),
            komga: value.komga.clone(),
            suwayomi: value.suwayomi.clone(),
            enabled_cron_check_mangas_update: value.enabled_cron_check_mangas_update,
            source_skip_cron: value.source_skip_cron.clone(),
            preload_chapters: value.preload_chapters,
            optimize_image: value.optimize_image,
            library_view_mode: value.library_view_mode,
            search_view_mode: value.search_view_mode,
            ram_storage_enabled: value.ram_storage_enabled,
            ram_storage_size_mb: value.ram_storage_size_mb,
            cookie_sync_server_url: value.cookie_sync_server_url.clone(),
            cookie_sync_device_name: value.cookie_sync_device_name.clone(),
            cookie_sync_chat_id: value.cookie_sync_chat_id,
            proxy_url: value.proxy_url.clone(),
            oauth_server_url: value.oauth_server_url.clone(),
            chapter_title_format: value.chapter_title_format,
            delete_downloaded_on_remove: value.delete_downloaded_on_remove,
            delete_downloaded_after_read: value.delete_downloaded_after_read,
            languages: value.languages.clone(),
            source_lists: value.source_lists.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn sample_settings() -> Settings {
        let json = r#"{
            "source_lists": [
                {"url": "https://a.example.com/index.min.json", "type": "aidoku"},
                {"url": "https://github.com/lnreader/lnreader-plugins", "type": "lnreader"}
            ],
            "languages": ["en"]
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn test_updateable_settings_source_lists_roundtrip() {
        let settings = sample_settings();
        let updateable = UpdateableSettings::from(&settings);
        let serialized = serde_json::to_string(&updateable).unwrap();
        let deserialized: UpdateableSettings = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.source_lists, settings.source_lists);
        assert_eq!(deserialized.languages, settings.languages);
    }

    #[test]
    fn test_updateable_settings_source_lists_default_empty() {
        let updateable = UpdateableSettings::from(&Settings::default());
        assert!(updateable.source_lists.is_empty());
    }

    #[test]
    fn test_update_settings_applies_source_lists() {
        let mut settings = Settings::default();
        let mut source_lists = Vec::new();
        source_lists.push(crate::settings::SourceList {
            url: Url::parse("https://github.com/lnreader/lnreader-plugins").unwrap(),
            source_type: crate::settings::SourceListType::LnReader,
        });
        let updateable = UpdateableSettings {
            source_lists: source_lists.clone(),
            ..UpdateableSettings::from(&settings)
        };
        updateable.apply_updates(&mut settings);

        assert_eq!(settings.source_lists, source_lists);
    }

    #[test]
    fn test_updateable_settings_accepts_float_encoded_chat_id() {
        // The KOReader rapidjson binding encodes integers beyond the 32-bit
        // range as floating point values (LuaJIT numbers are doubles), so a
        // Telegram chat id round-trips as `8820500297.0`.
        let settings = sample_settings();
        let mut value: serde_json::Value =
            serde_json::to_value(UpdateableSettings::from(&settings)).unwrap();
        value["cookie_sync_chat_id"] = serde_json::json!(8820500297.0);

        let deserialized: UpdateableSettings = serde_json::from_value(value).unwrap();
        assert_eq!(deserialized.cookie_sync_chat_id, Some(8820500297));
    }

    #[test]
    fn test_updateable_settings_rejects_fractional_chat_id() {
        let settings = sample_settings();
        let mut value: serde_json::Value =
            serde_json::to_value(UpdateableSettings::from(&settings)).unwrap();
        value["cookie_sync_chat_id"] = serde_json::json!(1.5);

        let result = serde_json::from_value::<UpdateableSettings>(value);
        assert!(result.is_err());
    }
}
