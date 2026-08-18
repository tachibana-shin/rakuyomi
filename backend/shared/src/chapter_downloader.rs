use dom_query::Document;
use futures::{stream, StreamExt};
use reqwest::redirect::Policy;
use std::{
    io::{Cursor, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tempfile::NamedTempFile;
use tokio_util::bytes::Bytes;
use tokio_util::sync::CancellationToken;

use anyhow::{anyhow, Context};
use tokio::sync::mpsc;
use zip::{CompressionMethod, ZipWriter};

use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

use crate::{
    cbz_metadata::ComicInfo,
    chapter_storage::ChapterStorage,
    model::{ChapterId, ChapterInformation, MangaInformation},
    settings::ChapterTitleFormat,
    source::{model::Page, Source},
    unscrable_image::{unscrable_image, Block},
    util::{
        create_xhtml, detect_image_extension, download_all_images, generate_error_image,
        get_image_src, into_html, prepare_cover, request_with_forced_referer_from_request,
    },
};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct DownloadError {
    pub page_index: usize,
    pub url: String,
    pub reason: String,
    pub attempts: usize,
}

#[allow(clippy::too_many_arguments)]
pub async fn ensure_chapter_is_in_storage(
    token: &CancellationToken,
    chapter_storage: &ChapterStorage,
    source: &Source,
    manga: &MangaInformation,
    chapter: &ChapterInformation,
    concurrent_requests_pages: usize,
    optimize_image: bool,
    on_progress: Option<Arc<dyn Fn(f32, f32) + Send + Sync>>,
    use_ram: bool,
    current_chapter_id: Option<&ChapterId>,
    chapter_title_format: ChapterTitleFormat,
) -> Result<(PathBuf, Vec<DownloadError>), Error> {
    if use_ram {
        if let Some(output) = chapter_storage.get_stored_chapter_and_errors(&chapter.id, true)? {
            return Ok((output.0, output.1.unwrap_or_default()));
        }
    }
    if let Some(output) = chapter_storage.get_stored_chapter_and_errors(&chapter.id, false)? {
        return Ok((output.0, output.1.unwrap_or_default()));
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
    let output_path: PathBuf =
        chapter_storage.get_path_to_store_chapter(&chapter.id, is_novel, use_ram);

    let metadata = ComicInfo::from_source_metadata(
        manga.clone(),
        chapter.clone(),
        &pages,
        chapter_title_format,
    );

    // Write chapter pages to a temporary file, so that if things go wrong
    // we do not have a borked .cbz file in the chapter storage.
    let parent = output_path
        .parent()
        .ok_or_else(|| Error::Other(anyhow::anyhow!("Output path has no parent")))?;
    let temporary_file = NamedTempFile::new_in(parent).map_err(|e| Error::Other(e.into()))?;

    // in mode write to RAM before download to free memory
    if use_ram {
        if let Some(current_chapter_id) = current_chapter_id {
            let _ = chapter_storage
                .evict_tmpfs_older_than_current(current_chapter_id, is_novel)
                .await;
        }
    }
    let errors = if is_novel {
        // is novel
        let temp_path = temporary_file.path().to_path_buf();

        download_chapter_novel_as_epub(
            &temporary_file,
            token,
            temp_path,
            source,
            pages,
            chapter,
            concurrent_requests_pages,
            on_progress.clone(),
        )
        .await
        .with_context(|| "Failed to download chapter pages")
        .map_err(Error::DownloadError)?;

        Vec::<DownloadError>::new()
    } else {
        download_chapter_pages_as_cbz(
            token,
            &temporary_file,
            metadata,
            source,
            pages,
            concurrent_requests_pages,
            optimize_image,
            on_progress.clone(),
            &chapter.id,
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
        .persist_chapter(&chapter.id, is_novel, temporary_file, &errors, use_ram)
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

fn zip_comment(chapter_id: &ChapterId) -> String {
    serde_json::json!({
        "source_id": chapter_id.source_id().value(),
        "manga_id": chapter_id.manga_id().value(),
        "chapter_id": chapter_id.value(),
    })
    .to_string()
}

#[allow(clippy::too_many_arguments)]
pub async fn download_chapter_pages_as_cbz<W>(
    cancel_token: &CancellationToken,
    output: W,
    metadata: ComicInfo,
    source: &Source,
    pages: Vec<Page>,
    concurrent_requests_pages: usize,
    optimize_image: bool,
    on_progress: Option<Arc<dyn Fn(f32, f32) + Send + Sync>>,
    chapter_id: &ChapterId,
) -> anyhow::Result<Vec<DownloadError>, anyhow::Error>
where
    W: Write + Seek,
{
    let total = pages.len() as f32;
    let mut processed = 0f32;

    let mut writer = ZipWriter::new(output);
    let file_options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(CompressionMethod::Stored);

    // Add ComicInfo.xml to the CBZ file
    writer.start_file("ComicInfo.xml", file_options)?;
    writer.write_all(metadata.to_xml()?.as_bytes())?;

    let client = crate::tls::client_builder()
        .timeout(Duration::from_secs(30))
        .redirect(Policy::none())
        .build()?;

    let (tx, mut rx) = mpsc::channel::<(usize, String, Vec<u8>, Option<DownloadError>)>(
        concurrent_requests_pages * 2,
    );

    let tx_main = tx.clone();
    let chapter_id_str = chapter_id.value().clone();
    tokio::spawn({
        let client = client.clone();
        let source = source.clone();
        let cancel_token = cancel_token.clone();

        async move {
            // Zero-pad page indices to the width of the page count, so
            // lexicographic filename ordering matches page order.
            let page_index_width = pages.len().to_string().len().max(2);

            stream::iter(pages)
                .map(|page| {
                    let tx = tx.clone();
                    let client = client.clone();
                    let source = source.clone();
                    let cancel_token = cancel_token.clone();
                    let chapter_id_str = chapter_id_str.clone();

                    async move {
                        let page_index = page.index;
                        let page_url = page.image_url.clone();

                        match async {
                            let image_url = page.image_url.ok_or(anyhow!("page has no image URL"))?;
                            let extension = Path::new(image_url.path())
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .unwrap_or("jpg")
                                .to_owned();

                            // Fallback filename (URL-derived extension): the
                            // success arm may override it with the real
                            // extension sniffed from the image magic bytes.
                            let filename =
                                format!("{:0>page_index_width$}.{}", page.index, extension);

                            // TODO we could stream the data from the client into the file
                            // would save a bit of memory but i dont think its a big deal
                            let response_bytes = if matches!(
                                &source.backend,
                                crate::source::SourceBackend::Keiyoushi(_)
                            ) {
                                // keiyoushi images may be IMGX-encrypted: fetch through the
                                // extension's own client so its interceptor decrypts them.
                                let bytes = source
                                    .fetch_page_image(&chapter_id_str, image_url.as_str())
                                    .await
                                    .map_err(|err| {
                                        eprintln!("Failed keiyoushi image fetch {err}");
                                        anyhow::anyhow!(err)
                                    })?;

                                // The extension's client swallows HTTP statuses
                                // (only its interceptor sees them), so a failed
                                // request or decrypt can still come back as
                                // bytes; render the same error image as the
                                // plain GET path when they are not an image.
                                if detect_image_extension(&bytes).is_none() {
                                    let head = String::from_utf8_lossy(
                                        &bytes[..bytes.len().min(16)],
                                    )
                                    .to_string();
                                    let reason = if bytes.starts_with(b"IMGX") {
                                        "image is still IMGX-encrypted".to_string()
                                    } else {
                                        format!("invalid image data: {head:?}")
                                    };
                                    let err = DownloadError {
                                        page_index: page.index,
                                        url: image_url.to_string(),
                                        reason: reason.clone(),
                                        attempts: 1,
                                    };
                                    eprintln!("{:?}", err);
                                    return Ok((
                                        page.index,
                                        format!("{:0>page_index_width$}.jpg", page.index),
                                        generate_error_image("Error", &reason, 500, 667)?,
                                        Some(err),
                                    ));
                                }

                                Bytes::from(bytes)
                            } else {
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

                                if !response.status().is_success() {
                                    let err = DownloadError {
                                        page_index: page.index,
                                        url: req_url.to_string(),
                                        reason: format!("HTTP {}", response.status()),
                                        attempts: 1,
                                    };

                                    eprintln!("{:?}", err);

                                    return Ok((
                                        page.index,
                                        format!("{:0>page_index_width$}.{}", page.index, extension),
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
                                    ));
                                }

                                let status = response.status();
                                let headers = response.headers().clone();

                                let response_bytes = response.bytes().await?;

                                if source.features.process_page_image {
                                    Bytes::from(
                                        source
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
                                            })?,
                                    )
                                } else {
                                    response_bytes
                                }
                            };

                            let (final_bytes, error_info) =
                                if source.features.process_page_image {
                                    (response_bytes.to_vec(), None)
                                } else if optimize_image {
                                        (
                                        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
                                            let data = response_bytes.to_vec();
                                            if let Some(image) =
                                                crate::source::decode_image::decode_image_fast(&data)
                                            {
                                                if let Ok(image) = image.map_err(|err| {
                                                    eprintln!("failed to load image with faster {err}")
                                                }) {
                                                    match crate::source::decode_image::decode_argb_to_rgb(
                                                        image.width, image.height, &image.data,
                                                    ) {
                                                        Ok(rgb_pixels) => {
                                                            let mut comp = mozjpeg::Compress::new(
                                                                mozjpeg::ColorSpace::JCS_RGB,
                                                            );
                                                            comp.set_size(
                                                                image.width as usize,
                                                                image.height as usize,
                                                            );
                                                            comp.set_fastest_defaults();

                                                            let mut comp = comp.start_compress(Vec::new())?;
                                                            comp.write_scanlines(&rgb_pixels)?;

                                                            Ok(comp.finish()?)
                                                        }
                                                        Err(e) => {
                                                            eprintln!("failed to convert ARGB to RGB: {e}");
                                                            Ok(data)
                                                        }
                                                    }
                                                } else {
                                                    Ok(data)
                                                }
                                            } else {
                                                Ok(data)
                                            }
                                        })
                                        .await??,
                                        None,
                                    )
                                    } else {
                                        (response_bytes.to_vec(), None)
                                    };

                            let final_bytes = if let Some(blocks_json) = page.base64.as_ref() {
                                let blocks: Vec<Block> = serde_json::from_str(blocks_json)
                                    .map_err(|e| anyhow!("Invalid blocks JSON: {:?}", e))?;

                                tokio::task::spawn_blocking(move || {
                                    match unscrable_image(final_bytes, blocks) {
                                        Ok(result) => Ok(result),
                                        Err(e) => {
                                            eprintln!("unscrable_image failed: {}", e);
                                            anyhow::bail!(e)
                                        }
                                    }
                                })
                                .await??
                            } else {
                                final_bytes
                            };

                            Ok::<_, anyhow::Error>((page.index, filename, final_bytes, error_info))
                        }
                        .await
                        {
                            Ok((index, filename, final_bytes, error_info)) => {
                                // Keiyoushi image URLs may end in a disguised
                                // extension (e.g. .js for IMGX-encrypted WebP):
                                // trust the magic bytes of the decrypted image
                                // over the extension derived from the URL.
                                let filename = match detect_image_extension(&final_bytes) {
                                    Some(ext) => format!("{:0>page_index_width$}.{}", index, ext),
                                    None => filename,
                                };
                                // Send result
                                let _ = tx
                                    .send((index, filename, final_bytes, error_info))
                                    .await;
                            }
                            Err(e) => {
                                eprintln!("Error downloading page {}: {e}", page_index);
                                let filename = format!("{:0>page_index_width$}.jpg", page_index);
                                let bytes = generate_error_image("Error", &e.to_string(), 500, 667)
                                    .unwrap_or_default();
                                let _ = tx
                                    .send((
                                        page_index,
                                        filename,
                                        bytes,
                                        Some(DownloadError {
                                            page_index,
                                            url: page_url
                                                .map(|u| u.to_string())
                                                .unwrap_or_default(),
                                            reason: e.to_string(),
                                            attempts: 1,
                                        }),
                                    ))
                                    .await;
                            }
                        }
                    }
                })
                .buffer_unordered(concurrent_requests_pages)
                .collect::<Vec<()>>()
                .await;

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
        processed += 1.0;
        if let Some(ref cb) = on_progress {
            cb(processed, total);
        }
        writer.write_all(&data)?;
    }

    let _ = writer.set_comment(zip_comment(chapter_id));
    Ok(errors)
}

#[allow(clippy::too_many_arguments)]
pub async fn download_chapter_novel_as_epub<W>(
    _: W,
    token: &CancellationToken,
    temp_path: std::path::PathBuf,
    source: &Source,
    pages: Vec<Page>,
    chapter: &ChapterInformation,
    concurrent_requests_pages: usize,
    on_progress: Option<Arc<dyn Fn(f32, f32) + Send + Sync>>,
) -> anyhow::Result<()>
where
    W: Write + Seek,
{
    let total = pages.len() as f32;
    let stored_process_images = std::sync::Arc::new(std::sync::Mutex::new(
        std::collections::HashMap::<usize, f32>::new(),
    ));
    let stored_process_images_clone = stored_process_images.clone();

    let client = crate::tls::client_builder()
        .timeout(Duration::from_secs(30))
        .build()?;

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

    let images = download_all_images(
        chapter.url.as_ref(),
        &pages,
        source,
        token,
        concurrent_requests_pages,
        #[cfg(not(feature = "all"))]
        move |idx, done, total| {
            let percent = done / total;
            let progress = percent * 0.8;

            if let Ok(mut stored) = stored_process_images.lock() {
                stored.insert(idx, progress);
                if let Some(ref cb) = on_progress {
                    cb(stored.values().copied().sum(), total);
                }
            }
        },
    )
    .await?;

    let chapter_url = chapter.url.clone();
    let comment = zip_comment(&chapter.id);
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
                        index_image += 1;

                        format!("<p><strong>Failed to download image: {}</strong></p>", e)
                    }
                };

                if let Ok(mut stored) = stored_process_images_clone.lock() {
                    stored.insert(idx, 1.0);
                    if let Some(ref cb) = on_progress {
                        cb(stored.values().copied().sum(), total);
                    }
                }

                epub.add_content(
                    EpubContent::new(
                        format!("pages/page_{}.xhtml", idx + 1),
                        Cursor::new(create_xhtml(&title, &html)),
                    )
                    .title(title)
                    .reftype(ReferenceType::Text),
                )?;
            } else if let Some(text) = &page.text {
                let document = Document::from(format!(
                    "<html><body>{}</body></html>",
                    into_html(text).to_owned()
                ));

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

                let xhtml = create_xhtml(&title, document.select_single("body").html().as_ref());

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

        epub.set_zip_comment(&comment);
        epub.generate(&mut output)?;

        Ok(())
    })
    .await??;

    Ok(())
}
