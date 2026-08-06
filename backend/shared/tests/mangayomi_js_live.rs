//! Live tests for real MangaYomi JavaScript extensions.
//!
//! Vendored from the mangayomi-extensions repo (see `tests/data/mangayomi-js/`)
//! so the JS runtime is exercised against sites that actually work today, not
//! just fixtures. Network-dependent: gated behind `#[ignore]` so CI stays
//! offline; run with:
//!
//! ```sh
//! cargo test -p shared --test mangayomi_js_live -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use shared::{
    model::SourceId,
    settings::Settings,
    source::{mangayomi::MangayomiSource, Source, SourceBackend},
    source_collection::SourceCollection,
    source_manager::SourceManager,
};
use tokio_util::sync::CancellationToken;

fn mangayomi(source: &Source) -> &MangayomiSource {
    match &source.backend {
        SourceBackend::Mangayomi(mangayomi) => mangayomi.as_ref(),
        _ => panic!("expected a MangaYomi source"),
    }
}

fn temp_sources_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rakuyomi-mangayomi-js-live-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Installs a vendored extension and returns a handle for invoking it.
fn install(
    code: &str,
    id: &str,
    name: &str,
    base_url: &str,
    api_url: &str,
) -> &'static MangayomiSource {
    let dir = temp_sources_dir(id);
    // Leak the manager so the borrowed source stays alive for the whole test;
    // the runtime worker holds the code anyway.
    let manager: &'static mut SourceManager = Box::leak(Box::new(
        SourceManager::from_folder(dir, Settings::default()).unwrap(),
    ));
    let source_id = SourceId::new(id.to_string());
    let metadata = format!(
        r#"{{"id": "{}", "name": "{}", "lang": "en", "baseUrl": "{}", "apiUrl": "{}", "iconUrl": "", "version": "1.0.0", "sourceCodeLanguage": 1, "typeSource": "single", "itemType": 0, "isManga": true}}"#,
        id, name, base_url, api_url
    );
    manager
        .install_mangayomi_source(&source_id, code, &metadata, "MangaYomi".to_string())
        .unwrap();
    mangayomi(manager.get_by_id(&source_id).expect("source installed"))
}

fn vendored(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/mangayomi-js")
            .join(name),
    )
    .unwrap()
}

fn popular(source: &MangayomiSource) -> usize {
    match source.get_manga_list(
        CancellationToken::new(),
        shared::aidoku::Listing {
            id: "popular".to_string(),
            name: "popular".to_string(),
            kind: Default::default(),
        },
    ) {
        Ok(list) => list.len(),
        Err(e) => {
            eprintln!("popular: {e:#}");
            0
        }
    }
}

fn search(source: &MangayomiSource, query: &str) -> (usize, Vec<String>) {
    match source.search_mangas(CancellationToken::new(), query.to_string(), 1) {
        Ok((list, _)) => (
            list.len(),
            list.iter()
                .map(|m| m.title.clone().unwrap_or_default())
                .collect(),
        ),
        Err(e) => {
            eprintln!("search: {e:#}");
            (0, vec![])
        }
    }
}

#[test]
#[ignore = "live network test; run with --ignored"]
fn weeb_central_works_live() {
    let source = install(
        &vendored("weebcentral.js"),
        "693275080",
        "Weeb Central",
        "https://weebcentral.com",
        "",
    );
    assert!(
        popular(source) > 0,
        "getPopular must return results from weebcentral.com"
    );
    let (n, titles) = search(source, "one piece");
    assert!(n > 0, "search must return results from weebcentral.com");
    assert!(
        titles
            .iter()
            .any(|t| t.to_lowercase().contains("one piece")),
        "top search results should include One Piece: {titles:?}"
    );
}

#[test]
#[ignore = "live network test; run with --ignored"]
fn manhwaz_works_live() {
    let source = install(
        &vendored("manhwaz.js"),
        "5738565393",
        "ManhwaZ",
        "https://manhwaz.com",
        "",
    );
    assert!(
        popular(source) > 0,
        "getPopular must return results from manhwaz.com"
    );
    let (n, titles) = search(source, "one piece");
    assert!(n > 0, "search must return results from manhwaz.com");
    assert!(
        titles
            .iter()
            .any(|t| t.to_lowercase().contains("one piece")),
        "top search results should include One Piece: {titles:?}"
    );
}

#[test]
#[ignore = "live network test; run with --ignored"]
fn webtoons_works_live() {
    let source = install(
        &vendored("webtoons.js"),
        "5738565394",
        "Webtoons",
        "https://www.webtoons.com",
        "",
    );
    assert!(
        popular(source) > 0,
        "getPopular must return results from webtoons.com"
    );
    let (n, _) = search(source, "one piece");
    assert!(n > 0, "search must return results from webtoons.com");
}

#[test]
#[ignore = "live network test; run with --ignored"]
fn mangadex_search_works_live() {
    let source = install(
        &vendored("mangadex.js"),
        "810342358",
        "MangaDex",
        "https://mangadex.org",
        "https://api.mangadex.org",
    );
    let (n, titles) = search(source, "one piece");
    assert!(n > 0, "search must return results from the mangadex API");
    assert!(
        titles
            .iter()
            .any(|t| t.to_lowercase().contains("one piece")),
        "top search results should include One Piece: {titles:?}"
    );
}
