use std::io::Cursor;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
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
use crate::source::decode_image::decode_image_fast;

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

    pub async fn delete_filename(&self, filename: String) -> std::io::Result<()> {
        let file_path = self.downloads_folder_path.join(filename);
        tokio::fs::remove_file(file_path).await
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

        let client = reqwest::Client::new();
        let bytes = tokio::select! {
            _ = token.cancelled() => Err(anyhow::anyhow!("cancelled")),
            result = async {
                let req = req().await?;
                let res = client.execute(req).await?.error_for_status()?;
                let bytes = res.bytes().await?;
                Ok(bytes)
            } => result,
        }?;

        let jpeg_bytes = {
            let storage = self.clone();
            tokio::task::spawn_blocking(move || storage.convert_image_data_to_jpeg(&bytes)).await??
        };
        tokio::fs::write(&file, &jpeg_bytes).await?;

        Ok(file)
    }

    fn convert_image_data_to_jpeg(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Maximum poster dimensions. Covers are displayed as small thumbnails so
        // full-resolution originals are wasteful and can exceed KOReader's LRU
        // image cache when loaded as raw bitmaps.
        const MAX_WIDTH: u32 = 400;
        const MAX_HEIGHT: u32 = 600;

        let (width, height, rgb_pixels) = {
            if let Some(data) = decode_image_fast(data) {
                let image = data?;

                // RGBA に変換（元は ARGB）
                let mut rgb_pixels: Vec<u8> =
                    Vec::with_capacity((image.width * image.height * 3) as usize);

                for px in &image.data {
                    let _a = ((px >> 24) & 0xFF) as u8;
                    let r = ((px >> 16) & 0xFF) as u8;
                    let g = ((px >> 8) & 0xFF) as u8;
                    let b = (px & 0xFF) as u8;

                    // JPEG は alpha に対応しないため RGB のみ書き込む
                    rgb_pixels.extend_from_slice(&[r, g, b]);
                }

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

        // Downscale to fit within MAX_WIDTH x MAX_HEIGHT, preserving aspect ratio.
        let (width, height, rgb_pixels) = if width > MAX_WIDTH || height > MAX_HEIGHT {
            let scale = (MAX_WIDTH as f32 / width as f32).min(MAX_HEIGHT as f32 / height as f32);
            let new_width = ((width as f32 * scale).round() as u32).max(1);
            let new_height = ((height as f32 * scale).round() as u32).max(1);
            let img = image::RgbImage::from_raw(width, height, rgb_pixels)
                .context("failed to build image buffer for resize")?;
            let resized = image::imageops::resize(
                &img,
                new_width,
                new_height,
                image::imageops::FilterType::CatmullRom,
            );
            (new_width, new_height, resized.into_raw())
        } else {
            (width, height, rgb_pixels)
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
    ) -> anyhow::Result<
        Option<(
            PathBuf,
            Option<Vec<crate::chapter_downloader::DownloadError>>,
        )>,
    > {
        if let Some(path) = self.get_stored_chapter(id) {
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

    pub fn get_stored_chapter(&self, id: &ChapterId) -> Option<PathBuf> {
        let new_path = self.path_for_chapter(id, false);
        if new_path.exists() {
            return Some(new_path);
        }

        let new_path_novel = self.path_for_chapter(id, true);
        if new_path_novel.exists() {
            return Some(new_path_novel);
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

    pub fn get_path_to_store_chapter(&self, id: &ChapterId, is_novel: bool) -> PathBuf {
        // New chapters should always use the new path format
        self.path_for_chapter(id, is_novel)
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
                .await
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
            .filter_map(|entry| entry.metadata().ok().map(|metadata| metadata.size()))
            .sum();

        Size::from_bytes(size_in_bytes)
    }

    async fn evict_least_recently_modified_chapter(&self) -> Result<()> {
        let chapter_to_evict = self
            .find_least_recently_modified_chapter()?
            .ok_or_else(|| anyhow!("couldn't find any chapters to evict from storage"))?;

        debug!(
            "evict_least_recently_modified_chapter: evicting {}",
            chapter_to_evict.display()
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

#[cfg(test)]
mod tests {
    use super::*;
    use size::Size;
    use tempfile::tempdir;

    fn make_storage() -> ChapterStorage {
        let dir = tempdir().unwrap();
        ChapterStorage::new(dir.into_path(), Size::from_mebibytes(100.0)).unwrap()
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
    fn small_image_is_stored_unchanged() {
        let storage = make_storage();
        let input = make_rgb_jpeg(200, 300);
        let output = storage.convert_image_data_to_jpeg(&input).unwrap();
        let (w, h) = output_dimensions(&output);
        assert_eq!((w, h), (200, 300));
    }

    #[test]
    fn wide_image_is_capped_at_max_width() {
        let storage = make_storage();
        // 1200x400 — wider than MAX_WIDTH (400), height within limit
        let input = make_rgb_jpeg(1200, 400);
        let output = storage.convert_image_data_to_jpeg(&input).unwrap();
        let (w, h) = output_dimensions(&output);
        assert!(w <= 400, "width {w} exceeds MAX_WIDTH");
        // Aspect ratio preserved: 1200/400 = 3.0, so h should be ~133
        assert_eq!(w, 400);
        assert_eq!(h, 133);
    }

    #[test]
    fn tall_image_is_capped_at_max_height() {
        let storage = make_storage();
        // 200x1200 — taller than MAX_HEIGHT (600), width within limit
        let input = make_rgb_jpeg(200, 1200);
        let output = storage.convert_image_data_to_jpeg(&input).unwrap();
        let (w, h) = output_dimensions(&output);
        assert!(h <= 600, "height {h} exceeds MAX_HEIGHT");
        assert_eq!(h, 600);
        assert_eq!(w, 100);
    }

    #[test]
    fn large_portrait_cover_fits_within_bounds() {
        let storage = make_storage();
        // Typical high-res manga cover
        let input = make_rgb_jpeg(1400, 2100);
        let output = storage.convert_image_data_to_jpeg(&input).unwrap();
        let (w, h) = output_dimensions(&output);
        assert!(w <= 400, "width {w} exceeds MAX_WIDTH");
        assert!(h <= 600, "height {h} exceeds MAX_HEIGHT");
    }

    #[test]
    fn exact_max_dimensions_are_not_resized() {
        let storage = make_storage();
        let input = make_rgb_jpeg(400, 600);
        let output = storage.convert_image_data_to_jpeg(&input).unwrap();
        let (w, h) = output_dimensions(&output);
        assert_eq!((w, h), (400, 600));
    }
}
