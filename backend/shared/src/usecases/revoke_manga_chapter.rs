use anyhow::Error;

use crate::{chapter_storage::ChapterStorage, model::ChapterId};

pub async fn revoke_manga_chapter(
    chapter_storage: &ChapterStorage,
    chapter: &ChapterId,
) -> Result<bool, Error> {
    let Some(path) = chapter_storage.get_stored_chapter(chapter) else {
        // No chapter stored → nothing removed
        return Ok(false);
    };

    let removed_main = (tokio::fs::remove_file(&path).await).is_ok();

    // Get path to "errors file" (optional) and delete it,
    // but ignore all failures because it's best-effort cleanup.
    if let Ok(path_errors) = chapter_storage.errors_source_path(&path) {
        // fire-and-forget but awaited; failure ignored
        let _ = tokio::fs::remove_file(path_errors).await;
    }

    Ok(removed_main)
}
