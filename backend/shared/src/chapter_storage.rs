use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::debug;
use sha2::{Digest, Sha256};
use size::Size;
use tempfile::NamedTempFile;
use walkdir::{DirEntry, WalkDir};

use crate::model::ChapterId;

const CHAPTER_FILE_EXTENSION: [&str; 2] = ["cbz", "epub"];

#[derive(Clone)]
pub struct ChapterStorage {
    downloads_folder_path: PathBuf,
    storage_size_limit: Size,
}

impl ChapterStorage {
    pub fn new(downloads_folder_path: PathBuf, storage_size_limit: Size) -> Result<Self> {
        fs::create_dir_all(&downloads_folder_path)
            .with_context(|| "while trying to ensure chapter storage exists")?;

        Ok(Self {
            downloads_folder_path,
            storage_size_limit,
        })
    }

    pub fn get_stored_chapter(&self, id: &ChapterId) -> Option<PathBuf> {
        let new_path = self.path_for_chapter(id, false);
        if new_path.exists() {
            return Some(new_path);
        }

        let new_path_novel = self.path_for_chapter(id, true);
        if new_path_novel.exists() {
            return Some(new_path);
        }

        // Backwards compatibility: check the old path format
        let old_path = self.path_for_chapter_legacy(id, false);
        if old_path.exists() {
            return Some(old_path);
        }

        let old_path_novel = self.path_for_chapter_legacy(id, true);
        if old_path_novel.exists() {
            return Some(old_path_novel);
        } else {
            return None;
        }
    }

    pub fn get_path_to_store_chapter(&self, id: &ChapterId, is_novel: bool) -> PathBuf {
        // New chapters should always use the new path format
        self.path_for_chapter(id, is_novel)
    }

    // FIXME depending on `NamedTempFile` here is pretty ugly
    pub fn persist_chapter(
        &self,
        id: &ChapterId,
        is_novel: bool,
        temporary_file: NamedTempFile,
    ) -> Result<PathBuf> {
        let mut current_size = self.calculate_storage_size();
        let persisted_chapter_size = Size::from_bytes(temporary_file.as_file().metadata()?.size());

        while current_size + persisted_chapter_size > self.storage_size_limit {
            debug!(
                "persist_chapter: current storage is {current_size}/{}, new persisted chapter is \
                {persisted_chapter_size}, attempting to evict",
                self.storage_size_limit
            );

            self.evict_least_recently_modified_chapter()
                .with_context(|| format!(
                    "while attempting to bring the storage size under the {} limit (current size: {}, persisted chapter size: {})",
                    self.storage_size_limit,
                    current_size,
                    persisted_chapter_size,
                ))?;

            current_size = self.calculate_storage_size();
        }

        // Persist using the new path format
        let path = self.path_for_chapter(id, is_novel);
        temporary_file.persist(&path)?;

        Ok(path)
    }

    pub fn set_downloads_folder_path(&mut self, path: PathBuf) -> Result<()> {
        fs::create_dir_all(&path)
            .with_context(|| "while trying to ensure chapter storage exists")?;

        self.downloads_folder_path = path;

        Ok(())
    }

    fn calculate_storage_size(&self) -> Size {
        let size_in_bytes: u64 = self
            .chapter_files_iterator()
            .filter_map(|entry| entry.metadata().ok().map(|metadata| metadata.size()))
            .sum();

        Size::from_bytes(size_in_bytes)
    }

    fn evict_least_recently_modified_chapter(&self) -> Result<()> {
        let chapter_to_evict = self
            .find_least_recently_modified_chapter()?
            .ok_or_else(|| anyhow!("couldn't find any chapters to evict from storage"))?;

        debug!(
            "evict_least_recently_modified_chapter: evicting {}",
            chapter_to_evict.display()
        );

        fs::remove_file(chapter_to_evict)?;

        Ok(())
    }

    fn find_least_recently_modified_chapter(&self) -> Result<Option<PathBuf>> {
        let chapter_path = self
            .chapter_files_iterator()
            .filter_map(|entry| {
                let path = entry.path().to_owned();
                let modified = entry.metadata().ok()?.modified().ok()?;

                Some((path, modified))
            })
            // FIXME i dont think we need to clone here
            .min_by_key(|(_, modified)| *modified)
            .map(|(path, _)| path.to_owned());

        Ok(chapter_path)
    }

    fn chapter_files_iterator(&self) -> impl Iterator<Item = DirEntry> {
        WalkDir::new(&self.downloads_folder_path)
            .into_iter()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let extension = entry.path().extension()?;
                let metadata = entry.metadata().ok()?;

                if !metadata.is_file() || !matches!(extension.to_str(), Some(ext) if CHAPTER_FILE_EXTENSION.contains(&ext))
                {
                    return None;
                }

                Some(entry)

            })
    }

    // DEPRECATED: This function provides backwards compatibility for the old chapter path format.
    // We should remove it after some versions (enough time for users to have already migrated :eyes:)
    fn path_for_chapter_legacy(&self, chapter_id: &ChapterId, is_novel: bool) -> PathBuf {
        let output_filename = sanitize_filename::sanitize(format!(
            "{}-{}.{}",
            chapter_id.source_id().value(),
            chapter_id.value(),
            if is_novel { "epub" } else { "cbz" }
        ));

        self.downloads_folder_path.join(output_filename)
    }

    fn path_for_chapter(&self, chapter_id: &ChapterId, is_novel: bool) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(chapter_id.source_id().value().as_bytes());
        hasher.update(chapter_id.manga_id().value().as_bytes());
        hasher.update(chapter_id.value().as_bytes());
        let hash_result = hasher.finalize();

        // Use URL-safe base64 encoding without padding for the filename
        let encoded_hash = general_purpose::URL_SAFE_NO_PAD.encode(hash_result);

        let output_filename = format!("{}.{}", encoded_hash, if is_novel { "epub" } else { "cbz" });

        self.downloads_folder_path.join(output_filename)
    }
}
