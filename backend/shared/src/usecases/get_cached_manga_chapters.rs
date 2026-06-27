use anyhow::Result;

use crate::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::{Chapter, MangaId},
};

pub async fn get_cached_manga_chapters(
    db: &Database,
    chapter_storage: &ChapterStorage,
    id: &MangaId,
    ram_mode_enabled: bool,
) -> Result<Vec<Chapter>> {
    db.find_cached_chapters(id, chapter_storage, ram_mode_enabled)
        .await
}
