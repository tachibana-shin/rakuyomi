//! On-demand, per-page chapter streaming.
//!
//! Instead of materializing a whole CBZ before the reader can open it, this
//! module exposes primitives to fetch chapter pages one at a time, caching
//! them on disk under `<downloads parent>/stream_pages/<chapter hash>/` so
//! revisits (and tile cache misses inside KOReader) don't hit the network.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use log::{error, info, warn};
use reqwest::redirect::Policy;
use sha2::{Digest, Sha256};
use tokio_util::bytes::Bytes;
use tokio_util::sync::CancellationToken;

use crate::model::ChapterId;
use crate::source::{model::Page, Source};
use crate::unscrable_image::{unscrable_image, Block};
use crate::util::{
    detect_image_extension, generate_error_image, request_with_forced_referer_from_request,
};

/// Maximum number of page lists kept in memory. Page lists are small
/// (a few KB each), so this is only a safety valve against unbounded growth.
const MAX_CACHED_PAGE_LISTS: usize = 16;

/// Maximum size of the on-disk stream page cache. When exceeded after an
/// insert, whole-chapter directories are evicted oldest-first.
pub const STREAM_CACHE_SIZE_LIMIT_BYTES: u64 = 512 * 1024 * 1024;

static PAGE_LISTS: tokio::sync::Mutex<Option<HashMap<String, Arc<Vec<Page>>>>> =
    tokio::sync::Mutex::const_new(None);

/// One lock per chapter, so concurrent requests for pages of different
/// chapters never serialize behind each other, while racing requests for the
/// same chapter fetch its page list exactly once.
static PAGE_LIST_LOCKS: tokio::sync::Mutex<Option<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    tokio::sync::Mutex::const_new(None);

#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamInfo {
    pub page_count: usize,
    pub is_novel: bool,
}

#[derive(Debug)]
pub struct FetchedPage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("page index {index} is out of range")]
    PageOutOfRange { index: usize },

    #[error("the chapter has no image pages")]
    TextChapter,

    #[error("an error occurred while fetching the chapter's page list")]
    PageList(#[source] anyhow::Error),

    #[error("an error occurred while fetching the page")]
    Fetch(#[source] anyhow::Error),

    #[error("unknown error")]
    Other(#[from] anyhow::Error),
}

async fn page_list_lock(chapter_key: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = PAGE_LIST_LOCKS.lock().await;
    let map = locks.get_or_insert_with(HashMap::new);
    map.entry(chapter_key.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Returns the cached page list for the chapter, fetching and caching it from
/// the source when missing. Concurrent calls for the same chapter share the
/// fetch through a per-chapter lock.
pub async fn cached_page_list(
    source: &Source,
    chapter_id: &ChapterId,
    chapter_num: Option<f32>,
) -> Result<Arc<Vec<Page>>, Error> {
    let chapter_key = chapter_key(chapter_id);

    {
        let guard = PAGE_LISTS.lock().await;
        if let Some(map) = guard.as_ref() {
            if let Some(pages) = map.get(&chapter_key) {
                return Ok(pages.clone());
            }
        }
    }

    let lock = page_list_lock(&chapter_key).await;
    let _guard = lock.lock().await;

    // Double-check after acquiring the lock: another task may have filled
    // the entry while we were waiting for it.
    {
        let guard = PAGE_LISTS.lock().await;
        if let Some(map) = guard.as_ref() {
            if let Some(pages) = map.get(&chapter_key) {
                return Ok(pages.clone());
            }
        }
    }

    let pages = source
        .get_page_list(
            CancellationToken::new(),
            chapter_id.manga_id().value().clone(),
            chapter_id.value().clone(),
            chapter_num,
        )
        .await
        .map_err(|err| Error::PageList(anyhow!("Failed to get page list: {err:#}")))?;

    if pages.is_empty() {
        return Err(Error::PageList(anyhow!(
            "No pages found for chapter {}",
            chapter_id.value()
        )));
    }

    let pages = Arc::new(pages);
    let mut guard = PAGE_LISTS.lock().await;
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_CACHED_PAGE_LISTS {
        // Simple safety valve: drop everything. Refetching a page list is cheap.
        map.clear();
    }
    map.insert(chapter_key, pages.clone());

    Ok(pages)
}

/// Returns stream metadata (page count, whether it is a text chapter).
pub async fn stream_info(
    source: &Source,
    chapter_id: &ChapterId,
    chapter_num: Option<f32>,
) -> Result<StreamInfo, Error> {
    let pages = cached_page_list(source, chapter_id, chapter_num).await?;
    let is_novel = pages.first().and_then(|p| p.text.as_ref()).is_some();

    Ok(StreamInfo {
        page_count: pages.len(),
        is_novel,
    })
}

/// Fetches a single page of the chapter, going through the on-disk cache.
///
/// `primary_root` is where new pages are cached (RAM-backed tmpfs when RAM
/// storage is enabled); `fallback_root` is the persistent location used when
/// the primary runs out of space. Cache lookups check both.
pub async fn fetch_chapter_page(
    source: &Source,
    primary_root: &Path,
    fallback_root: Option<&Path>,
    chapter_id: &ChapterId,
    chapter_num: Option<f32>,
    index: usize,
) -> Result<FetchedPage, Error> {
    if index == 0 {
        return Err(Error::PageOutOfRange { index });
    }

    let pages = cached_page_list(source, chapter_id, chapter_num).await?;
    let page = pages
        .get(index - 1)
        .ok_or(Error::PageOutOfRange { index })?;

    if page.text.is_some() {
        return Err(Error::TextChapter);
    }

    let cache_dir = chapter_cache_dir(primary_root, chapter_id);
    let fallback_dir = fallback_root.map(|root| chapter_cache_dir(root, chapter_id));

    let mut cached_path = find_cached_page(&cache_dir, index);
    if cached_path.is_none() {
        cached_path = fallback_dir
            .as_deref()
            .and_then(|dir| find_cached_page(dir, index));
    }
    if let Some(path) = cached_path {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| detect_extension_from_filename(ext));
        if let (Some(extension), Ok(bytes)) = (extension, tokio::fs::read(&path).await) {
            info!(
                "stream [{}]: page {}/{} served from cache ({} bytes)",
                chapter_id.value(),
                index,
                pages.len(),
                bytes.len()
            );
            return Ok(FetchedPage { bytes, extension });
        }

        warn!(
            "Failed to read cached stream page at {}, refetching",
            path.display()
        );
    }

    let started_at = std::time::Instant::now();
    info!(
        "stream [{}]: page {}/{} downloading...",
        chapter_id.value(),
        index,
        pages.len()
    );

    let (bytes, extension) = download_page(source, chapter_id, page, index).await?;

    info!(
        "stream [{}]: page {}/{} downloaded ({} bytes, {}) in {:?}",
        chapter_id.value(),
        index,
        pages.len(),
        bytes.len(),
        extension,
        started_at.elapsed()
    );

    if store_cached_page(&cache_dir, index, extension, &bytes)
        .await
        .is_err()
    {
        // Most likely out of space on the primary storage (e.g. a small
        // tmpfs): fall back to persistent disk.
        warn!(
            "Failed to store stream page in {}, trying fallback",
            cache_dir.display()
        );
        match fallback_dir {
            Some(fallback_dir) => {
                match store_cached_page(&fallback_dir, index, extension, &bytes).await {
                    Ok(()) => enforce_cache_limit(fallback_root.unwrap_or(primary_root)).await,
                    Err(err) => {
                        warn!("Failed to store stream page in fallback cache: {err:#}")
                    }
                }
            }
            None => warn!("No fallback storage for stream page cache"),
        }
    } else {
        enforce_cache_limit(primary_root).await;
    }

    Ok(FetchedPage { bytes, extension })
}

/// Deletes the on-disk stream cache of a chapter. Missing directories are not
/// an error.
pub async fn revoke_chapter_cache(stream_pages_root: &Path, chapter_id: &ChapterId) {
    let dir = chapter_cache_dir(stream_pages_root, chapter_id);
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(_) => info!("Deleted stream page cache at {}", dir.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => warn!(
            "Failed to delete stream page cache {}: {}",
            dir.display(),
            err
        ),
    }
}

fn chapter_key(chapter_id: &ChapterId) -> String {
    format!(
        "{}:{}:{}",
        chapter_id.source_id().value(),
        chapter_id.manga_id().value(),
        chapter_id.value()
    )
}

fn chapter_hash(chapter_id: &ChapterId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(chapter_id.source_id().value().as_bytes());
    hasher.update(chapter_id.manga_id().value().as_bytes());
    hasher.update(chapter_id.value().as_bytes());

    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn chapter_cache_dir(stream_pages_root: &Path, chapter_id: &ChapterId) -> PathBuf {
    stream_pages_root.join(chapter_hash(chapter_id))
}

fn find_cached_page(cache_dir: &Path, index: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(cache_dir).ok()?;
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem == index.to_string())
        })
}

fn detect_extension_from_filename(extension: &str) -> Option<&'static str> {
    // Map stored file extensions back to the canonical set returned by
    // `detect_image_extension`, since they double as content-type keys.
    match extension.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        "bmp" => Some("bmp"),
        "avif" => Some("avif"),
        "jxl" => Some("jxl"),
        _ => None,
    }
}

async fn store_cached_page(
    cache_dir: &Path,
    index: usize,
    extension: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(cache_dir)
        .await
        .with_context(|| format!("while creating {}", cache_dir.display()))?;

    let output_path = cache_dir.join(format!("{index}.{extension}"));
    let temporary_file = tempfile::NamedTempFile::new_in(cache_dir)?;
    std::fs::write(temporary_file.path(), bytes)?;
    temporary_file
        .persist(&output_path)
        .map_err(|err| anyhow!("failed to persist {}: {err}", output_path.display()))?;

    Ok(())
}

/// Keeps the total size of the stream cache under
/// [`STREAM_CACHE_SIZE_LIMIT_BYTES`] by deleting whole-chapter directories,
/// oldest-modified first. Runs on the blocking thread pool, and only once
/// every [`CACHE_ENFORCEMENT_INTERVAL`] inserts: scanning the whole cache on
/// every single page store would be wasteful.
async fn enforce_cache_limit(stream_pages_root: &Path) {
    static INSERT_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let count = INSERT_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if !count.is_multiple_of(CACHE_ENFORCEMENT_INTERVAL) {
        return;
    }

    let root = stream_pages_root.to_path_buf();
    let result = tokio::task::spawn_blocking(move || enforce_cache_limit_sync(&root)).await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => warn!("Stream cache eviction failed: {err:#}"),
        Err(err) => warn!("Stream cache eviction task failed: {err}"),
    }
}

/// Run cache enforcement after every N-th inserted page.
const CACHE_ENFORCEMENT_INTERVAL: u64 = 16;

fn enforce_cache_limit_sync(stream_pages_root: &Path) -> anyhow::Result<()> {
    let mut dirs: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
    let mut total: u64 = 0;
    for entry in std::fs::read_dir(stream_pages_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let metadata = entry.metadata()?;
        let modified = metadata.modified()?;
        let dir_size = dir_size_recursive(&path).unwrap_or(0);
        total += dir_size;
        dirs.push((path, modified, dir_size));
    }

    if total <= STREAM_CACHE_SIZE_LIMIT_BYTES {
        return Ok(());
    }

    dirs.sort_by_key(|(_, modified, _)| *modified);
    for (path, _, dir_size) in dirs {
        if total <= STREAM_CACHE_SIZE_LIMIT_BYTES {
            break;
        }
        info!(
            "Evicting stream page cache at {} to free space",
            path.display()
        );
        if std::fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(dir_size);
        }
    }

    Ok(())
}

fn dir_size_recursive(path: &Path) -> anyhow::Result<u64> {
    let mut total = 0;
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry?;
        if entry.file_type().is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

type DownloadedPage = (Vec<u8>, &'static str);

/// Downloads a single page from the source, mirroring the logic used when
/// assembling CBZ files (keiyoushi interception, WASM request rewriting and
/// per-source post-processing), with an error image fallback instead of a
/// hard failure.
async fn download_page(
    source: &Source,
    chapter_id: &ChapterId,
    page: &Page,
    index: usize,
) -> Result<DownloadedPage, Error> {
    let image_url = page
        .image_url
        .clone()
        .ok_or_else(|| Error::Fetch(anyhow!("page {index} has no image URL")))?;

    let url_extension = Path::new(image_url.path())
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(detect_extension_from_filename)
        .unwrap_or("jpg");

    let response_bytes: Bytes = match &source.backend {
        crate::source::SourceBackend::Keiyoushi(_) => {
            // keiyoushi images may be IMGX-encrypted: fetch through the
            // extension's own client so its interceptor decrypts them.
            let bytes = source
                .fetch_page_image(chapter_id.value(), image_url.as_str())
                .await
                .map_err(|err| Error::Fetch(anyhow!("keiyoushi image fetch failed: {err}")))?;

            if detect_image_extension(&bytes).is_none() {
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(16)]);
                error!("keiyoushi page {index}: invalid image data ({head})");
                return error_page("Error", "invalid image data").await;
            }

            Bytes::from(bytes)
        }
        _ => {
            let client = crate::tls::client_builder()
                .timeout(std::time::Duration::from_secs(30))
                .redirect(Policy::none())
                .build()
                .map_err(|err| Error::Fetch(err.into()))?;

            let request = source
                .get_image_request(image_url.clone(), page.ctx.clone())
                .await
                .map_err(|err| Error::Fetch(anyhow!("WASM modify request failed: {err}")))?;

            let req_url = request.url().clone();
            let req_headers = request.headers().clone();
            let response = request_with_forced_referer_from_request(&client, request, 10)
                .await
                .map_err(|err| Error::Fetch(anyhow!("request failed: {err}")))?;

            if !response.status().is_success() {
                let status_text = response.status().canonical_reason().unwrap_or("Unknown");
                let status_code = response.status().as_u16().to_string();
                error!("page {index} ({req_url}): HTTP {}", status_code);
                return error_page(&status_code, status_text).await;
            }

            let status = response.status();
            let headers = response.headers().clone();
            let body = response
                .bytes()
                .await
                .map_err(|err| Error::Fetch(err.into()))?;

            if source.features.process_page_image {
                let processed = source
                    .process_page_image(
                        CancellationToken::new(),
                        (req_url, req_headers),
                        (status, headers),
                        body.clone(),
                        page.ctx.clone(),
                    )
                    .await
                    .map_err(|err| Error::Fetch(anyhow!("WASM process page failed: {err}")))?;

                Bytes::from(processed)
            } else {
                body
            }
        }
    };

    // Sources like mangayomi may serve scrambled images: when the page
    // carries a blocks descriptor, descramble before anything else —
    // mirroring `chapter_downloader`.
    let response_bytes = if let Some(blocks_json) = page.base64.as_ref() {
        let blocks: Vec<Block> = serde_json::from_str(blocks_json)
            .map_err(|err| Error::Fetch(anyhow!("Invalid blocks JSON: {err:?}")))?;

        let scrambled = response_bytes.to_vec();
        let unscrambled = tokio::task::spawn_blocking(move || unscrable_image(scrambled, blocks))
            .await
            .map_err(|err| Error::Fetch(anyhow!("unscrable_image task failed: {err}")))?
            .map_err(|err| {
                error!("page {index}: unscrable_image failed: {err}");
                Error::Fetch(anyhow!("unscrable_image failed: {err}"))
            })?;

        Bytes::from(unscrambled)
    } else {
        response_bytes
    };

    // Trust the magic bytes of the final image over any URL-derived
    // extension (keiyoushi URLs may end in disguised extensions).
    let extension = detect_image_extension(&response_bytes).unwrap_or(url_extension);

    Ok((response_bytes.to_vec(), extension))
}

/// Generates a placeholder error image, so a single broken page does not kill
/// the reading session.
async fn error_page(status_or_code: &str, msg: &str) -> Result<DownloadedPage, Error> {
    generate_error_image(status_or_code, msg, 500, 667)
        .map(|bytes| (bytes, "png"))
        .map_err(|err| {
            warn!("Failed to generate error image: {err:#}");
            Error::Fetch(err)
        })
}
