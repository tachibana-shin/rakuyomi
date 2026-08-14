//! Live tests for real keiyoushi (mihon) extension APKs.
//!
//! The whole RakuYomi stack is exercised against sites that work today:
//! APK install through `SourceManager`, engine boot (dexvm), the real HTTP
//! callback (Chrome user-agent, redirect tracking), preference persistence,
//! coroutine->classic fallbacks and relative URL absolutisation.
//!
//! Network-dependent: gated behind `#[ignore]` so CI stays offline; run with:
//!
//! ```sh
//! cargo test -p shared --test keiyoushi_live -- --ignored --nocapture
//! ```
//!
//! The APK under test defaults to the vendored mangapill fixture from
//! dex_runtime (`~/dex_runtime/fixtures/tachiyomi-en.mangapill-v1.4.9.apk`);
//! point `RAKUYOMI_APK` at another keiyoushi extension to test it instead.
//!
//! Known live-data caveats (all outside RakuYomi's control):
//! - mangapill's `mangaDetailsParse` leaves title/url empty; it only fills
//!   the description.
//! - mangapill's `pageListParse` selects `div.container > div:first-child >
//!   div:first-child > img`, which the current site layout no longer
//!   matches; even the native Tachiyomi app yields empty page lists today.
//!   Page fetching is therefore exercised (request + parse run end to end)
//!   but not asserted.

use std::{path::PathBuf, sync::Arc};

use shared::{
    model::SourceId,
    settings::Settings,
    source::{keiyoushi::KeiyoushiSource, Source, SourceBackend},
    source_manager::SourceManager,
};
use tokio_util::sync::CancellationToken;

fn keiyoushi(source: &Source) -> &KeiyoushiSource {
    match &source.backend {
        SourceBackend::Keiyoushi(keiyoushi) => keiyoushi.as_ref(),
        _ => panic!("expected a Keiyoushi source"),
    }
}

/// The id the fixture APK registers under: keiyoushi ids are derived from
/// the stored file name (like aidoku `.aix` ids), so installing with the
/// extension's keiyoushi index id (`en.mangapill`) makes the source
/// register under the same id the settings/library see.
const MANGA_PILL_ID: &str = "eu.kanade.tachiyomi.extension.en.mangapill";

fn apk_path() -> PathBuf {
    if let Ok(path) = std::env::var("RAKUYOMI_APK") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/shin".to_string());
    PathBuf::from(home).join("dex_runtime/fixtures/tachiyomi-en.mangapill-v1.4.9.apk")
}

fn temp_sources_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rakuyomi-keiyoushi-live-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Installs the fixture APK through the real `SourceManager` pipeline and
/// returns the registered source.
fn install_mangapill() -> Source {
    let path = apk_path();
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture APK {}: {e}", path.display()));

    // Leak the manager so the borrowed source stays alive for the whole test.
    let manager: &'static Arc<tokio::sync::Mutex<SourceManager>> =
        Box::leak(Box::new(Arc::new(tokio::sync::Mutex::new(
            SourceManager::from_folder(temp_sources_dir(), Settings::default()).unwrap(),
        ))));
    manager
        .blocking_lock()
        .install_keiyoushi_source(
            &SourceId::new(MANGA_PILL_ID.to_string()),
            bytes,
            "keiyoushi/extensions".to_string(),
            manager,
        )
        .unwrap_or_else(|e| panic!("failed to install keiyoushi APK: {e:#}"));

    let guard = manager.blocking_lock();
    let all_ids: Vec<String> = guard
        .sources_by_id
        .keys()
        .map(|id| id.value().clone())
        .collect();
    assert_eq!(all_ids, vec![MANGA_PILL_ID.to_string()]);
    let source = guard
        .sources_by_id
        .get(&SourceId::new(MANGA_PILL_ID.to_string()))
        .expect("mangapill source must be registered")
        .clone();
    drop(guard);
    is_mangapill_or_panic(&source);
    source
}

/// Asserts the registered source is the mangapill extension itself (id,
/// name and language parsed out of the APK, no synthetic metadata).
fn is_mangapill_or_panic(source: &Source) {
    let info = &source.manifest().info;
    assert_eq!(info.id, MANGA_PILL_ID);
    assert_eq!(info.name, "MangaPill");
    assert_eq!(info.lang.as_deref(), Some("en"));
    assert!(matches!(source.backend, SourceBackend::Keiyoushi(_)));
}

#[test]
#[ignore = "live network test; run with --ignored"]
fn mangapill_popular_search_details_chapters_pages() {
    let source = install_mangapill();
    let ks = keiyoushi(&source);
    let cancellation_token = CancellationToken::new();

    // 1. popular listing
    let listing = shared::aidoku::Listing {
        id: "popular".to_string(),
        name: "popular".to_string(),
        kind: Default::default(),
    };
    let mangas = ks
        .get_manga_list(cancellation_token.clone(), listing)
        .unwrap_or_else(|e| panic!("get_manga_list (popular) failed: {e:#}"));
    assert!(
        !mangas.is_empty(),
        "popular must return mangas from mangapill.sea"
    );
    for manga in &mangas {
        assert!(
            manga
                .url
                .as_ref()
                .is_some_and(|u| u.as_str().starts_with("http"))
                || manga.id.starts_with("/"),
            "manga urls must be absolute or slash-paths: {manga:#?}"
        );
    }
    let first = mangas[0].clone();
    eprintln!(
        "live: popular returned {} mangas; first = {:?} ({:?})",
        mangas.len(),
        first.title,
        first.url.as_ref().map(|u| u.to_string())
    );

    // 2. search
    let (results, _has_next) = ks
        .search_mangas(cancellation_token.clone(), "one piece".to_string(), 1)
        .unwrap_or_else(|e| panic!("search_mangas failed: {e:#}"));
    assert!(
        !results.is_empty(),
        "search must return results from mangapill.sea"
    );
    assert!(
        results.iter().any(|m| m
            .title
            .as_deref()
            .is_some_and(|t| t.to_lowercase().contains("one piece"))),
        "top search results should include One Piece: {:?}",
        results
            .iter()
            .map(|m| m.title.clone().unwrap_or_default())
            .collect::<Vec<_>>()
    );
    eprintln!(
        "live: search('one piece') returned {} results",
        results.len()
    );

    // 3. details via the popular first entry (a real detail-page URL)
    let manga_id = first
        .url
        .as_ref()
        .map(|u| u.to_string())
        .unwrap_or(first.id.clone());
    eprintln!("live: details call for {manga_id}");
    let details = ks
        .get_manga_details(cancellation_token.clone(), manga_id.clone())
        .unwrap_or_else(|e| panic!("get_manga_details failed: {e:#}"));
    eprintln!(
        "live: details: title={:?} description_len={}",
        details.title,
        details.description.as_deref().map_or(0, str::len)
    );
    // The mangapill extension itself leaves title/url empty on the detail
    // parse; what it really fills is the description. Assert on that (the
    // live data proves the request/parse pipeline end to end).
    assert!(
        details
            .description
            .as_deref()
            .is_some_and(|d| !d.is_empty()),
        "details must carry the manga description"
    );

    // 4. chapters
    let chapters = ks
        .get_chapter_list(cancellation_token.clone(), manga_id)
        .unwrap_or_else(|e| panic!("get_chapter_list failed: {e:#}"));
    assert!(
        !chapters.is_empty(),
        "chapters must be listed for {:?}",
        details.title
    );
    let first_chapter = chapters[0].clone();
    eprintln!(
        "live: {} chapters; first = {:?} ({}) url={:?}",
        chapters.len(),
        first_chapter.title,
        first_chapter
            .chapter_num
            .map(|n| n.to_string())
            .unwrap_or_default(),
        first_chapter.url
    );

    // 5. pages: run the full request + parse pipeline on a few chapters.
    //    Not asserted: mangapill's `pageListParse` selects a layout the
    //    current site no longer serves (see module docs), so even the
    //    native app gets empty lists today.
    let mut pages = Vec::new();
    for chapter in chapters.iter().take(6) {
        let page_list = ks
            .get_page_list(
                cancellation_token.clone(),
                chapter.manga_id.clone(),
                chapter.id.clone(),
                chapter.chapter_num,
            )
            .unwrap_or_else(|e| panic!("get_page_list failed: {e:#}"));
        eprintln!(
            "live: pages call for {:?} returned {}",
            chapter.title,
            page_list.len()
        );
        if !page_list.is_empty() {
            pages = page_list;
            break;
        }
    }
    if pages.is_empty() {
        eprintln!(
            "live: warn: no reader markup parsed for the first chapters — the \
             mangapill extension's page selectors are stale vs. the current \
             site layout (the native app fails the same way)"
        );
    } else {
        for page in &pages {
            assert!(
                page.image_url.is_some() || page.text.is_some(),
                "every page must carry an image url or text: {page:#?}"
            );
        }
        eprintln!(
            "live: {} pages; first = {}",
            pages.len(),
            page_url(&pages[0])
        );

        // 6. image request headers for the first page
        let image_url = pages[0]
            .image_url
            .clone()
            .expect("keiyoushi pages are image urls");
        ks.get_image_request(image_url.clone(), None)
            .unwrap_or_else(|e| panic!("get_image_request failed: {e:#}"));
        eprintln!("live: image request for {image_url} built ok");
    }
}

fn page_url(page: &shared::source::model::Page) -> String {
    page.image_url
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| page.text.clone().unwrap_or_default())
}
