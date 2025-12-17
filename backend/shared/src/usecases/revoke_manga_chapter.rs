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

    let removed_main = match tokio::fs::remove_file(&path).await {
        Ok(_) => true,
        Err(_) => false,
    };

    // Get path to "errors file" (optional) and delete it,
    // but ignore all failures because it's best-effort cleanup.
    if let Some(path_errors) = chapter_storage.errors_source_path(&path).ok() {
        // fire-and-forget but awaited; failure ignored
        let _ = tokio::fs::remove_file(path_errors).await;
    }

    Ok(removed_main)
}
