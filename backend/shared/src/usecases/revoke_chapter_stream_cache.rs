use crate::{chapter_storage::ChapterStorage, model::ChapterId};

/// Deletes the page stream cache of a chapter, if any — both the primary
/// location (tmpfs when RAM storage is enabled) and the persistent fallback.
/// Best-effort: failures are logged inside [`chapter_streamer`] and
/// otherwise ignored.
///
/// [`chapter_streamer`]: crate::chapter_streamer
pub async fn revoke_chapter_stream_cache(chapter_storage: &ChapterStorage, chapter: &ChapterId) {
    let primary_root = chapter_storage.stream_pages_path();
    let persistent_root = chapter_storage.stream_pages_fallback_path();

    crate::chapter_streamer::revoke_chapter_cache(&persistent_root, chapter).await;
    if primary_root != persistent_root {
        crate::chapter_streamer::revoke_chapter_cache(&primary_root, chapter).await;
    }
}
