use anyhow::Result;

use crate::{
    chapter_storage::ChapterStorage, database::Database, model::MangaId, settings::Settings,
    usecases::revoke_manga_chapter::revoke_manga_chapter,
};

pub async fn remove_manga_from_library(
    db: &Database,
    chapter_storage: &ChapterStorage,
    settings: &Settings,
    id: MangaId,
) -> Result<()> {
    if settings.delete_downloaded_on_remove {
        let chapter_ids = db.get_manga_chapter_ids(&id).await?;
        for chapter_id in &chapter_ids {
            // Best-effort: failing to delete a single file shouldn't block removal.
            // Clean up both persistent and RAM-backed storage.
            let _ = revoke_manga_chapter(chapter_storage, chapter_id, false).await;
            let _ = revoke_manga_chapter(chapter_storage, chapter_id, true).await;
        }
    }

    db.remove_manga_from_library(id).await?;

    Ok(())
}
