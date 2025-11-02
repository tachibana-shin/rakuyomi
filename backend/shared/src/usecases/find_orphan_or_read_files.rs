use std::path::PathBuf;

use anyhow::Result;

use crate::{chapter_storage::ChapterStorage, database::Database};

pub async fn find_orphan_or_read_files(
    db: &Database,
    chapter_storage: &ChapterStorage,
    invalid_mode: bool
) -> Result<Vec<PathBuf>> {
    Ok(db.find_orphan_or_read_files(chapter_storage, invalid_mode).await)
}
