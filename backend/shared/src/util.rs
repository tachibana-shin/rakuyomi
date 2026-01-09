use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
};

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use dom_query::Document;
use futures::{stream, StreamExt};
use reqwest::{Client, Request};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::source::{model::Page, Source};

pub async fn has_internet_connection() -> bool {
    try_connecting_to_cloudflare().await.is_ok()
}

async fn try_connecting_to_cloudflare() -> Result<()> {
    let addrs = [
        SocketAddr::from(([1, 0, 0, 1], 80)),
        SocketAddr::from(([1, 1, 1, 1], 80)),
    ];

    TcpStream::connect(&addrs[..]).await?;

    Ok(())
}

pub async fn request_with_forced_referer_from_request(
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

pub fn generate_error_image(
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

pub async fn prepare_cover(
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

pub async fn download_image(
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

pub fn create_xhtml(title: &str, html: &str) -> String {
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

pub fn get_image_src<F>(base_url: Option<&Url>, get: F) -> Option<String>
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

pub async fn download_all_images(
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
