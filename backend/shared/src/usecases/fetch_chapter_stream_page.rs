use tokio_util::sync::CancellationToken;

use crate::{
    chapter_storage::ChapterStorage,
    chapter_streamer::{self, FetchedPage},
    database::Database,
    model::ChapterId,
    source::Source,
};

/// Fetches a single chapter page (1-based index), going through the stream
/// cache. When RAM storage is enabled the primary cache lives on the tmpfs;
/// pages falling out of it are kept on persistent disk instead.
///
/// Callers must pass an owned/cloned [`ChapterStorage`] so no storage locks
/// are held across network fetches.
pub async fn fetch_chapter_stream_page(
    _token: &CancellationToken,
    database: &Database,
    source: &Source,
    chapter_storage: &ChapterStorage,
    chapter_id: &ChapterId,
    page_index: usize,
) -> Result<FetchedPage, Error> {
    let chapter = database
        .find_cached_chapter_information(chapter_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Expected chapter to be in the database"))?;

    // Resolve cache roots up front. When the tmpfs is already full, skip it
    // entirely and go straight to persistent disk.
    let tmpfs_full = chapter_storage.tmpfs_full_storage().await.unwrap_or(false);
    let primary_root = if tmpfs_full {
        chapter_storage.stream_pages_fallback_path()
    } else {
        chapter_storage.stream_pages_path()
    };
    let persistent_root = chapter_storage.stream_pages_fallback_path();
    let fallback_root = if primary_root == persistent_root {
        None
    } else {
        Some(persistent_root)
    };

    chapter_streamer::fetch_chapter_page(
        source,
        &primary_root,
        fallback_root.as_deref(),
        chapter_id,
        chapter.chapter_number,
        page_index,
    )
    .await
    .map_err(|err| match err {
        chapter_streamer::Error::PageOutOfRange { index } => Error::PageOutOfRange { index },
        chapter_streamer::Error::TextChapter => Error::TextChapter,
        chapter_streamer::Error::Fetch(e) => Error::Fetch(e),
        chapter_streamer::Error::Other(e) => Error::Other(e),
        // Page list errors are network errors, just like page fetches.
        chapter_streamer::Error::PageList(e) => Error::Fetch(e),
    })
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("page index {index} is out of range")]
    PageOutOfRange { index: usize },
    #[error("the chapter has no image pages")]
    TextChapter,
    #[error("an error occurred while fetching the page")]
    Fetch(#[source] anyhow::Error),
    #[error("unknown error")]
    Other(#[from] anyhow::Error),
}
