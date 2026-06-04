use anyhow::Result;

use crate::{
    chapter_storage::ChapterStorage, database::Database, model::ChapterId, settings::Settings,
    usecases::revoke_manga_chapter::revoke_manga_chapter,
};

pub async fn mark_chapter_as_read(
    db: &Database,
    chapter_storage: &ChapterStorage,
    settings: &Settings,
    id: &ChapterId,
    value: Option<bool>,
) -> Result<()> {
    db.mark_chapter_as_read(id, value).await?;

    // When the chapter is being marked as read (the default action when no
    // explicit value is provided), optionally delete its downloaded file.
    let marking_as_read = value.unwrap_or(true);
    if marking_as_read && settings.delete_downloaded_after_read {
        // Best-effort cleanup in both persistent and RAM-backed storage;
        // read state has already been persisted.
        let _ = revoke_manga_chapter(chapter_storage, id, false).await;
        let _ = revoke_manga_chapter(chapter_storage, id, true).await;
    }

    Ok(())
}
