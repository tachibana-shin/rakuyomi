use futures::{stream, StreamExt, TryStreamExt};
use rust_decimal::prelude::*;
use std::{
    io::Seek,
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;

use anyhow::{anyhow, Context};
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

use crate::{
    cbz_metadata::ComicInfo,
    chapter_storage::ChapterStorage,
    model::{ChapterInformation, MangaInformation},
    source::{model::Page, Source},
};

const CONCURRENT_REQUESTS: usize = 4;

pub async fn ensure_chapter_is_in_storage(
    chapter_storage: &ChapterStorage,
    source: &Source,
    manga: &MangaInformation,
    chapter: &ChapterInformation,
) -> Result<PathBuf, Error> {
    if let Some(path) = chapter_storage.get_stored_chapter(&chapter.id) {
        return Ok(path);
    }

    // FIXME like downloaderror is a really bad name??
    let pages = source
        .get_page_list(
            CancellationToken::new(),
            chapter.id.manga_id().value().clone(),
            chapter.id.value().clone(),
            chapter.chapter_number.unwrap_or_default().to_f64(),
        )
        .await
        .with_context(|| "Failed to get page list")
        .map_err(Error::DownloadError)?;

    if pages.is_empty() {
        return Err(Error::DownloadError(anyhow!(
            "No pages found for chapter {}",
            chapter_id.value()
        )));
    }

    // FIXME this logic should be contained entirely within the storage..? maybe we could return something that's writable
    // and then commit it into the storage (or maybe a implicit commit on drop, but i dont think it works well as there
    // could be errors while committing it)
    let output_path = chapter_storage.get_path_to_store_chapter(&chapter.id);

    let metadata = ComicInfo::from_source_metadata(manga.clone(), chapter.clone(), &pages);

    // Write chapter pages to a temporary file, so that if things go wrong
    // we do not have a borked .cbz file in the chapter storage.
    let temporary_file =
        NamedTempFile::new_in(output_path.parent().unwrap()).map_err(|e| Error::Other(e.into()))?;
    download_chapter_pages_as_cbz(&temporary_file, metadata, source, pages)
        .await
        .with_context(|| "Failed to download chapter pages")
        .map_err(Error::DownloadError)?;

    // If we succeeded downloading all the chapter pages, persist our temporary
    // file into the chapter storage definitively.
    chapter_storage
        .persist_chapter(&chapter.id, temporary_file)
        .with_context(|| {
            format!(
                "Failed to persist chapter {} into storage",
                chapter.id.value()
            )
        })
        .map_err(Error::Other)?;

    Ok(output_path)
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an error occurred while downloading the chapter pages")]
    DownloadError(#[source] anyhow::Error),
    #[error("unknown error")]
    Other(#[from] anyhow::Error),
}

pub async fn download_chapter_pages_as_cbz<W>(
    output: W,
    metadata: ComicInfo,
    source: &Source,
    pages: Vec<Page>,
) -> anyhow::Result<()>
where
    W: Write + Seek,
{
    let mut writer = ZipWriter::new(output);
    let file_options = FileOptions::default().compression_method(CompressionMethod::Stored);

    // Add ComicInfo.xml to the CBZ file
    writer.start_file("ComicInfo.xml", file_options)?;
    let xml_content = metadata.to_xml()?;
    writer.write_all(xml_content.as_bytes())?;

    let client = reqwest::Client::builder()
        // Some sources return invalid certs, but otherwise download images just fine...
        // This is kinda bad.
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    stream::iter(pages)
        .map(|page| {
            let client = &client;

            async move {
                let image_url = page.image_url.ok_or(anyhow!("page has no image URL"))?;
                let extension = Path::new(image_url.path())
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("jpg")
                    .to_owned();

                // FIXME we should left pad this number with zeroes up to the maximum
                // amount of pages needed, but for now we pad 4 digits
                // stop reading the bible if this ever becomes an issue
                let filename = format!("{:0>4}.{}", page.index, extension);

                // TODO we could stream the data from the client into the file
                // would save a bit of memory but i dont think its a big deal
                let request = source.get_image_request(image_url).await?;
                let response_bytes = client
                    .execute(request)
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;

                anyhow::Ok((filename, response_bytes))
            }
        })
        .buffer_unordered(CONCURRENT_REQUESTS)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .try_for_each(|(filename, response_bytes)| {
            writer.start_file(filename, file_options)?;
            writer.write_all(response_bytes.as_ref())?;

            Ok(())
        })
}
