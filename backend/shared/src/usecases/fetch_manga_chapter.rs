use std::path::PathBuf;

use anyhow::anyhow;
use tokio_util::sync::CancellationToken;

use crate::{
    chapter_downloader::{
        ensure_chapter_is_in_storage, DownloadError, Error as ChapterDownloaderError,
    },
    chapter_storage::ChapterStorage,
    database::Database,
    model::ChapterId,
    source::Source,
};

pub async fn fetch_manga_chapter(
    token: &CancellationToken,
    database: &Database,
    source: &Source,
    chapter_storage: &ChapterStorage,
    chapter_id: &ChapterId,
    concurrent_requests_pages: usize,
) -> Result<(PathBuf, Vec<DownloadError>), Error> {
    let manga = database
        .find_cached_manga_information(chapter_id.manga_id())
        .await
        .ok_or_else(|| anyhow!("Expected manga to be in the database"))?;

    let chapter = database
        .find_cached_chapter_information(chapter_id)
        .await
        .ok_or_else(|| anyhow!("Expected chapter to be in the database"))?;

    ensure_chapter_is_in_storage(
        token,
        chapter_storage,
        source,
        &manga,
        &chapter,
        concurrent_requests_pages,
    )
    .await
    .map_err(|e| match e {
        ChapterDownloaderError::DownloadError(e) => Error::DownloadError(e),
        ChapterDownloaderError::Other(e) => Error::Other(e),
    })
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an error occurred while downloading the chapter pages")]
    DownloadError(#[source] anyhow::Error),
    #[error("unknown error")]
    Other(#[from] anyhow::Error),
}
