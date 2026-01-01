use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use dom_query::Document;
use futures::{stream, StreamExt, TryStreamExt};
use reqwest::{redirect::Policy, Client, Request};
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Seek, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;
use url::Url;

use anyhow::{anyhow, bail, Context};
use tokio::sync::mpsc;
use zip::{CompressionMethod, ZipWriter};

use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

use crate::{
    cbz_metadata::ComicInfo,
    chapter_storage::ChapterStorage,
    model::{ChapterInformation, MangaInformation},
    source::{model::Page, Source},
    usecases::unscrable_image::{unscrable_image, Block},
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

async fn request_with_forced_referer_from_request(
    client: &Client,
    mut req: Request,
    max_redirects: usize,
) -> Result<reqwest::Response, anyhow::Error> {
    let referer = req
        .headers()
        .get("Referer")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    for _ in 0..max_redirects {
        let req_method = { req.method().clone() };
        let original_headers = { req.headers().clone() };

        let mut last_err: Option<reqwest::Error> = None;
        let mut resp_ok: Option<reqwest::Response> = None;

        for attempt in 1..=3 {
            let cloned_req = req.try_clone().context("Can't clone Request")?;
            match client.execute(cloned_req).await {
                Ok(resp) => {
                    // HTTP status errors are considered *normal*
                    // → do NOT retry
                    resp_ok = Some(resp);
                    break;
                }
                Err(e) => {
                    // Only retry connection-level errors
                    if e.is_status() {
                        // HTTP status error is "normal", no retry
                        return Err(e.into());
                    }

                    last_err = Some(e);

                    if attempt < 3 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            200 * attempt as u64,
                        ))
                        .await;
                    }
                }
            }
        }

        let resp = match resp_ok {
            Some(r) => r,
            None => return Err(last_err.unwrap().into()),
        };

        let status = resp.status();
        if !status.is_redirection() {
            return Ok(resp);
        }

        let loc = resp
            .headers()
            .get("Location")
            .context("redirect missing Location header")?
            .to_str()
            .context("invalid Location header")?;

        let next_url = resp.url().join(loc)?;

        let mut new_req = { client.request(req_method, next_url.clone()).build()? };

        {
            let new_headers = new_req.headers_mut();

            for (k, v) in original_headers.iter() {
                if k.as_str().eq_ignore_ascii_case("referer") {
                    continue; // we will re-add referer below
                }
                new_headers.insert(k, v.clone());
            }

            // If referer existed → keep it permanently
            if let Some(ref r) = referer {
                new_headers.insert("Referer", r.parse().unwrap());
            }
        }

        req = new_req;
    }

    anyhow::bail!("too many redirects")
}

fn generate_error_image(
    status_or_code: &str,
    msg: &str,
    width: u32,
    height: u32,
) -> anyhow::Result<Vec<u8>> {
    use ab_glyph::{FontArc, PxScale};
    use image::{ImageBuffer, Rgba};
    use imageproc::drawing::draw_text_mut;

    let mut img = ImageBuffer::from_pixel(width, height, Rgba([255, 255, 255, 255]));

    let font_data = include_bytes!("../fonts/DejaVuSansMono.ttf") as &[u8];
    let font = FontArc::try_from_vec(font_data.to_vec()).unwrap();

    let title_scale = PxScale { x: 28.0, y: 28.0 };
    let msg_scale = PxScale { x: 20.0, y: 20.0 };

    let title = format!("ERROR {}", status_or_code);

    draw_text_mut(
        &mut img,
        Rgba([0, 0, 0, 255]),
        20,
        20,
        title_scale,
        &font,
        &title,
    );

    let wrapped = wrap_text(msg, 46);

    let mut y = 60;
    for line in wrapped {
        draw_text_mut(
            &mut img,
            Rgba([0, 0, 0, 255]),
            20,
            y,
            msg_scale,
            &font,
            &line,
        );
        y += 26;
    }

    for x in 0..width {
        img.put_pixel(x, 0, Rgba([0, 0, 0, 255]));
        img.put_pixel(x, height - 1, Rgba([0, 0, 0, 255]));
    }
    for y in 0..height {
        img.put_pixel(0, y, Rgba([0, 0, 0, 255]));
        img.put_pixel(width - 1, y, Rgba([0, 0, 0, 255]));
    }

    let mut buf = Vec::new();
    {
        use image::codecs::jpeg::JpegEncoder;
        use image::ColorType;
        use image::DynamicImage;
        use image::ImageEncoder;

        let img = DynamicImage::ImageRgba8(img).to_rgb8();

        // JPEG エンコーダ（Seek 不要）
        let encoder = JpegEncoder::new_with_quality(&mut buf, 100);

        // RGB24 としてエンコード
        encoder
            .write_image(&img, width, height, ColorType::Rgb8.into())
            .context("JPEG encode failed")?;
    }

    Ok(buf)
}

/// Simple line-wrapper
fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > max_chars {
            lines.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
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

async fn prepare_cover(
    cover_url: Option<Url>,
    client: &reqwest::Client,
    source: &Source,
) -> anyhow::Result<Option<Vec<u8>>> {
    if let Some(url) = cover_url {
        let req = source.get_image_request(url, None).await?;
        let resp = client.execute(req).await?.error_for_status()?;
        let bytes = resp.bytes().await?;
        let bytes_vec = bytes.to_vec();

        // Ensure we return JPEG bytes. If already JPEG, return as-is.
        Ok(match image::guess_format(&bytes_vec) {
            Ok(image::ImageFormat::Jpeg) => Some(bytes_vec),
            _ => {
                // Try to decode and re-encode as JPEG (quality 90).
                if let Ok(img) = image::load_from_memory(&bytes_vec) {
                    let mut buf = Vec::new();
                    if img
                        .write_to(
                            &mut std::io::Cursor::new(&mut buf),
                            image::ImageFormat::Jpeg,
                        )
                        .is_ok()
                    {
                        Some(buf)
                    } else {
                        // fallback to original bytes on failure
                        Some(bytes_vec)
                    }
                } else {
                    // fallback: return original bytes
                    Some(bytes_vec)
                }
            }
        })
    } else {
        Ok(None)
    }
}

async fn download_image(
    url: String,
    index: usize,
    source: &Source,
) -> anyhow::Result<(Vec<u8>, String, String)> {
    let Some(url) = &Url::parse(&url).ok() else {
        bail!("Invalid URL: {}", url);
    };

    let bytes_vec = if url.scheme() == "data" {
        // Parse data URI
        let s = url.as_str();
        let Some(comma_idx) = s.find(',') else {
            bail!("base64 data URI missing comma: {}", url);
        };

        let meta = &s[5..comma_idx];
        let data_part = &s[comma_idx + 1..];
        if !meta.contains("base64") {
            bail!("data URI is not base64 encoded: {}", url);
        }

        BASE64.decode(data_part.as_bytes()).map_err(|err| {
            anyhow!(format!(
                "base64 decode failed for page {}: {:?}",
                index, err
            ))
        })?
    } else {
        let request = source
            .get_image_request(url.clone(), None)
            .await
            .map_err(|err| anyhow!(format!("failed WASM modify request {err}")))?;
        let req_url = request.url().clone();

        let client = Client::builder().build()?;
        let response = request_with_forced_referer_from_request(&client, request, 10)
            .await
            .map_err(|err| anyhow!(format!("Request error: {err}")))?;

        response
            .bytes()
            .await
            .map_err(|err| {
                anyhow!(format!(
                    "failed to get bytes for page {} from {}: {:?}",
                    index, req_url, err
                ))
            })?
            .to_vec()
    };

    let (ext, mime) = match image::guess_format(&bytes_vec) {
        Ok(fmt) => {
            let ext = fmt.extensions_str()[0];
            let mime = fmt.to_mime_type().to_string();
            (ext.to_string(), mime)
        }
        Err(_) => {
            let ext = "bin".to_string();
            let mime = "application/octet-stream".to_string();
            (ext, mime)
        }
    };

    Ok((bytes_vec, ext, mime))
}

fn create_xhtml(title: &str, html: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><meta charset="utf-8"/><title>{}</title></head>
<body>{}</body>
</html>"#,
        html_escape::encode_text(&title),
        html
    )
}

pub fn into_html(text: &str) -> String {
    // Regex: match HTML marker at beginning of document
    // (?i)  : case-insensitive
    // ^\s*  : allow leading whitespace
    let html_marker = regex::Regex::new(r"(?i)^\s*<!--\s*html\s*-->").unwrap();

    if html_marker.is_match(text) {
        // Remove the marker and return raw HTML
        html_marker.replace(text, "").to_string()
    } else {
        markdown::to_html(text)
    }
}

fn get_image_src<F>(base_url: Option<&Url>, get: F) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    get("src")
        .or_else(|| get("data-src"))
        .or_else(|| get("data-srcset"))
        .or_else(|| get("data-lazy"))
        .map(|src| {
            base_url
                .and_then(|url| url.join(&src).ok().map(|url| url.to_string()))
                .unwrap_or(src)
        })
}

async fn download_all_images(
    base_url: Option<&Url>,
    pages: Vec<Page>,
    source: &Source,
    token: &CancellationToken,
) -> anyhow::Result<HashMap<String, anyhow::Result<(Vec<u8>, String, String)>>> {
    let mut seen = HashSet::<String>::new();
    type Task = std::pin::Pin<
        Box<
            dyn futures::Future<Output = (String, anyhow::Result<(Vec<u8>, String, String)>)>
                + Send,
        >,
    >;
    let mut tasks: Vec<Task> = Vec::new();

    for page in &pages {
        if token.is_cancelled() {
            break;
        }
        if let Some(image_url) = &page.image_url {
            if seen.insert(image_url.to_string()) {
                let url = image_url.clone();
                let index = page.index;
                let source = source.clone();
                tasks.push(Box::pin(async move {
                    let result = download_image(url.to_string(), index, &source).await;
                    (url.to_string(), result)
                }));
            }
        }

        if let Some(text) = &page.text {
            let html = into_html(text);
            let document = Document::fragment(html);

            for img in document.select("img").iter() {
                if let Some(src) = get_image_src(base_url, |n| img.attr(n).map(|t| t.to_string())) {
                    if seen.insert(src.clone()) {
                        let url = src.clone();
                        let index = page.index;
                        let source = source.clone();
                        tasks.push(Box::pin(async move {
                            let result = download_image(url.clone(), index, &source).await;
                            (url, result)
                        }));
                    }
                }
            }
        }
    }

    let store: HashMap<_, _> = stream::iter(tasks).buffer_unordered(4).collect().await;

    Ok(store)
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

                let xhtml = create_xhtml(&title, &document.html().to_string());

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
