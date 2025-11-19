use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use size::{consts, Size};

use crate::settings::{ChapterSortingMode, LibrarySortingMode, Settings, StorageSizeLimit};

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
        }
    }
}
