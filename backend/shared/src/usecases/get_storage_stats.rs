use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{chapter_storage::ChapterStorage, database::Database, model::ChapterId};

/// Storage used by the downloaded chapters of a single manga.
#[derive(Serialize)]
pub struct MangaStorageUsage {
    /// ID of the source the manga belongs to.
    pub source_id: String,
    /// ID of the manga within its source.
    pub manga_id: String,
    /// Total size of the manga's downloaded chapter files, in bytes.
    pub bytes: u64,
}

/// Storage usage statistics for downloaded chapters of library mangas.
#[derive(Serialize)]
pub struct StorageStats {
    /// Total size of all downloaded chapter files, in bytes.
    pub total_bytes: u64,
    /// Per-manga breakdown of the total.
    pub per_manga: Vec<MangaStorageUsage>,
}

/// Computes how much space the downloaded chapters of every manga in the
/// library currently occupy, both in total and broken down per manga.
/// Includes RAM-backed (tmpfs) downloads when RAM storage is enabled.
///
/// The filesystem scan runs on a blocking thread so the async executor is
/// never stalled. It is deliberately sequential: e-reader eMMC gains nothing
/// from parallel I/O, and a sequential metadata-only walk keeps I/O pressure
/// at a minimum.
pub async fn get_storage_stats(
    db: &Database,
    chapter_storage: &ChapterStorage,
) -> Result<StorageStats> {
    let chapter_ids = db.get_library_chapter_ids().await?;
    let chapter_storage = chapter_storage.clone();

    tokio::task::spawn_blocking(move || scan_stored_chapters(&chapter_storage, chapter_ids))
        .await
        .context("storage stats scan task failed")
}

fn scan_stored_chapters(
    chapter_storage: &ChapterStorage,
    chapter_ids: Vec<ChapterId>,
) -> StorageStats {
    let mut totals: HashMap<(String, String), u64> = HashMap::new();
    let mut total_bytes: u64 = 0;
    let include_ram = chapter_storage.is_ram_enabled();

    for chapter_id in chapter_ids {
        let mut chapter_bytes = stored_chapter_size(chapter_storage, &chapter_id, false);
        if include_ram {
            chapter_bytes += stored_chapter_size(chapter_storage, &chapter_id, true);
        }

        if chapter_bytes > 0 {
            total_bytes += chapter_bytes;

            let key = (
                chapter_id.source_id().value().clone(),
                chapter_id.manga_id().value().clone(),
            );
            *totals.entry(key).or_insert(0) += chapter_bytes;
        }
    }

    let per_manga = totals
        .into_iter()
        .map(|((source_id, manga_id), bytes)| MangaStorageUsage {
            source_id,
            manga_id,
            bytes,
        })
        .collect();

    StorageStats {
        total_bytes,
        per_manga,
    }
}

/// Returns the size in bytes of a chapter's stored file, or 0 when the
/// chapter isn't stored (or its metadata can't be read).
fn stored_chapter_size(
    chapter_storage: &ChapterStorage,
    chapter_id: &ChapterId,
    use_ram: bool,
) -> u64 {
    let Some(path) = chapter_storage.get_stored_chapter(chapter_id, use_ram) else {
        return 0;
    };

    match std::fs::metadata(&path) {
        Ok(metadata) => metadata.len(),
        Err(_) => 0,
    }
}
