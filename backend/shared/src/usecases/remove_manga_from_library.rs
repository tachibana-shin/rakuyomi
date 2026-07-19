use anyhow::Result;

use crate::{
    chapter_storage::ChapterStorage, database::Database, model::MangaId,
    usecases::revoke_manga_chapter::revoke_manga_chapter,
};

/// Removes a manga from the user's library. When `delete_downloaded_on_remove`
/// is enabled, the manga's downloaded chapter files are deleted first on a
/// best-effort basis; failing to delete individual files never blocks the
/// removal itself.
pub async fn remove_manga_from_library(
    db: &Database,
    chapter_storage: &ChapterStorage,
    delete_downloaded_on_remove: bool,
    id: MangaId,
) -> Result<()> {
    if delete_downloaded_on_remove {
        let chapter_ids = db.find_cached_chapter_ids(&id).await?;
        for chapter_id in &chapter_ids {
            let _ = revoke_manga_chapter(chapter_storage, chapter_id, false).await;
        }
    }

    db.remove_manga_from_library(id).await?;

    Ok(())
}
