use anyhow::Error;

use crate::{chapter_storage::ChapterStorage, model::ChapterId};

/// Deletes the downloaded file of a chapter (and its error-log sidecar), if
/// present. Deletion goes through [`ChapterStorage`] so the cached storage
/// size used for eviction decisions stays accurate. Returns whether a chapter
/// file was actually removed.
pub async fn revoke_manga_chapter(
    chapter_storage: &ChapterStorage,
    chapter: &ChapterId,
    use_ram: bool,
) -> Result<bool, Error> {
    let Some(path) = chapter_storage.get_stored_chapter(chapter, use_ram) else {
        // No chapter stored → nothing removed
        return Ok(false);
    };

    // Delete through ChapterStorage so `cached_storage_size` is decremented;
    // deleting the file directly would leave the eviction accounting stale.
    let removed_main = match path.file_name().and_then(|name| name.to_str()) {
        Some(filename) => chapter_storage
            .delete_filename(filename.to_string(), use_ram)
            .await
            .is_ok(),
        None => false,
    };

    // Get path to "errors file" (optional) and delete it,
    // but ignore all failures because it's best-effort cleanup.
    if let Ok(path_errors) = chapter_storage.errors_source_path(&path) {
        // fire-and-forget but awaited; failure ignored
        let _ = tokio::fs::remove_file(path_errors).await;
    }

    Ok(removed_main)
}
