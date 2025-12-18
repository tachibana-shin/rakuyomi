use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures::{stream, StreamExt, TryStreamExt};
use kuchiki::{parse_html, traits::TendrilSink, NodeRef};
use reqwest::{header::HeaderMap, redirect::Policy, Client, Request};
use scraper::{Html, Selector};
use std::{
    collections::HashMap,
    io::{Cursor, Seek, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;
use url::Url;

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
};

use rust_decimal::prelude::ToPrimitive;

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
            chapter.chapter_number.unwrap_or_default().to_f64(),
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
    let is_novel = pages.get(0).and_then(|p| p.text.as_ref()).is_some();

    // FIXME this logic should be contained entirely within the storage..? maybe we could return something that's writable
    // and then commit it into the storage (or maybe a implicit commit on drop, but i dont think it works well as there
    // could be errors while committing it)
    let output_path: PathBuf = chapter_storage.get_path_to_store_chapter(&chapter.id, is_novel);

    let metadata = ComicInfo::from_source_metadata(manga.clone(), chapter.clone(), &pages);

    // Write chapter pages to a temporary file, so that if things go wrong
    // we do not have a borked .cbz file in the chapter storage.
    let temporary_file =
        NamedTempFile::new_in(output_path.parent().unwrap()).map_err(|e| Error::Other(e.into()))?;

    let errors = if is_novel {
        // is novel
        let temp_path = temporary_file.path().to_path_buf();
        let book_name: String = pages[0].base64.clone().unwrap_or("Unknown".to_string());
        let cover_url = pages[0].image_url.clone();
        let p_pages = if matches!(pages[0].text.as_deref(), Some("novel")) {
            pages.to_vec()[1..].to_owned()
        } else {
            pages
        };

        download_chapter_novel_as_epub(
            &temporary_file,
            temp_path,
            source,
            p_pages,
            book_name,
            cover_url,
        )
        .await
        .with_context(|| "Failed to download chapter pages")
        .map_err(Error::DownloadError)?;

        Vec::<DownloadError>::from([])
    } else {
        download_chapter_pages_as_cbz(
            &token,
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
    let xml_content = metadata.to_xml()?;
    writer.write_all(xml_content.as_bytes())?;

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .redirect(Policy::none())
        .build()
        .unwrap();

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
                        let req_url = { request.url().clone() };
                        let req_headers = { request.headers().clone() };
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
                                    url: req_url.clone().to_string(),
                                    reason: format!("HTTP {}", response.status()),
                                    attempts: 1,
                                };

                                eprintln!("{:?}", err);

                                (
                                    generate_error_image(
                                        &response.status().as_u16().to_string(),
                                        &response
                                            .status()
                                            .canonical_reason()
                                            .unwrap_or("Unknown Error"),
                                        500,
                                        667,
                                    )?,
                                    Some(err),
                                )
                            } else {
                                let status = { response.status() };
                                let headers = { response.headers().clone() };

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
// assume Page.text: Option<String>, Page.base64: Option<String>, Page.url: String
pub async fn extract_image_urls(pages: &[Page]) -> anyhow::Result<Vec<(usize, String)>> {
    // Return Vec of (page_index, src_string)
    let mut urls = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let html_content = page.text.as_ref().context("Expected text in novel page")?;
        let doc = Html::parse_fragment(html_content);
        let selector = Selector::parse("img").unwrap();
        for img in doc.select(&selector) {
            if let Some(src) = img.value().attr("src") {
                urls.push((i, src.to_string()));
            }
        }
    }
    Ok(urls)
}

async fn prepare_cover_cursor(
    cover_url: Option<Url>,
    client: &reqwest::Client,
    source: &Source,
) -> Option<Cursor<Vec<u8>>> {
    if let Some(url) = cover_url {
        let req = source.get_image_request(url, None).await.ok()?;
        let resp = client.execute(req).await.ok()?.error_for_status().ok()?;
        let bytes = resp.bytes().await.ok()?;
        Some(Cursor::new(bytes.to_vec()))
    } else {
        None
    }
}

fn replace_a_with_span(document: &NodeRef) {
    for a_node in document.select("a").unwrap().collect::<Vec<_>>() {
        let node = a_node.as_node();

        let span_node = NodeRef::new_element(
            markup5ever::QualName::new(
                None,
                markup5ever::Namespace::from("http://www.w3.org/1999/xhtml"),
                markup5ever::LocalName::from("span"),
            ),
            None,
        );
        for child in node.children() {
            span_node.append(child);
        }

        node.insert_after(span_node);
        node.detach();
    }
}

pub async fn download_chapter_novel_as_epub<W>(
    _: W,
    temp_path: std::path::PathBuf,
    source: &Source,
    mut pages: Vec<Page>,
    book_name: String,
    cover_url: Option<Url>,
) -> anyhow::Result<()>
where
    W: Write + Seek,
{
    let client = Client::builder().build().unwrap();

    let image_refs = extract_image_urls(&pages).await?;

    let mut abs_to_filename: HashMap<String, String> = HashMap::new();
    let mut filename_to_bytes: HashMap<String, Vec<u8>> = HashMap::new();
    let mut page_repls: HashMap<usize, Vec<(String, String)>> = HashMap::new();
    let mut file_counter: usize = 0;

    for (page_index, orig_src) in image_refs.into_iter() {
        let abs_opt: Option<Url> = match Url::parse(&orig_src) {
            Ok(u) => Some(u),
            Err(_) => None,
        };

        let abs = match abs_opt {
            Some(u) => u,
            None => continue,
        };

        let abs_str = abs.as_str().to_string();

        if let Some(fname) = abs_to_filename.get(&abs_str) {
            page_repls
                .entry(page_index)
                .or_default()
                .push((orig_src.clone(), fname.clone()));
            continue;
        }

        let ext = abs
            .path_segments()
            .and_then(|s| s.last())
            .and_then(|name| name.split('.').last())
            .unwrap_or("jpg");
        let filename = format!("images/img_{}.{}", file_counter, ext);
        file_counter += 1;

        let bytes_vec: Option<Vec<u8>> = if abs.scheme() == "data" {
            // Parse data URI
            let s = abs.as_str();
            if let Some(comma_idx) = s.find(',') {
                let meta = &s[5..comma_idx];
                let data_part = &s[comma_idx + 1..];
                if meta.contains("base64") {
                    match BASE64.decode(data_part.as_bytes()) {
                        Ok(b) => Some(b),
                        Err(e) => {
                            eprintln!(
                                "[WARN] Base64 decode failed for page {}: {:?}",
                                page_index, e
                            );
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            // HTTP fetch with error handling

            let mut headers = HeaderMap::new();
            headers  .insert("User-Agent", 
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:107.0) Gecko/20100101 Firefox/107.0".parse().unwrap());

            headers.insert("Referer", "https://docln.net/".parse().unwrap());

            match client.get(abs.clone()).headers(headers).send().await {
                Ok(resp) => match resp.error_for_status() {
                    Ok(ok_resp) => match ok_resp.bytes().await {
                        Ok(b) => Some(b.to_vec()),
                        Err(e) => {
                            eprintln!("[WARN] Read bytes failed for {}: {:?}", abs, e);
                            None
                        }
                    },
                    Err(e) => {
                        eprintln!("[WARN] HTTP status error for {}: {:?}", abs, e);
                        None
                    }
                },
                Err(e) => {
                    eprintln!("[WARN] Request execution failed for {}: {:?}", abs, e);
                    None
                }
            }
        };

        if let Some(bvec) = bytes_vec {
            abs_to_filename.insert(abs_str.clone(), filename.clone());
            filename_to_bytes.insert(filename.clone(), bvec);
            page_repls
                .entry(page_index)
                .or_default()
                .push((orig_src.clone(), filename.clone()));
        } else {
            // failed download
            continue;
        }
    }

    let filename_to_bytes = std::mem::take(&mut filename_to_bytes);
    let pages_for_epub = std::mem::take(&mut pages);

    let cursor_cover_img = prepare_cover_cursor(cover_url, &client, source).await;

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut output = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)?;

        let mut epub = EpubBuilder::new(ZipLibrary::new()?)?;
        // epub.set_lang("vi");
        epub.set_title(book_name);
        epub.inline_toc().set_toc_name("Map");

        if let Some(cursor) = cursor_cover_img {
            epub.add_cover_image("cover.jpg", cursor, "image/jpeg")?;
        }

        for (filename, bytes) in filename_to_bytes {
            let mime = if filename.ends_with(".png") {
                "image/png"
            } else if filename.ends_with(".gif") {
                "image/gif"
            } else {
                "image/jpeg"
            };
            epub.add_resource(&filename, Cursor::new(bytes), mime)?;
        }

        for (idx, page) in pages_for_epub.into_iter().enumerate() {
            let title = page
                .base64
                .clone()
                .unwrap_or_else(|| format!("Chapter {}", idx + 1));

            let document = parse_html().one(page.text.unwrap_or_default().clone());
            if let Some(rpls) = page_repls.get(&idx) {
                for css_match in document.select("img").unwrap() {
                    let mut attrs = css_match.attributes.borrow_mut();
                    if let Some(src) = attrs.get_mut("src") {
                        if let Some((_, fname)) = rpls.iter().find(|(orig, _)| orig == src) {
                            *src = format!("../{}", fname.clone());
                        }
                    }
                }
            }

            replace_a_with_span(&document);

            let mut buffer = Vec::new();
            document.serialize(&mut buffer).unwrap();

            let xhtml = format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><meta charset="utf-8"/><title>{}</title></head>
<body>{}</body>
</html>"#,
                html_escape::encode_text(&title),
                String::from_utf8(buffer).unwrap()
            );

            epub.add_content(
                EpubContent::new(
                    format!("chapters/chapter_{}.xhtml", idx + 1),
                    Cursor::new(xhtml),
                )
                .title(title)
                .reftype(ReferenceType::Text),
            )?;
        }

        epub.generate(&mut output)?;
        Ok(())
    })
    .await??;

    Ok(())
}
