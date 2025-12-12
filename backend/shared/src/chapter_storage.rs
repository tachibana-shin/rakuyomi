use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::{fs, future::Future};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::debug;
use sha2::{Digest, Sha256};
use size::Size;
use tempfile::NamedTempFile;
use tokio::io::AsyncReadExt;
use walkdir::{DirEntry, WalkDir};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use image::ImageFormat;
use reqwest::Request;

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

    pub async fn cache_poster(&self, url: &url::Url) -> Result<Option<PathBuf>> {
        let mut hasher = Sha256::new();
        hasher.update(url.as_str().as_bytes());
        let encoded_hash = URL_SAFE_NO_PAD.encode(hasher.finalize());

        let poster_dir = self.downloads_folder_path.join(".posters");

        let meta_path = poster_dir.join(format!(".{encoded_hash}"));

        if meta_path.exists() {
            let mut f = tokio::fs::File::open(&meta_path).await?;
            let mut ext = String::new();
            f.read_to_string(&mut ext).await?;

            let cached_path = poster_dir.join(format!("{encoded_hash}.{}", ext));

            if cached_path.exists() {
                return Ok(Some(cached_path));
            }
        }

        Ok(None)
    }

    pub async fn cached_poster<F, Fut>(&self, url: &url::Url, req: F) -> Result<PathBuf>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<Request>>,
    {
        // --- Hash URL for stable filename ---
        let mut hasher = Sha256::new();
        hasher.update(url.as_str().as_bytes());
        let encoded_hash = URL_SAFE_NO_PAD.encode(hasher.finalize());

        // --- Directory for posters ---
        let poster_dir = self.downloads_folder_path.join(".posters");
        tokio::fs::create_dir_all(&poster_dir).await?;

        // --- Sidecar: stores only extension ---
        let meta_path = poster_dir.join(format!(".{encoded_hash}"));

        // ============================================================
        // FAST PATH → Use existing cache without doing HTTP requests
        // ============================================================
        if meta_path.exists() {
            let mut f = tokio::fs::File::open(&meta_path).await?;
            let mut ext = String::new();
            f.read_to_string(&mut ext).await?;

            let cached_path = poster_dir.join(format!("{encoded_hash}.{}", ext));

            if cached_path.exists() {
                return Ok(cached_path);
            }
        }

        // ============================================================
        // SLOW PATH → download file with custom headers
        // ============================================================

        let client = reqwest::Client::new();
        let req = req().await?;

        let res = client.execute(req).await?.error_for_status()?;

        // --- Detect extension from Content-Type ---
        let mut ext: Option<&str> = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .and_then(|mime| match mime {
                "image/jpeg" => Some("jpg"),
                "image/png" => Some("png"),
                "image/webp" => Some("webp"),
                "image/avif" => Some("avif"),
                "image/gif" => Some("gif"),
                _ => None,
            });

        // --- Detect from URL path ---
        if ext.is_none() {
            if let Some(path) = url.path().rsplit('/').next() {
                if let Some(dot) = path.split('.').last() {
                    ext = match dot.to_lowercase().as_str() {
                        "jpg" | "jpeg" => Some("jpg"),
                        "png" => Some("png"),
                        "webp" => Some("webp"),
                        "gif" => Some("gif"),
                        "avif" => Some("avif"),
                        _ => None,
                    };
                }
            }
        }
        let bytes = res.bytes().await?;

        // --- Detect from actual image bytes ---
        if ext.is_none() {
            if let Ok(format) = image::guess_format(&bytes) {
                ext = Some(match format {
                    ImageFormat::Jpeg => "jpg",
                    ImageFormat::Png => "png",
                    ImageFormat::WebP => "webp",
                    ImageFormat::Gif => "gif",
                    ImageFormat::Avif => "avif",
                    _ => anyhow::bail!("unsupported image format"),
                });
            }
        }

        // 最後のフォールバック
        let ext = ext.unwrap_or("jpg");

        let poster_path = poster_dir.join(format!("{encoded_hash}.{ext}"));

        // --- Save poster file ---
        tokio::fs::write(&poster_path, &bytes).await?;

        // --- Save metadata sidecar (.hash → ext) ---
        tokio::fs::write(&meta_path, ext).await?;

        Ok(poster_path)
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
                Ok(buffer) => match serde_json::from_slice::<
                    Vec<crate::chapter_downloader::DownloadError>,
                >(&buffer)
                {
                    Ok(list) => Some(list),
                    Err(_) => None,
                },
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
            return Some(old_path_novel);
        } else {
            return None;
        }
    }

    pub fn get_path_to_store_chapter(&self, id: &ChapterId, is_novel: bool) -> PathBuf {
        // New chapters should always use the new path format
        self.path_for_chapter(id, is_novel)
    }

    pub fn errors_source_path(&self, path: &PathBuf) -> anyhow::Result<std::path::PathBuf> {
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

        if errors.len() > 0 {
            let _ = std::fs::write(
                &self.errors_source_path(&path)?,
                serde_json::to_vec(&errors)?,
            );
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
