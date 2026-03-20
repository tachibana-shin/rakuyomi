use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use size::{consts, Size};

use crate::settings::{
    ChapterSortingMode, LibrarySortingMode, LibraryViewMode, Settings, StorageSizeLimit,
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

#[derive(Serialize, Deserialize)]
pub struct UpdateableSettings {
    chapter_sorting_mode: ChapterSortingMode,
    library_sorting_mode: LibrarySortingMode,
    storage_size_limit_mb: usize,
    storage_path: Option<PathBuf>,
    concurrent_requests_pages: Option<usize>,
    api_sync: Option<String>,
    tracking_auto_sync: bool,
    anilist_access_token: Option<String>,
    mal_client_id: Option<String>,
    mal_client_secret: Option<String>,
    mal_access_token: Option<String>,
    mal_refresh_token: Option<String>,
    anilist_refresh_token: Option<String>,
    shikimori_client_id: Option<String>,
    shikimori_client_secret: Option<String>,
    shikimori_access_token: Option<String>,
    shikimori_refresh_token: Option<String>,
    kavita_url: Option<String>,
    kavita_api_key: Option<String>,
    enabled_cron_check_mangas_update: bool,
    source_skip_cron: Option<String>,
    preload_chapters: usize,
    optimize_image: bool,
    library_view_mode: LibraryViewMode,
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
        settings.anilist_access_token = self.anilist_access_token.filter(|v| !v.trim().is_empty());
        settings.mal_client_id = self.mal_client_id.filter(|v| !v.trim().is_empty());
        settings.mal_client_secret = self.mal_client_secret.filter(|v| !v.trim().is_empty());
        settings.mal_access_token = self.mal_access_token.filter(|v| !v.trim().is_empty());
        settings.mal_refresh_token = self.mal_refresh_token.filter(|v| !v.trim().is_empty());
        settings.anilist_refresh_token = self.anilist_refresh_token.filter(|v| !v.trim().is_empty());
        settings.shikimori_client_id = self.shikimori_client_id.filter(|v| !v.trim().is_empty());
        settings.shikimori_client_secret = self.shikimori_client_secret.filter(|v| !v.trim().is_empty());
        settings.shikimori_access_token = self.shikimori_access_token.filter(|v| !v.trim().is_empty());
        settings.shikimori_refresh_token = self.shikimori_refresh_token.filter(|v| !v.trim().is_empty());
        settings.kavita_url = self.kavita_url.filter(|v| !v.trim().is_empty());
        settings.kavita_api_key = self.kavita_api_key.filter(|v| !v.trim().is_empty());
        settings.enabled_cron_check_mangas_update = self.enabled_cron_check_mangas_update;
        settings.source_skip_cron = self.source_skip_cron;
        settings.preload_chapters = self.preload_chapters;
        settings.optimize_image = self.optimize_image;
        settings.library_view_mode = self.library_view_mode;
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
            anilist_access_token: value.anilist_access_token.clone(),
            mal_client_id: value.mal_client_id.clone(),
            mal_client_secret: value.mal_client_secret.clone(),
            mal_access_token: value.mal_access_token.clone(),
            mal_refresh_token: value.mal_refresh_token.clone(),
            anilist_refresh_token: value.anilist_refresh_token.clone(),
            shikimori_client_id: value.shikimori_client_id.clone(),
            shikimori_client_secret: value.shikimori_client_secret.clone(),
            shikimori_access_token: value.shikimori_access_token.clone(),
            shikimori_refresh_token: value.shikimori_refresh_token.clone(),
            kavita_url: value.kavita_url.clone(),
            kavita_api_key: value.kavita_api_key.clone(),
            enabled_cron_check_mangas_update: value.enabled_cron_check_mangas_update,
            source_skip_cron: value.source_skip_cron.clone(),
            preload_chapters: value.preload_chapters,
            optimize_image: value.optimize_image,
            library_view_mode: value.library_view_mode,
        }
    }
}
