//! TEMPORARY batch probe: finds keiyoushi fixtures whose page lists parse
//! live. Run once per APK with RAKUYOMI_APK set; deletes nothing, prints
//! everything. Not part of the permanent suite: remove after promotion.
//!
//! Needs to live in tests/ because shared's SourceManager must be reachable.

use std::sync::Arc;

use shared::{
    model::SourceId,
    settings::Settings,
    source::{keiyoushi::KeiyoushiSource, Source, SourceBackend},
    source_manager::SourceManager,
};

fn keiyoushi(source: &Source) -> &KeiyoushiSource {
    match &source.backend {
        SourceBackend::Keiyoushi(k) => k.as_ref(),
        _ => panic!("expected keiyoushi source"),
    }
}

#[test]
#[ignore = "live network probe; run with --ignored"]
fn probe_apk() {
    let apk = std::env::var("RAKUYOMI_APK").expect("set RAKUYOMI_APK");
    eprintln!("=== PROBE {apk} ===");
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
            None,
        )
        .unwrap_or_else(|e| panic!("install failed: {e:#}"));
    let manager_guard = manager.blocking_lock();
    let source = manager_guard
        .sources_by_id
        .iter()
        .find(|(_, s)| s.manifest().info.lang.as_deref() == Some("en"))
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
    match &mangas {
        Ok(m) => eprintln!(
            "popular: {} mangas; first={:?} {:?}",
            m.len(),
            m[0].title,
            m[0].url
        ),
        Err(e) => eprintln!("popular: ERR {e:#}"),
    }
    let mangas = mangas.unwrap_or_default();

    let search = ks.search_mangas(tok.clone(), "berserk".to_string(), 1);
    match &search {
        Ok((r, _)) if !r.is_empty() => {
            eprintln!("search: {} results; first={:?}", r.len(), r[0].title)
        }
        Ok((_r, _)) => eprintln!("search: OK but EMPTY"),
        Err(e) => eprintln!("search: ERR {e:#}"),
    }
    let search = search.unwrap_or_default();
    let _ = &search;

    // Prefer the search result: the popular-list top entry is affected by a
    // mangadex-side webnovel migration (en feed returns external-only
    // chapters), which makes the chapter-list legitimately empty.
    let first = if !search.0.is_empty() {
        search.0[0].clone()
    } else if !mangas.is_empty() {
        mangas[0].clone()
    } else {
        eprintln!("NO MANGA FOUND: aborting pages");
        return;
    };
    let manga_id = first
        .url
        .map(|u| u.path().to_string())
        .unwrap_or(first.id.clone());
    eprintln!("details call for {manga_id}");

    let details = ks.get_manga_details(tok.clone(), manga_id.clone());
    match &details {
        Ok(d) => eprintln!(
            "details: title={:?} desc_len={}",
            d.title,
            d.description.as_deref().map_or(0, str::len)
        ),
        Err(e) => eprintln!("details: ERR {e:#}"),
    }
    let _ = details;

    let chapters = ks.get_chapter_list(tok.clone(), manga_id);
    match &chapters {
        Ok(c) => eprintln!(
            "chapters: {} total; first={:?} num={:?}",
            c.len(),
            c[0].title,
            c[0].chapter_num
        ),
        Err(e) => eprintln!("chapters: ERR {e:#}"),
    }
    let chapters = chapters.unwrap_or_default();

    for (i, ch) in chapters.iter().take(3).enumerate() {
        let pages = ks.get_page_list(
            tok.clone(),
            ch.manga_id.clone(),
            ch.id.clone(),
            ch.chapter_num,
        );
        match &pages {
            Ok(p) if !p.is_empty() => eprintln!(
                "pages[{}]: chapter={:?} => {} pages; first={:?}",
                i,
                ch.title,
                p.len(),
                p[0].image_url
            ),
            Ok(_p) => eprintln!("pages[{}]: chapter={:?} => EMPTY", i, ch.title),
            Err(e) => eprintln!("pages[{}]: ERR {e:#}", i),
        }
    }
}
