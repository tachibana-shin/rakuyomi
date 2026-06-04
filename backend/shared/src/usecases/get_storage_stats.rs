use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;

use crate::{chapter_storage::ChapterStorage, database::Database};

#[derive(Serialize)]
pub struct MangaStorageUsage {
    pub source_id: String,
    pub manga_id: String,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct StorageStats {
    pub total_bytes: u64,
    pub per_manga: Vec<MangaStorageUsage>,
}

/// Computes how much disk space the downloaded chapters of every manga in the
/// library currently occupy, both in total and broken down per manga.
pub async fn get_storage_stats(
    db: &Database,
    chapter_storage: &ChapterStorage,
) -> Result<StorageStats> {
    let chapter_ids = db.get_library_chapter_ids().await?;

    let mut totals: HashMap<(String, String), u64> = HashMap::new();
    let mut total_bytes: u64 = 0;

    for chapter_id in chapter_ids {
        if let Some(path) = chapter_storage.get_stored_chapter(&chapter_id, false) {
            if let Ok(metadata) = std::fs::metadata(&path) {
                let size = metadata.len();
                total_bytes += size;

                let key = (
                    chapter_id.source_id().value().clone(),
                    chapter_id.manga_id().value().clone(),
                );
                *totals.entry(key).or_insert(0) += size;
            }
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

    Ok(StorageStats {
        total_bytes,
        per_manga,
    })
}
