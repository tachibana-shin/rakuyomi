use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use size::{consts, Size};

use crate::settings::{
    ChapterSortingMode, ChapterTitleFormat, LibrarySortingMode, LibraryViewMode, SearchViewMode,
    Settings, StorageSizeLimit,
};

pub fn update_settings(
    settings: &mut Settings,
    settings_path: &Path,
    settings_to_update: UpdateableSettings,
) -> Result<()> {
    // Clone the settings and save the cloned one first, so that we only change the application settings
    // iff everything goes well
    let mut updated_settings = settings.clone();
    settings_to_update.apply_updates(&mut updated_settings);
    updated_settings.save_to_file(settings_path)?;

    *settings = updated_settings;

    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[derive(Serialize, Deserialize)]
pub struct UpdateableSettings {
    chapter_sorting_mode: ChapterSortingMode,
    library_sorting_mode: LibrarySortingMode,
    storage_size_limit_mb: usize,
    storage_path: Option<PathBuf>,
    concurrent_requests_pages: Option<usize>,
    api_sync: Option<String>,
    enabled_cron_check_mangas_update: bool,
    source_skip_cron: Option<String>,
    preload_chapters: usize,
    optimize_image: bool,
    library_view_mode: LibraryViewMode,
    search_view_mode: SearchViewMode,
    ram_storage_enabled: bool, // readonly not allow UpdateableSettings update value
    ram_storage_size_mb: usize, // readonly not allow UpdateableSettings update value
    cookie_sync_server_url: Option<String>,
    cookie_sync_device_name: Option<String>,
    cookie_sync_chat_id: Option<i64>,
    proxy_url: Option<String>,
    #[serde(default)]
    chapter_title_format: ChapterTitleFormat,
    #[serde(default = "default_true")]
    delete_downloaded_on_remove: bool,
    #[serde(default = "default_false")]
    delete_downloaded_after_read: bool,
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
        settings.chapter_title_format = self.chapter_title_format;
        settings.delete_downloaded_on_remove = self.delete_downloaded_on_remove;
        settings.delete_downloaded_after_read = self.delete_downloaded_after_read;
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
            chapter_title_format: value.chapter_title_format,
            delete_downloaded_on_remove: value.delete_downloaded_on_remove,
            delete_downloaded_after_read: value.delete_downloaded_after_read,
        }
    }
}
