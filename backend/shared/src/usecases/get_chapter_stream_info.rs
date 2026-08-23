use anyhow::anyhow;
use tokio_util::sync::CancellationToken;

use crate::{
    chapter_streamer::{self, StreamInfo},
    database::Database,
    model::ChapterId,
    source::Source,
};

/// Returns metadata about a chapter's pages, fetching the page list from the
/// source when needed, without downloading anything.
pub async fn get_chapter_stream_info(
    _token: &CancellationToken,
    database: &Database,
    source: &Source,
    chapter_id: &ChapterId,
) -> Result<StreamInfo, Error> {
    let chapter = database
        .find_cached_chapter_information(chapter_id)
        .await?
        .ok_or_else(|| anyhow!("Expected chapter to be in the database"))?;

    chapter_streamer::stream_info(source, chapter_id, chapter.chapter_number)
        .await
        .map_err(|err| match err {
            chapter_streamer::Error::PageList(e) => Error::PageList(e),
            chapter_streamer::Error::Other(e) => Error::Other(e),
            err => Error::Other(err.into()),
        })
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an error occurred while fetching the page list")]
    PageList(#[source] anyhow::Error),
    #[error("unknown error")]
    Other(#[from] anyhow::Error),
}
