use serde::Serialize;

use crate::chapter_storage::ChapterStorage;

/// Storage usage statistics for downloaded chapters.
#[derive(Serialize)]
pub struct StorageStats {
    /// Total size of persistently stored chapter files, in bytes.
    pub total_bytes: u64,
}

/// Returns how much space downloaded chapters currently occupy, read from
/// the in-memory counter [`ChapterStorage`] already maintains for eviction
/// decisions — no database queries and no filesystem I/O.
pub fn get_storage_stats(chapter_storage: &ChapterStorage) -> StorageStats {
    StorageStats {
        total_bytes: chapter_storage.stored_size_bytes(),
    }
}
