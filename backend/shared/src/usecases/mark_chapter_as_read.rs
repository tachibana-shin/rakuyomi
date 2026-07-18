use anyhow::Result;

use crate::{
    chapter_storage::ChapterStorage, database::Database, model::ChapterId,
    usecases::revoke_manga_chapter::revoke_manga_chapter,
};

/// Sets the read state of a single chapter. A `value` of `None` marks the
/// chapter as read (the default action). When the chapter ends up marked as
/// read and `delete_downloaded_after_read` is enabled, its downloaded file is
/// deleted on a best-effort basis; cleanup failures never affect the
/// already-persisted read state.
pub async fn mark_chapter_as_read(
    db: &Database,
    chapter_storage: &ChapterStorage,
    delete_downloaded_after_read: bool,
    id: &ChapterId,
    value: Option<bool>,
) -> Result<()> {
    db.mark_chapter_as_read(id, value).await?;

    let marking_as_read = value.unwrap_or(true);
    if marking_as_read && delete_downloaded_after_read {
        let _ = revoke_manga_chapter(chapter_storage, id, false).await;
    }

    Ok(())
}
