use anyhow::Result;

use crate::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::{Chapter, MangaId},
};

pub async fn get_cached_manga_chapters(
    db: &Database,
    chapter_storage: &ChapterStorage,
    id: MangaId,
) -> Result<Vec<Chapter>> {
    Ok(db.find_cached_chapters(&id, &chapter_storage).await)
}
