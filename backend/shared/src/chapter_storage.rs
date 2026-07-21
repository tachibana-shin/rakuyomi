use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::{fs, future::Future};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use image::ImageReader;
use log::debug;
use sha2::{Digest, Sha256};
use size::Size;
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;
use walkdir::{DirEntry, WalkDir};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Request;

use crate::model::{ChapterId, MangaId};
use crate::source::decode_image::{decode_argb_to_rgb, decode_image_fast};

const CHAPTER_FILE_EXTENSION: [&str; 2] = ["cbz", "epub"];

pub struct ChapterStorage {
    /// Always the persistent download path — never changes.
    downloads_folder_path: PathBuf,
    ram_enabled: bool,
    tmpfs_mount_error: Option<String>,
    storage_size_limit: Size,
    cached_storage_size: Arc<AtomicU64>,
}

impl Clone for ChapterStorage {
    fn clone(&self) -> Self {
        Self {
            downloads_folder_path: self.downloads_folder_path.clone(),
            ram_enabled: self.ram_enabled,
            tmpfs_mount_error: self.tmpfs_mount_error.clone(),
            storage_size_limit: self.storage_size_limit,
            cached_storage_size: Arc::clone(&self.cached_storage_size),
        }
    }
}

impl ChapterStorage {
    pub fn new(
        downloads_folder_path: PathBuf,
        storage_size_limit: Size,
        ram_enabled: bool,
    ) -> Result<Self> {
        fs::create_dir_all(&downloads_folder_path)
            .with_context(|| "while trying to ensure chapter storage exists")?;

        let storage = Self {
            downloads_folder_path,
            ram_enabled,
            tmpfs_mount_error: None,
            storage_size_limit,
            cached_storage_size: Arc::new(AtomicU64::new(0)),
        };
        storage.cached_storage_size.store(
            storage.calculate_storage_size().bytes() as u64,
            Ordering::Relaxed,
        );

        Ok(storage)
    }

    /// Switch to RAM-backed tmpfs storage.
    /// tmpfs is mounted at `<parent_of_downloads>/tmpfs/`.
    /// If already mounted, remounts with the new size.
    /// Returns an error (e.g. `EPERM` — need root) on failure.
    #[cfg(target_os = "linux")]
    pub fn enable_ram(&mut self, size_mb: usize) -> Result<()> {
        use log::{info, warn};
        use nix::errno::Errno;
        use nix::mount::{mount, MsFlags};

        let ram_path = self.tmpfs_path();
        fs::create_dir_all(&ram_path).with_context(|| {
            format!(
                "failed to create tmpfs mount point at {}",
                ram_path.display()
            )
        })?;

        let mount_opts = format!("size={size_mb}M");

        let result = match mount(
            Some("tmpfs"),
            &ram_path,
            Some("tmpfs"),
            MsFlags::empty(),
            Some(mount_opts.as_str()),
        ) {
            Ok(_) => {
                info!("mounted tmpfs at {}", ram_path.display());
                self.tmpfs_mount_error = None;
                self.ram_enabled = true;
                Ok(())
            }

            Err(Errno::EBUSY) => {
                info!("tmpfs already mounted, attempting remount instead...");
                match mount(
                    Some("tmpfs"),
                    &ram_path,
                    Some("tmpfs"),
                    MsFlags::MS_REMOUNT,
                    Some(mount_opts.as_str()),
                ) {
                    Ok(_) => {
                        info!("remounted tmpfs at {}", ram_path.display());
                        self.tmpfs_mount_error = None;
                        self.ram_enabled = true;
                        Ok(())
                    }
                    Err(e) => {
                        let msg = format!("remount failed: {e}");
                        warn!("failed to remount tmpfs at {}: {e}", ram_path.display());
                        self.tmpfs_mount_error = Some(msg.clone());
                        self.ram_enabled = false;

                        Err(anyhow::anyhow!(msg))
                    }
                }
            }

            Err(e) => {
                let msg = format!("mount failed: {e}");
                warn!("failed to mount tmpfs at {}: {e}", ram_path.display());
                self.tmpfs_mount_error = Some(msg.clone());
                self.ram_enabled = false;

                Err(anyhow::anyhow!(msg))
            }
        };

        result
    }

    #[cfg(target_os = "android")]
    pub fn enable_ram(&mut self, _size_mb: usize) -> Result<()> {
        Err(anyhow::anyhow!("Not implemented for Android"))
    }

    #[cfg(all(not(target_os = "linux"), not(target_os = "android")))]
    pub fn enable_ram(&mut self, _size_mb: usize) -> Result<()> {
        self.ram_enabled = false;
        Err(anyhow::anyhow!("RAM-backed storage is only supported on Linux"))
    }

    /// Switch back to persistent disk storage.
    /// Unmounts the tmpfs if it was mounted.
    #[cfg(target_os = "linux")]
    pub fn disable_ram(&mut self) {
        use log::{info, warn};
        use nix::mount::umount;
        if !self.ram_enabled {
            return;
        }
        let ram_path = self.tmpfs_path();
        if let Err(e) = umount(&ram_path) {
            warn!("failed to unmount tmpfs at {}: {e}", ram_path.display());
        } else {
            info!("unmounted tmpfs at {}", ram_path.display());
        }
        let _ = fs::remove_dir(&ram_path);
        self.ram_enabled = false;
        self.tmpfs_mount_error = None;
    }

    #[cfg(not(target_os = "linux"))]
    pub fn disable_ram(&mut self) {}

    /// Returns `true` when writing to tmpfs instead of persistent storage.
    pub fn is_ram_enabled(&self) -> bool {
        self.ram_enabled
    }

    /// Returns the last tmpfs mount error, if any.
    pub fn tmpfs_mount_error(&self) -> Option<&str> {
        self.tmpfs_mount_error.as_deref()
    }

    pub async fn clean_tmpfs(&self) -> Result<()> {
        if !self.ram_enabled {
            return Ok(());
        }

        let ram_path = self.tmpfs_path();
        let mut entries = tokio::fs::read_dir(&ram_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;

            if file_type.is_dir() {
                tokio::fs::remove_dir_all(&path).await?;
            } else {
                tokio::fs::remove_file(&path).await?;
            }
        }

        Ok(())
    }

    pub async fn evict_tmpfs_older_than_current(
        &self,
        current_chapter_id: &ChapterId,
        is_novel: bool,
    ) -> Result<u64> {
        if !self.ram_enabled {
            return Ok(0);
        }
        let current_path = self.path_for_chapter(current_chapter_id, is_novel, true);
        if !current_path.exists() {
            return Ok(0);
        }
        let current_mtime = tokio::fs::metadata(&current_path).await?.modified()?;

        let mut freed = 0u64;
        for entry in WalkDir::new(self.tmpfs_path())
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path == current_path {
                continue;
            }
            if let Ok(meta) = tokio::fs::metadata(path).await {
                if let Ok(mtime) = meta.modified() {
                    if mtime < current_mtime {
                        let size = meta.len();
                        let _ = tokio::fs::remove_file(path).await;
                        freed += size;
                    }
                }
            }
        }
        Ok(freed)
    }

    pub async fn tmpfs_full_storage(&self) -> Result<bool> {
        // if tmpfs free < 4kb return true
        if !self.ram_enabled {
            return Ok(false);
        }

        #[cfg(not(target_os = "android"))]
        match nix::sys::statfs::statfs(&self.tmpfs_path()) {
            Ok(stats) => Ok((stats.blocks_available() * (stats.block_size() as u64)) < 4096),
            Err(_) => Ok(false),
        }

        #[cfg(target_os = "android")]
        Ok(false)
    }

    /// tmpfs mount path — sibling of `downloads_folder_path`.
    pub fn tmpfs_path(&self) -> PathBuf {
        self.downloads_folder_path
            .parent()
            .unwrap_or(&self.downloads_folder_path)
            .join("tmpfs")
    }

    /// Persistent downloads storage path.
    pub fn downloads_path(&self) -> &PathBuf {
        &self.downloads_folder_path
    }

    pub fn collect_all_files(&self, depth: usize) -> std::collections::HashSet<PathBuf> {
        WalkDir::new(&self.downloads_folder_path)
            .max_depth(depth)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .filter(|entry| {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    matches!(ext.to_lowercase().as_str(), "cbz" | "epub")
                } else {
                    false
                }
            })
            .map(|entry| entry.path().to_path_buf())
            .collect()
    }

    pub async fn delete_filename(&self, filename: String, tmpfs: bool) -> std::io::Result<()> {
        let parent = if tmpfs {
            if self.ram_enabled {
                self.tmpfs_path()
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "tmpfs is not enabled",
                ));
            }
        } else {
            self.downloads_folder_path.clone()
        };

        let file_path = parent.join(&filename);
        let canonical = tokio::fs::canonicalize(&file_path).await?;
        let base = tokio::fs::canonicalize(&parent).await?;
        if !canonical.starts_with(&base) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "path traversal denied",
            ));
        }
        // Get metadata before removal
        let file_size = if tmpfs {
            None
        } else {
            std::fs::metadata(&file_path).ok().map(|m| m.len())
        };

        // Perform the deletion
        tokio::fs::remove_file(file_path).await?;

        // Update cache only after successful removal
        if let Some(size) = file_size {
            self.cached_storage_size.fetch_sub(size, Ordering::Relaxed);
        }

        Ok(())
    }

    fn path_for_poster(&self, manga_id: &MangaId) -> PathBuf {
        let mut hasher = Sha256::new();

        hasher.update(manga_id.source_id().value().as_bytes());
        hasher.update(manga_id.value().as_bytes());

        let encoded_hash = URL_SAFE_NO_PAD.encode(hasher.finalize());

        let poster_dir = self.downloads_folder_path.join(".posters");

        let file = poster_dir.join(format!("{}.jpg", encoded_hash));

        file
    }

    pub fn poster_exists(&self, manga_id: &MangaId) -> Option<PathBuf> {
        let file = self.path_for_poster(manga_id);

        if file.exists() {
            Some(file)
        } else {
            None
        }
    }

    pub async fn cached_poster<F, Fut>(
        &self,
        token: &CancellationToken,
        manga_id: &MangaId,
        req: F,
    ) -> Result<PathBuf>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<Request>>,
    {
        let poster_dir = self.downloads_folder_path.join(".posters");
        tokio::fs::create_dir_all(&poster_dir).await?;

        let file = self.path_for_poster(manga_id);
        if file.exists() {
            return Ok(file);
        }

        let client = crate::tls::client_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let bytes = tokio::select! {
            _ = token.cancelled() => Err(anyhow::anyhow!("cancelled")),
            result = async {
                let req = req().await?;
                let res = client.execute(req).await?.error_for_status()?;
                let bytes = res.bytes().await?;
                Ok(bytes)
            } => result,
        }?;

        tokio::fs::write(&file, &self.convert_image_data_to_jpeg(&bytes)?).await?;

        Ok(file)
    }

    fn convert_image_data_to_jpeg(&self, data: &[u8]) -> Result<Vec<u8>> {
        let (width, height, rgb_pixels) = {
            if let Some(data) = decode_image_fast(data) {
                let image = data?;

                let rgb_pixels = decode_argb_to_rgb(image.width, image.height, &image.data)?;
                (image.width as u32, image.height as u32, rgb_pixels)
            }
            // fallback with image
            else {
                let cursor = Cursor::new(data);
                let rgb_img = ImageReader::new(cursor)
                    .with_guessed_format()
                    .ok()
                    .and_then(|r| r.decode().ok())
                    .map(|img| img.to_rgb8())
                    .context("decode failed")?;

                let width = rgb_img.width();
                let height = rgb_img.height();

                (width, height, rgb_img.to_vec())
            }
        };

        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        comp.set_size(width as usize, height as usize);
        comp.set_fastest_defaults();

        let mut comp = comp.start_compress(Vec::new())?;
        comp.write_scanlines(&rgb_pixels)?;

        Ok(comp.finish()?)
    }

    pub fn get_stored_chapter_and_errors(
        &self,
        id: &ChapterId,
        use_ram: bool,
    ) -> anyhow::Result<
        Option<(
            PathBuf,
            Option<Vec<crate::chapter_downloader::DownloadError>>,
        )>,
    > {
        if let Some(path) = self.get_stored_chapter(id, use_ram) {
            let file_errors = self.errors_source_path(&path)?;

            let errors = match std::fs::read(&file_errors) {
                Ok(buffer) => {
                    serde_json::from_slice::<Vec<crate::chapter_downloader::DownloadError>>(&buffer)
                        .ok()
                }
                Err(_) => None,
            };

            return Ok(Some((path, errors)));
        }

        Ok(None)
    }

    pub fn get_stored_chapter(&self, id: &ChapterId, use_ram: bool) -> Option<PathBuf> {
        let new_path = self.path_for_chapter(id, false, use_ram);
        if new_path.exists() {
            return Some(new_path);
        }

        let new_path_novel = self.path_for_chapter(id, true, use_ram);
        if new_path_novel.exists() {
            return Some(new_path_novel);
        }

        if use_ram {
            return None;
        }

        // Backwards compatibility: check the old path format
        let old_path = self.path_for_chapter_legacy(id, false);
        if old_path.exists() {
            return Some(old_path);
        }

        let old_path_novel = self.path_for_chapter_legacy(id, true);
        if old_path_novel.exists() {
            Some(old_path_novel)
        } else {
            None
        }
    }

    pub fn is_stored_chapter_in_index(
        &self,
        id: &ChapterId,
        index: &std::collections::HashSet<PathBuf>,
    ) -> bool {
        let candidates = [
            self.path_for_chapter(id, false, false),
            self.path_for_chapter(id, true, false),
            self.path_for_chapter_legacy(id, false),
            self.path_for_chapter_legacy(id, true),
        ];

        candidates.into_iter().any(|path| index.contains(&path))
    }

    pub fn get_path_to_store_chapter(
        &self,
        id: &ChapterId,
        is_novel: bool,
        use_ram: bool,
    ) -> PathBuf {
        // New chapters should always use the new path format
        self.path_for_chapter(id, is_novel, use_ram)
    }

    pub fn errors_source_path(&self, path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!(".errors file has no parent directory"))?;

        let file_stem = path
            .file_stem()
            .ok_or_else(|| anyhow::anyhow!(".errors file has no filename stem"))?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Filename is not valid UTF-8"))?;

        let meta_name = format!(".{}.errors", file_stem);

        Ok(parent.join(meta_name))
    }

    // FIXME depending on `NamedTempFile` here is pretty ugly
    pub async fn persist_chapter(
        &self,
        id: &ChapterId,
        is_novel: bool,
        temporary_file: NamedTempFile,
        errors: &Vec<crate::chapter_downloader::DownloadError>,
        use_ram: bool,
    ) -> Result<PathBuf> {
        let mut current_size = self.cached_storage_size();
        let persisted_chapter_size = Size::from_bytes(temporary_file.as_file().metadata()?.len());

        while current_size + persisted_chapter_size > self.storage_size_limit {
            debug!(
                "persist_chapter: current storage is {current_size}/{}, new persisted chapter is \
                {persisted_chapter_size}, attempting to evict",
                self.storage_size_limit
            );

            let evicted_size = self
                .evict_least_recently_modified_chapter()
                .await
                .with_context(|| format!(
                    "while attempting to bring the storage size under the {} limit (current size: {}, persisted chapter size: {})",
                    self.storage_size_limit,
                    current_size,
                    persisted_chapter_size,
                ))?;

            let evicted_bytes = evicted_size.bytes() as u64;
            let current_bytes = current_size.bytes() as u64;
            current_size = Size::from_bytes(current_bytes.saturating_sub(evicted_bytes));
            self.cached_storage_size
                .fetch_sub(evicted_bytes, Ordering::Relaxed);
        }

        // Persist using the new path format
        let path = self.path_for_chapter(id, is_novel, use_ram);
        temporary_file.persist(&path)?;

        // Update cache with new file size
        if let Ok(metadata) = std::fs::metadata(&path) {
            self.cached_storage_size
                .fetch_add(metadata.len(), Ordering::Relaxed);
        }

        if !errors.is_empty() {
            let _ = std::fs::write(
                &self.errors_source_path(&path)?,
                serde_json::to_vec(&errors)?,
            );
        } else {
            let _ = std::fs::remove_file(&self.errors_source_path(&path)?);
        }

        Ok(path)
    }

    pub fn set_downloads_folder_path(&mut self, path: PathBuf) -> Result<()> {
        fs::create_dir_all(&path)
            .with_context(|| "while trying to ensure chapter storage exists")?;

        self.downloads_folder_path = path;

        Ok(())
    }

    // cache this function
    fn calculate_storage_size(&self) -> Size {
        let size_in_bytes: u64 = self
            .chapter_files_iterator()
            .filter_map(|entry| entry.metadata().ok().map(|metadata| metadata.len()))
            .sum();

        Size::from_bytes(size_in_bytes)
    }

    fn cached_storage_size(&self) -> Size {
        Size::from_bytes(self.cached_storage_size.load(Ordering::Relaxed))
    }

    /// Total size in bytes of persistently stored chapters, read from the
    /// in-memory counter maintained at startup and updated on every
    /// persist/delete. Performs no filesystem I/O.
    pub fn stored_size_bytes(&self) -> u64 {
        self.cached_storage_size.load(Ordering::Relaxed)
    }

    pub fn refresh_storage_size(&self) {
        self.cached_storage_size.store(
            self.calculate_storage_size().bytes() as u64,
            Ordering::Relaxed,
        );
    }

    async fn evict_least_recently_modified_chapter(&self) -> Result<Size> {
        let chapter_to_evict = self
            .find_least_recently_modified_chapter()?
            .ok_or_else(|| anyhow!("couldn't find any chapters to evict from storage"))?;

        debug!(
            "evict_least_recently_modified_chapter: evicting {}",
            chapter_to_evict.display()
        );

        let evicted_size = Size::from_bytes(
            std::fs::metadata(&chapter_to_evict)
                .map(|m| m.len())
                .unwrap_or(0),
        );

        let cloned_path = chapter_to_evict.clone();
        let _ = match tokio::fs::remove_file(chapter_to_evict).await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // Already deleted
            Err(e) => Err(anyhow!(
                "Failed to delete file {}: {}",
                cloned_path.display(),
                e
            )),
        };

        Ok(evicted_size)
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

    fn path_for_chapter(&self, chapter_id: &ChapterId, is_novel: bool, use_ram: bool) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(chapter_id.source_id().value().as_bytes());
        hasher.update(chapter_id.manga_id().value().as_bytes());
        hasher.update(chapter_id.value().as_bytes());
        let hash_result = hasher.finalize();

        // Use URL-safe base64 encoding without padding for the filename
        let encoded_hash = general_purpose::URL_SAFE_NO_PAD.encode(hash_result);

        let output_filename = format!("{}.{}", encoded_hash, if is_novel { "epub" } else { "cbz" });

        if use_ram && self.ram_enabled {
            self.tmpfs_path().join(output_filename)
        } else {
            self.downloads_folder_path.join(output_filename)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use size::Size;
    use tempfile::tempdir;

    fn make_storage() -> ChapterStorage {
        let dir = tempdir().unwrap();
        ChapterStorage::new(dir.keep(), Size::from_mebibytes(100.0), false).unwrap()
    }

    fn make_rgb_jpeg(width: u32, height: u32) -> Vec<u8> {
        let pixels: Vec<u8> = vec![128u8; (width * height * 3) as usize];
        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        comp.set_size(width as usize, height as usize);
        comp.set_fastest_defaults();
        let mut comp = comp.start_compress(Vec::new()).unwrap();
        comp.write_scanlines(&pixels).unwrap();
        comp.finish().unwrap()
    }

    fn output_dimensions(jpeg: &[u8]) -> (u32, u32) {
        let cursor = std::io::Cursor::new(jpeg);
        let img = image::ImageReader::new(cursor)
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn image_is_transcoded_to_jpeg() {
        let storage = make_storage();
        let input = make_rgb_jpeg(200, 300);
        let output = storage.convert_image_data_to_jpeg(&input).unwrap();
        let (w, h) = output_dimensions(&output);
        assert_eq!((w, h), (200, 300));
    }
}
