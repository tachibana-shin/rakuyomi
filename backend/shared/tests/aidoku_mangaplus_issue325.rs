//! Live regression test for issue #325 ("MangaPlus blank pages").
//!
//! Downloads the `multi.mangaplus` WASM source from the Aidoku community
//! index and runs the full home -> chapter list -> page list pipeline to
//! verify that `get_page_list` returns pages with valid image URLs.
//!
//! Network-dependent: gated behind `#[ignore]` so CI stays offline; run with:
//!
//! ```sh
//! cargo test -p shared --test aidoku_mangaplus_issue325 -- --ignored --nocapture
//! ```

use std::{path::PathBuf, sync::Arc};

use shared::{
    model::SourceId, settings::Settings, source::SourceBackend, source_manager::SourceManager, tls,
    util::request_with_forced_referer_from_request,
};
use tokio_util::sync::CancellationToken;

const INDEX_URL: &str = "https://aidoku-community.github.io/sources/index.min.json";
const SOURCE_ID: &str = "multi.mangaplus";

fn http() -> reqwest::Client {
    tls::client_builder().build().unwrap()
}

async fn download_aix(dir: &PathBuf) -> PathBuf {
    let client = http();
    let bytes = client
        .get(INDEX_URL)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let index: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let sources = index["sources"].as_array().unwrap();
    let entry = sources
        .iter()
        .find(|s| s["id"].as_str() == Some(SOURCE_ID))
        .unwrap_or_else(|| panic!("{SOURCE_ID} not found in index"));

    let base = INDEX_URL
        .rsplit_once('/')
        .map(|(base, _)| base)
        .unwrap_or(INDEX_URL);
    let download_url = format!("{}/{}", base, entry["downloadURL"].as_str().unwrap());
    eprintln!("downloading {SOURCE_ID} from {download_url}");

    let bytes = client
        .get(&download_url)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    eprintln!("downloaded {} bytes", bytes.len());

    let file = dir.join(format!("{SOURCE_ID}.aix"));
    std::fs::write(&file, &bytes).unwrap();
    file
}

#[tokio::test]
#[ignore = "live network test; run with --ignored"]
async fn mangaplus_page_list_issue325() {
    let dir = std::env::temp_dir().join("rakuyomi-issue325-mangaplus-v4");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Download the .aix file.
    let file = download_aix(&dir).await;

    // 2. Install the source.
    let mut manager = SourceManager::from_folder(dir, Settings::default()).unwrap();
    let arc_manager = Arc::new(tokio::sync::Mutex::new(manager.clone()));
    manager
        .install_source(
            &SourceId::new(SOURCE_ID.to_string()),
            std::fs::read(&file).unwrap(),
            "issue325-test".into(),
            &arc_manager,
        )
        .unwrap();

    let source = manager
        .sources_by_id
        .get(&SourceId::new(SOURCE_ID.to_string()))
        .unwrap()
        .clone();

    // 3. Get manga list via home/popular (triggers ensure_booted + SDK detection).
    let token = CancellationToken::new();
    let mangas = tokio::task::spawn_blocking({
        let source = source.clone();
        move || {
            let mut backend = match &source.backend {
                SourceBackend::Aidoku(s) => s.lock().unwrap(),
                _ => panic!("not an aidoku source"),
            };
            eprintln!("next_sdk={}", backend.next_sdk);
            let result = backend.get_manga_list(
                token,
                aidoku::Listing {
                    id: "Updates".into(),
                    name: "Updates".into(),
                    kind: aidoku::ListingKind::default(),
                },
            );
            match &result {
                Ok(m) => eprintln!("home: {} results", m.len()),
                Err(e) => eprintln!("home ERR: {e:#}"),
            }
            result
        }
    })
    .await
    .unwrap()
    .expect("get_manga_list failed");

    assert!(!mangas.is_empty(), "no manga found");
    let manga = &mangas[0];
    eprintln!("manga: id={} title={:?}", manga.id, manga.title);

    // 3b. Also test search (was returning 0 results earlier).
    let token = CancellationToken::new();
    let search_result = tokio::task::spawn_blocking({
        let source = source.clone();
        move || {
            let mut backend = match &source.backend {
                SourceBackend::Aidoku(s) => s.lock().unwrap(),
                _ => panic!("not an aidoku source"),
            };
            let result = backend.search_mangas(token, "one piece".to_string(), 1);
            match &result {
                Ok((m, h)) => eprintln!("search: {} results, has_next={}", m.len(), h),
                Err(e) => eprintln!("search ERR: {e:#}"),
            }
            result
        }
    })
    .await
    .unwrap()
    .expect("search failed");

    let (search_mangas, _) = search_result;
    if search_mangas.is_empty() {
        eprintln!("WARNING: search returned 0 results (may be a protobuf parsing issue in get_search_manga_list)");
    } else {
        eprintln!("search OK: {} results", search_mangas.len());
    }

    // 4. Get chapter list.
    let manga_id = manga.id.clone();
    let chapters = tokio::task::spawn_blocking({
        let source = source.clone();
        move || {
            let mut backend = match &source.backend {
                SourceBackend::Aidoku(s) => s.lock().unwrap(),
                _ => panic!("not an aidoku source"),
            };
            let token = CancellationToken::new();
            let result = backend.get_chapter_list(token, manga_id);
            match &result {
                Ok(c) => eprintln!("chapters: {} total", c.len()),
                Err(e) => eprintln!("chapters ERR: {e:#}"),
            }
            result
        }
    })
    .await
    .unwrap()
    .expect("get_chapter_list failed");

    assert!(!chapters.is_empty(), "chapter list is empty");
    let chapter = &chapters[0];
    eprintln!(
        "chapter: id={} num={:?} title={:?}",
        chapter.id, chapter.chapter_num, chapter.title
    );

    // 5. Get page list — core of issue #325.
    let manga_id = manga.id.clone();
    let chapter_id = chapter.id.clone();
    let chapter_num = chapter.chapter_num;
    let pages = tokio::task::spawn_blocking({
        let source = source.clone();
        move || {
            let mut backend = match &source.backend {
                SourceBackend::Aidoku(s) => s.lock().unwrap(),
                _ => panic!("not an aidoku source"),
            };
            let token = CancellationToken::new();
            let result = backend.get_page_list(token, manga_id, chapter_id, chapter_num);
            match &result {
                Ok(p) => eprintln!("pages: {} total", p.len()),
                Err(e) => eprintln!("pages ERR: {e:#}"),
            }
            result
        }
    })
    .await
    .unwrap()
    .expect("get_page_list failed");

    assert!(!pages.is_empty(), "page list is empty — blank pages bug!");
    eprintln!("pages: {} total", pages.len());

    // 6. Verify pages have valid image URLs.
    let mut valid = 0;
    for (i, page) in pages.iter().enumerate() {
        match &page.image_url {
            Some(url) => {
                eprintln!("  page {i}: {url}");
                valid += 1;
            }
            None => eprintln!(
                "  page {i}: NO URL (base64={:?} text={:?})",
                page.base64, page.text
            ),
        }
    }
    assert!(
        valid > 0,
        "no pages have image URLs — blank pages bug! {}/{} pages have None image_url",
        pages.len() - valid,
        pages.len()
    );
    eprintln!("OK: {valid}/{} pages have valid image URLs", pages.len());

    // 7. Download + decrypt EVERY page concurrently (like the real reader),
    //    to surface issues that only appear under MangaPlus's shared view_token
    //    across 23 parallel CDN requests. This is what issue #325 reports.
    let client = tls::client_builder().build().unwrap();
    let mut handles = Vec::new();
    for page in pages.iter() {
        let url = match &page.image_url {
            Some(u) => u.clone(),
            None => continue,
        };
        let ctx = page.ctx.clone();
        let source = source.clone();
        let client = client.clone();
        handles.push(tokio::task::spawn(async move {
            // Build the source request (next-sdk path applies Plus-Vw-Token).
            let req = {
                let source = source.clone();
                let url = url.clone();
                let ctx = ctx.clone();
                tokio::task::spawn_blocking(move || {
                    let mut backend = match &source.backend {
                        SourceBackend::Aidoku(s) => s.lock().unwrap(),
                        _ => panic!("not an aidoku source"),
                    };
                    backend.get_image_request(url, ctx)
                })
                .await
                .unwrap()
            }
            .expect("get_image_request failed");

            let req_headers = req.headers().clone();
            let resp = request_with_forced_referer_from_request(&client, req, 10)
                .await
                .expect("image request failed");
            let status = resp.status();
            let resp_headers = resp.headers().clone();
            let raw_bytes = resp.bytes().await.expect("read image bytes");

            let decrypted = {
                let source = source.clone();
                let url = url.clone();
                let req_headers = req_headers.clone();
                let resp_headers = resp_headers.clone();
                let raw = raw_bytes.clone();
                let ctx = ctx.clone();
                tokio::task::spawn_blocking(move || {
                    let mut backend = match &source.backend {
                        SourceBackend::Aidoku(s) => s.lock().unwrap(),
                        _ => panic!("not an aidoku source"),
                    };
                    backend.process_page_image(
                        CancellationToken::new(),
                        (url, req_headers),
                        (status, resp_headers),
                        raw,
                        ctx,
                    )
                })
                .await
                .unwrap()
            }
            .expect("process_page_image failed");

            let is_jpeg = decrypted.len() >= 3 && decrypted[..3] == [0xFF, 0xD8, 0xFF];
            let is_png = decrypted.len() >= 4 && decrypted[..4] == [0x89, 0x50, 0x4E, 0x47];
            (status, raw_bytes.len(), decrypted.len(), is_jpeg || is_png)
        }));
    }

    let mut blank = 0;
    let mut ok = 0;
    for (i, h) in handles.into_iter().enumerate() {
        let (status, raw, dec, valid) = h.await.unwrap();
        if valid {
            ok += 1;
        } else {
            blank += 1;
            eprintln!(
                "BLANK page {i}: status={status}, raw={raw}B, decrypted={dec}B (not a valid image)"
            );
        }
    }
    eprintln!(
        "concurrent download: {ok} valid, {blank} blank out of {} pages",
        pages.len()
    );
    assert_eq!(
        blank,
        0,
        "blank pages bug reproduced: {blank}/{} pages are blank",
        pages.len()
    );
    eprintln!(
        "OK: all {} pages downloaded and decrypted into valid images",
        pages.len()
    );
}
