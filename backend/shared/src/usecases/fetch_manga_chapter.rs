use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use tokio_util::sync::CancellationToken;

use crate::{
    chapter_downloader::{
        ensure_chapter_is_in_storage, DownloadError, Error as ChapterDownloaderError,
    },
    chapter_storage::ChapterStorage,
    database::Database,
    model::ChapterId,
    settings::ChapterTitleFormat,
    source::Source,
};

/// Errors returned while fetching a manga chapter.
pub use crate::chapter_downloader::Error;

#[allow(clippy::too_many_arguments)]
pub async fn fetch_manga_chapter(
    token: &CancellationToken,
    database: &Database,
    source: &Source,
    chapter_storage: &ChapterStorage,
    chapter_id: &ChapterId,
    concurrent_requests_pages: usize,
    optimize_image: bool,
    on_progress: Option<Arc<dyn Fn(f32, f32) + Send + Sync>>,
    use_ram: bool,
    chapter_title_format: ChapterTitleFormat,
) -> Result<(PathBuf, Vec<DownloadError>), Error> {
    let manga = database
        .find_cached_manga_information(chapter_id.manga_id())
        .await?
        .ok_or_else(|| anyhow!("Expected manga to be in the database"))?;

    let chapter = database
        .find_cached_chapter_information(chapter_id)
        .await?
        .ok_or_else(|| anyhow!("Expected chapter to be in the database"))?;

    match ensure_chapter_is_in_storage(
        token,
        chapter_storage,
        source,
        &manga,
        &chapter,
        concurrent_requests_pages,
        optimize_image,
        on_progress.clone(),
        use_ram,
        None,
        chapter_title_format,
    )
    .await
    {
        Ok(v) => Ok(v),
        Err(ChapterDownloaderError::Other(_))
            if use_ram && chapter_storage.tmpfs_full_storage().await? =>
        {
            ensure_chapter_is_in_storage(
                token,
                chapter_storage,
                source,
                &manga,
                &chapter,
                concurrent_requests_pages,
                optimize_image,
                on_progress.clone(),
                false,
                None,
                chapter_title_format,
            )
            .await
        }
        result => result,
    }
}
