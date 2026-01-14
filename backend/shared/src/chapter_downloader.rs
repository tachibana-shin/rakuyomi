use dom_query::Document;
use futures::{stream, StreamExt, TryStreamExt};
use reqwest::{redirect::Policy, Client};
use std::{
    io::{Cursor, Seek, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;

use anyhow::{anyhow, Context};
use tokio::sync::mpsc;
use zip::{CompressionMethod, ZipWriter};

use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

use crate::{
    cbz_metadata::ComicInfo,
    chapter_storage::ChapterStorage,
    model::{ChapterInformation, MangaInformation},
    source::{model::Page, Source},
    usecases::unscrable_image::{unscrable_image, Block},
    util::{
        create_xhtml, download_all_images, generate_error_image, get_image_src, prepare_cover,
        request_with_forced_referer_from_request,
    },
};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct DownloadError {
    pub page_index: usize,
    pub url: String,
    pub reason: String,
    pub attempts: usize,
}

pub async fn ensure_chapter_is_in_storage(
    token: &CancellationToken,
    chapter_storage: &ChapterStorage,
    source: &Source,
    manga: &MangaInformation,
    chapter: &ChapterInformation,
    concurrent_requests_pages: usize,
) -> Result<(PathBuf, Vec<DownloadError>), Error> {
    if let Some(output) = chapter_storage.get_stored_chapter_and_errors(&chapter.id)? {
        return Ok((
            output.0,
            output.1.unwrap_or_else(|| Vec::<DownloadError>::from([])),
        ));
    }

    // FIXME like downloaderror is a really bad name??
    let pages = source
        .get_page_list(
            token.clone(),
            chapter.id.manga_id().value().clone(),
            chapter.id.value().clone(),
            chapter.chapter_number,
        )
        .await
        .with_context(|| "Failed to get page list")
        .map_err(Error::DownloadError)?;

    if pages.is_empty() {
        return Err(Error::DownloadError(anyhow!(
            "No pages found for chapter {}",
            chapter.id.value()
        )));
    }
    let is_novel = pages.first().and_then(|p| p.text.as_ref()).is_some();

    // FIXME this logic should be contained entirely within the storage..? maybe we could return something that's writable
    // and then commit it into the storage (or maybe a implicit commit on drop, but i dont think it works well as there
    // could be errors while committing it)
    let output_path: PathBuf = chapter_storage.get_path_to_store_chapter(&chapter.id, is_novel);

    let metadata = ComicInfo::from_source_metadata(manga.clone(), chapter.clone(), &pages);

    // Write chapter pages to a temporary file, so that if things go wrong
    // we do not have a borked .cbz file in the chapter storage.
    let parent = output_path
        .parent()
        .ok_or_else(|| Error::Other(anyhow::anyhow!("Output path has no parent")))?;
    let temporary_file = NamedTempFile::new_in(parent).map_err(|e| Error::Other(e.into()))?;

    let errors = if is_novel {
        // is novel
        let temp_path = temporary_file.path().to_path_buf();

        download_chapter_novel_as_epub(&temporary_file, token, temp_path, source, pages, chapter)
            .await
            .with_context(|| "Failed to download chapter pages")
            .map_err(Error::DownloadError)?;

        Vec::<DownloadError>::from([])
    } else {
        download_chapter_pages_as_cbz(
            token,
            &temporary_file,
            metadata,
            source,
            pages,
            concurrent_requests_pages,
        )
        .await
        .map_err(|err| {
            eprintln!("Error = {err}");
            err
        })
        .with_context(|| "Failed to download chapter pages")
        .map_err(Error::DownloadError)?
    };

    // If we succeeded downloading all the chapter pages, persist our temporary
    // file into the chapter storage definitively.
    chapter_storage
        .persist_chapter(&chapter.id, is_novel, temporary_file, &errors)
        .await
        .with_context(|| {
            format!(
                "Failed to persist chapter {} into storage",
                chapter.id.value()
            )
        })
        .map_err(Error::Other)?;

    Ok((output_path, errors))
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an error occurred while downloading the chapter pages")]
    DownloadError(#[source] anyhow::Error),
    #[error("unknown error")]
    Other(#[from] anyhow::Error),
}

pub async fn download_chapter_pages_as_cbz<W>(
    cancel_token: &CancellationToken,
    output: W,
    metadata: ComicInfo,
    source: &Source,
    pages: Vec<Page>,
    concurrent_requests_pages: usize,
) -> anyhow::Result<Vec<DownloadError>, anyhow::Error>
where
    W: Write + Seek,
{
    let mut writer = ZipWriter::new(output);
    let file_options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(CompressionMethod::Stored);

    // Add ComicInfo.xml to the CBZ file
    writer.start_file("ComicInfo.xml", file_options)?;
    writer.write_all(metadata.to_xml()?.as_bytes())?;

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .redirect(Policy::none())
        .build()?;

    let (tx, mut rx) = mpsc::channel::<(usize, String, Vec<u8>, Option<DownloadError>)>(
        concurrent_requests_pages * 2,
    );

    let tx_main = tx.clone();
    tokio::spawn({
        let client = client.clone();
        let source = source.clone();
        let cancel_token = cancel_token.clone();

        async move {
            stream::iter(pages)
                .map(|page| {
                    let tx = tx.clone();
                    let client = client.clone();
                    let source = source.clone();
                    let cancel_token = cancel_token.clone();

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
                        let request = source
                            .get_image_request(image_url, page.ctx.clone())
                            .await
                            .map_err(|err| {
                                eprintln!("Failed WASM modify request {err}");
                                err
                            })?;
                        let req_url = request.url().clone();
                        let req_headers = request.headers().clone();
                        let response =
                            request_with_forced_referer_from_request(&client, request, 10)
                                .await
                                .inspect_err(|err| {
                                    eprintln!("Request error: {err}");
                                })?;

                        let (final_bytes, error_info) = {
                            if !response.status().is_success() {
                                let err = DownloadError {
                                    page_index: page.index,
                                    url: req_url.to_string(),
                                    reason: format!("HTTP {}", response.status()),
                                    attempts: 1,
                                };

                                eprintln!("{:?}", err);

                                (
                                    generate_error_image(
                                        &response.status().as_u16().to_string(),
                                        response
                                            .status()
                                            .canonical_reason()
                                            .unwrap_or("Unknown Error"),
                                        500,
                                        667,
                                    )?,
                                    Some(err),
                                )
                            } else {
                                let status = response.status();
                                let headers = response.headers().clone();

                                let response_bytes = response.bytes().await?;

                                let response_bytes = source
                                    .process_page_image(
                                        cancel_token.clone(),
                                        (req_url, req_headers),
                                        (status, headers),
                                        response_bytes,
                                        page.ctx.clone(),
                                    )
                                    .await
                                    .map_err(|err| {
                                        eprintln!("Error = {err}");
                                        err
                                    })?;

                                let final_image = if let Some(blocks_json) = page.base64.as_ref() {
                                    let blocks: Vec<Block> = serde_json::from_str(blocks_json)
                                        .map_err(|e| anyhow!("Invalid blocks JSON: {:?}", e))?;

                                    match unscrable_image(response_bytes.to_vec(), blocks) {
                                        Ok(result) => result,
                                        Err(e) => {
                                            eprintln!("unscrable_image failed: {}", e);
                                            anyhow::bail!(e)
                                        }
                                    }
                                } else {
                                    response_bytes.to_vec()
                                };

                                (final_image, None)
                            }
                        };

                        // Send result
                        let _ = tx
                            .send((page.index, filename, final_bytes, error_info))
                            .await;

                        Ok::<_, anyhow::Error>(())
                    }
                })
                .buffer_unordered(concurrent_requests_pages)
                .try_collect::<Vec<_>>()
                .await
                .ok();

            drop(tx_main);
        }
    });

    // Collect errors
    let mut errors = Vec::<DownloadError>::new();

    // Writer task
    while let Some((_index, filename, data, err)) = rx.recv().await {
        if let Some(e) = err {
            errors.push(e);
        }

        writer.start_file(filename, file_options)?;
        writer.write_all(&data)?;
    }

    Ok(errors)
}

pub async fn download_chapter_novel_as_epub<W>(
    _: W,
    token: &CancellationToken,
    temp_path: std::path::PathBuf,
    source: &Source,
    pages: Vec<Page>,
    chapter: &ChapterInformation,
) -> anyhow::Result<()>
where
    W: Write + Seek,
{
    let client = Client::builder().build()?;

    let cover_url = chapter.thumbnail.clone();
    let lang = chapter.lang.clone();

    let book_name: String = chapter.title.clone().unwrap_or_else(|| {
        if let Some(chapter_number) = chapter.chapter_number {
            format!("Ch.{chapter_number}")
        } else {
            "Unknown Title".to_owned()
        }
    });

    let cover_img = prepare_cover(cover_url, &client, source)
        .await
        .map_err(|e| {
            eprintln!(
                "Failed to prepare cover image for EPUB of book '{}': {:?}",
                book_name, e
            );
        })
        .ok()
        .flatten();

    let images = download_all_images(chapter.url.as_ref(), pages.clone(), source, token).await?;

    let chapter_url = chapter.url.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut output = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)?;

        let mut epub = EpubBuilder::new(ZipLibrary::new()?)?;
        if let Some(lang) = lang {
            epub.set_lang(lang);
        }
        epub.set_title(book_name);

        let mut index_image = 0;
        // epub.inline_toc().set_toc_name("Map");

        if let Some(cursor) = cover_img {
            epub.add_cover_image("cover.jpg", Cursor::new(cursor), "image/jpeg")?;
        }

        for (idx, page) in pages.iter().enumerate() {
            let title = page
                .base64
                .clone()
                .unwrap_or_else(|| format!("Page {}", idx + 1));

            if let Some(image_url) = &page.image_url {
                let Some(image_result) = images.get(&image_url.to_string()) else {
                    continue;
                };
                let html = match image_result {
                    Ok((image_bytes, ext, mime)) => {
                        let filename = format!("images/img_{}.{}", index_image, ext);
                        index_image += 1;

                        epub.add_resource(&filename, Cursor::new(image_bytes), mime)?;

                        format!("<img src=\"../{}\"/>", filename)
                    }
                    Err(e) => {
                        eprintln!("Failed to download image for EPUB: {:?}", e);

                        format!("<p><strong>Failed to download image: {}</strong></p>", e)
                    }
                };
                index_image += 1;

                epub.add_content(
                    EpubContent::new(
                        format!("pages/page_{}.xhtml", idx + 1),
                        Cursor::new(create_xhtml(&title, &html)),
                    )
                    .title(title)
                    .reftype(ReferenceType::Text),
                )?;
            } else if let Some(text) = &page.text {
                let document = Document::fragment(text.to_owned());

                // Apply results sequentially
                for img in document.select("img").iter() {
                    let Some(src) =
                        get_image_src(chapter_url.as_ref(), |n| img.attr(n).map(|v| v.to_string()))
                    else {
                        continue;
                    };
                    let Some(image_result) = images.get(&src) else {
                        continue;
                    };
                    match image_result {
                        Ok((image_bytes, ext, mime)) => {
                            let filename = format!("images/img_{}.{}", index_image, ext);
                            index_image += 1;

                            epub.add_resource(&filename, Cursor::new(image_bytes), mime)?;

                            img.set_attr("src", &format!("../{}", filename));
                        }
                        Err(e) => {
                            eprintln!("Failed to download image for EPUB: {:?}", e);

                            let image_bytes =
                                generate_error_image("Image error", &e.to_string(), 500, 667)?;

                            let filename = format!("images/img_{}.{}", index_image, "jpeg");
                            index_image += 1;

                            epub.add_resource(&filename, Cursor::new(image_bytes), "image/jpeg")?;

                            img.set_attr("src", &format!("../{}", filename));
                        }
                    }
                }

                let xhtml = create_xhtml(&title, document.html().as_ref());

                epub.add_content(
                    EpubContent::new(format!("pages/page_{}.xhtml", idx + 1), Cursor::new(xhtml))
                        .title(title)
                        .reftype(ReferenceType::Text),
                )?;
            } else {
                let html =
                    "<p><strong>No content available for this page.</strong></p>".to_string();
                epub.add_content(
                    EpubContent::new(
                        format!("pages/page_{}.xhtml", idx + 1),
                        Cursor::new(create_xhtml(&title, &html)),
                    )
                    .title(title)
                    .reftype(ReferenceType::Text),
                )?;
            }
        }

        epub.generate(&mut output)?;

        Ok(())
    })
    .await??;

    Ok(())
}
