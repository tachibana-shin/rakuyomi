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

pub async fn ensure_chapter_is_in_storage(
    token: &CancellationToken,
    chapter_storage: &ChapterStorage,
    source: &Source,
    manga: &MangaInformation,
    chapter: &ChapterInformation,
    concurrent_requests_pages: usize,
) -> Result<PathBuf, Error> {
    if let Some(path) = chapter_storage.get_stored_chapter(&chapter.id) {
        return Ok(path);
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

    if is_novel {
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
    } else {
        download_chapter_pages_as_cbz(
            &token.clone(),
            &temporary_file,
            metadata,
            source,
            pages,
            concurrent_requests_pages,
        )
        .await
        .map_err(|err| {
            println!("Error = {err}");
            err
        })
        .with_context(|| "Failed to download chapter pages")
        .map_err(Error::DownloadError)?;
    }

    // If we succeeded downloading all the chapter pages, persist our temporary
    // file into the chapter storage definitively.
    chapter_storage
        .persist_chapter(&chapter.id, is_novel, temporary_file)
        .await
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

        let resp = client.execute(req).await.context("request failed")?;

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

pub async fn download_chapter_pages_as_cbz<W>(
    cancel_token: &CancellationToken,
    output: W,
    metadata: ComicInfo,
    source: &Source,
    pages: Vec<Page>,
    concurrent_requests_pages: usize,
) -> anyhow::Result<(), anyhow::Error>
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

    let (tx, mut rx) = mpsc::channel::<(usize, String, Vec<u8>)>(concurrent_requests_pages * 2);

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
                                println!("Failed WASM modify request {err}");
                                err
                            })?;
                        let req_url = { request.url().clone() };
                        let req_headers = { request.headers().clone() };
                        let response =
                            request_with_forced_referer_from_request(&client, request, 10)
                                .await
                                .inspect_err(|err| {
                                    eprintln!("Request error: {err}");
                                })?
                                .error_for_status()
                                .inspect_err(|err| {
                                    eprintln!("Request error: {err}");
                                })?;
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
                                println!("Error = {err}");
                                err
                            })?;

                        let final_image = if let Some(blocks_json) = page.base64.as_ref() {
                            let blocks: Vec<Block> = serde_json::from_str(blocks_json)
                                .map_err(|e| anyhow!("Invalid blocks JSON: {:?}", e))?;

                            match unscrable_image(response_bytes.to_vec(), blocks) {
                                Ok(result) => result,
                                Err(e) => {
                                    println!("unscrable_image failed: {}", e);
                                    anyhow::bail!(e)
                                }
                            }
                        } else {
                            response_bytes.to_vec()
                        };

                        tx.send((page.index, filename, final_image)).await.ok();
                        Ok::<_, anyhow::Error>(())
                    }
                })
                .buffer_unordered(concurrent_requests_pages)
                .try_collect::<Vec<_>>() // chỉ để propagate lỗi
                .await
                .ok();

            drop(tx_main);
        }
    });

    // Writer task — sequential zip writing
    while let Some((_index, filename, data)) = rx.recv().await {
        writer.start_file(filename.clone(), file_options)?;
        writer.write_all(&data)?;
    }

    Ok(())
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
