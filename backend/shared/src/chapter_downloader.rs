use futures::{stream, StreamExt, TryStreamExt};
use std::{
    io::{Seek, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;

use anyhow::{anyhow, Context};
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

use crate::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::ChapterId,
    source::{model::Page, Source},
};

const CONCURRENT_REQUESTS: usize = 4;

pub async fn ensure_chapter_is_in_storage(
    database: &Database,
    chapter_storage: &ChapterStorage,
    source: &Source,
    chapter_id: &ChapterId,
    chapter_title: &str,
    chapter_num: Option<f64>,
) -> Result<PathBuf, Error> {
    if let Some(path) = chapter_storage.get_stored_chapter(chapter_id) {
        return Ok(path);
    }

    let manga_information = database
        .find_cached_manga_information(chapter_id.manga_id())
        .await;
    let manga_title = manga_information
        .as_ref()
        .and_then(|info| info.title.clone());

    // FIXME like downloaderror is a really bad name??
    let pages = source
        .get_page_list(
            CancellationToken::new(),
            chapter_id.manga_id().value().clone(),
            chapter_id.value().clone(),
            chapter_num,
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
    let output_path = chapter_storage.get_path_to_store_chapter(chapter_id);

    // Write chapter pages to a temporary file, so that if things go wrong
    // we do not have a borked .cbz file in the chapter storage.
    let temporary_file =
        NamedTempFile::new_in(output_path.parent().unwrap()).map_err(|e| Error::Other(e.into()))?;
    download_chapter_pages_as_cbz(
        &temporary_file,
        source,
        pages,
        manga_title,
        chapter_title,
        chapter_num,
    )
    .await
    .with_context(|| "Failed to download chapter pages")
    .map_err(Error::DownloadError)?;

    // If we succeeded downloading all the chapter pages, persist our temporary
    // file into the chapter storage definitively.
    chapter_storage
        .persist_chapter(chapter_id, temporary_file)
        .with_context(|| {
            format!(
                "Failed to persist chapter {} into storage",
                chapter_id.value()
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
    source: &Source,
    pages: Vec<Page>,
    manga_title: Option<String>,
    chapter_title: &str,
    chapter_number: Option<f64>,
) -> anyhow::Result<()>
where
    W: Write + Seek,
{
    let mut writer = ZipWriter::new(output);
    let client = reqwest::Client::builder()
        // Some sources return invalid certs, but otherwise download images just fine...
        // This is kinda bad.
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let file_options = FileOptions::default().compression_method(CompressionMethod::Stored);
    let mut comic_info_xml = String::from("<ComicInfo>\n");
    let mut title = String::new();

    if let Some(number) = chapter_number {
        title.push_str(&format!("Ch. {}", number));
    }

    if !chapter_title.is_empty() {
        if !title.is_empty() {
            title.push_str(" - ");
        }
        title.push_str(&chapter_title);
    }

    if let Some(series) = manga_title.clone() {
        if !title.is_empty() {
            title.push_str(" - ");
        }
        title.push_str(&series);
    }

    comic_info_xml.push_str(&format!("  <Title>{}</Title>\n", title));

    if let Some(series) = manga_title.clone() {
        comic_info_xml.push_str(&format!("  <Series>{}</Series>\n", series));
    }

    if let Some(number) = chapter_number {
        comic_info_xml.push_str(&format!("  <Number>{}</Number>\n", number));
    }

    if let (Some(number), Some(series)) = (chapter_number, manga_title) {
        comic_info_xml.push_str(&format!(
            "  <Summary>Chapter {} - {}</Summary>\n",
            number, series
        ));
    }

    comic_info_xml.push_str("</ComicInfo>");

    writer.start_file("ComicInfo.xml", file_options)?;
    writer.write_all(comic_info_xml.as_bytes())?;

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
