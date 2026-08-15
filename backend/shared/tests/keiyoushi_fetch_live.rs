//! TEMPORARY live probe: verifies the full RakuYomi download path for
//! IMGX-encrypted keiyoushi sources. Installs the moetruyen extension,
//! fetches a page list, then calls Source::fetch_page_image and checks
//! the bytes are decrypted (not "IMGX"-headed). Run with RAKUYOMI_APK set.
//! Not part of the permanent suite: remove after promotion.

use std::sync::Arc;

use shared::{
    model::SourceId,
    settings::Settings,
    source::{Source, SourceBackend},
    source_manager::SourceManager,
};

fn keiyoushi(source: &Source) -> &shared::source::keiyoushi::KeiyoushiSource {
    match &source.backend {
        SourceBackend::Keiyoushi(k) => k.as_ref(),
        _ => panic!("expected keiyoushi source"),
    }
}

#[test]
#[ignore = "live network probe; run with --ignored"]
fn probe_fetch_page_image() {
    let apk = std::env::var("RAKUYOMI_APK").expect("set RAKUYOMI_APK");
    eprintln!("=== FETCH-PAGE-IMAGE PROBE {apk} ===");
    let bytes = std::fs::read(&apk).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "rakuyomi-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let manager = Arc::new(tokio::sync::Mutex::new(
        SourceManager::from_folder(dir, Settings::default()).unwrap(),
    ));
    manager
        .blocking_lock()
        .install_keiyoushi_source(
            &SourceId::new("probe".to_string()),
            bytes,
            "keiyoushi/extensions".to_string(),
            &manager,
        )
        .unwrap_or_else(|e| panic!("install failed: {e:#}"));
    let manager_guard = manager.blocking_lock();
    let source = manager_guard
        .sources_by_id
        .iter()
        .find(|(_, s)| s.manifest().info.lang.as_deref() == Some("vi"))
        .or_else(|| manager_guard.sources_by_id.iter().next())
        .expect("no source")
        .1
        .clone();
    drop(manager_guard);
    let info = &source.manifest().info;
    eprintln!(
        "manifest: id={} name={:?} lang={:?}",
        info.id, info.name, info.lang
    );

    let ks = keiyoushi(&source);
    use tokio_util::sync::CancellationToken;
    let tok = CancellationToken::new();

    let listing = shared::aidoku::Listing {
        id: "popular".to_string(),
        name: "popular".to_string(),
        kind: Default::default(),
    };
    let mangas = ks.get_manga_list(tok.clone(), listing);
    let mangas = match &mangas {
        Ok(m) if !m.is_empty() => {
            eprintln!("popular: {} mangas; first={:?}", m.len(), m[0].title);
            mangas.unwrap()
        }
        Ok(_) => panic!("popular OK but EMPTY"),
        Err(e) => panic!("popular ERR {e:#}"),
    };
    let manga = mangas[0].clone();
    let manga_id = manga
        .url
        .map(|u| u.path().to_string())
        .unwrap_or(manga.id.clone());
    let details = ks.get_manga_details(tok.clone(), manga_id.clone());
    match &details {
        Ok(d) => eprintln!("details: title={:?}", d.title),
        Err(e) => eprintln!("details: ERR {e:#}"),
    }
    let chapters = ks.get_chapter_list(tok.clone(), manga_id.clone());
    let chapters = match &chapters {
        Ok(c) if !c.is_empty() => {
            eprintln!("chapters: {} total; first={:?}", c.len(), c[0].title);
            chapters.unwrap()
        }
        Ok(_) => panic!("chapters OK but EMPTY"),
        Err(e) => panic!("chapters ERR {e:#}"),
    };

    let chapter = chapters[0].clone();
    let pages = ks.get_page_list(
        tok.clone(),
        chapter.manga_id.clone(),
        chapter.id.clone(),
        chapter.chapter_num,
    );
    let pages = match &pages {
        Ok(p) if !p.is_empty() => {
            eprintln!("pages: {} total; first={:?}", p.len(), p[0].image_url);
            pages.unwrap()
        }
        Ok(_) => panic!("pages OK but EMPTY"),
        Err(e) => panic!("pages ERR {e:#}"),
    };

    let first = pages[0].image_url.clone().unwrap();
    eprintln!("fetch_page_image: chapter={} url={first}", chapter.id);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let out = rt.block_on(source.fetch_page_image(&chapter.id, &first));
    match &out {
        Ok(b) => {
            let head: Vec<u8> = b.iter().take(8).copied().collect();
            let magic = String::from_utf8_lossy(&head).to_string();
            eprintln!(
                "fetch_page_image OK: {} bytes, magic={:?} text={:?}",
                b.len(),
                head,
                magic
            );
            assert!(
                !head.starts_with(b"IMGX"),
                "bytes still IMGX-encrypted: {head:?}"
            );
            assert!(
                b.len() > 1000,
                "suspiciously small image: {} bytes",
                b.len()
            );
        }
        Err(e) => panic!("fetch_page_image ERR {e:#}"),
    }
    eprintln!("=== FETCH-PAGE-IMAGE PROBE PASSED ===");
}
