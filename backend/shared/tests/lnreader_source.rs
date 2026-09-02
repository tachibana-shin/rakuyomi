//! End-to-end smoke tests for the LNReader plugin runner.
//!
//! Each test loads a real plugin from the LNReader repository
//! (`tests/data/*.js`) and runs the full pipeline: popular list, novel
//! details, chapter list, chapter html, and search. Requires network access;
//! a test is skipped when its fixture is missing.

use std::path::PathBuf;
use std::sync::Arc;

use shared::{
    settings::Settings, source_collection::SourceCollection, source_manager::SourceManager,
};
use tokio_util::sync::CancellationToken;

fn fixture(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    path.exists().then_some(path)
}

async fn run_plugin_smoke(
    plugin_file: &str,
    plugin_id: &str,
    expected_name: &str,
    expected_url_host: &str,
    search_query: &str,
) {
    let Some(fixture) = fixture(plugin_file) else {
        eprintln!("skipping: fixture tests/data/{plugin_file} not found");
        return;
    };

    let settings = Settings::default();
    let dir = std::env::temp_dir().join(format!(
        "rakuyomi-lnreader-e2e-{plugin_id}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let manager = Arc::new(tokio::sync::Mutex::new(
        SourceManager::from_folder(dir.clone(), settings).unwrap(),
    ));

    let contents = std::fs::read(&fixture).unwrap();
    let id = shared::model::SourceId::new(plugin_id.to_string());
    manager
        .lock()
        .await
        .install_lnreader_source(&id, contents, "LNReader".to_string(), &manager)
        .unwrap();

    let source = manager
        .lock()
        .await
        .get_by_id(&id)
        .expect("source installed")
        .clone();
    let manifest = source.manifest();
    assert_eq!(manifest.info.id, plugin_id);
    assert_eq!(manifest.info.name, expected_name);
    assert!(
        manifest
            .info
            .version
            .as_str()
            .is_some_and(|v| v.contains('.')),
        "LNReader version is a string"
    );

    // search_mangas with a query exercises the plugin's searchNovels method.
    // An empty query would call popularNovels, which requires filters that the
    // LNReader runtime does not pass, so we use a real search term instead.
    let mut mangas = Vec::new();
    let mut has_next = false;
    for attempt in 1..=4 {
        let (result, next) = source
            .search_mangas(CancellationToken::new(), search_query.to_string(), 1)
            .await
            .unwrap();
        mangas = result;
        has_next = next;
        if !mangas.is_empty() {
            break;
        }
        eprintln!("search came back empty (attempt {attempt}), retrying...");
        tokio::time::sleep(std::time::Duration::from_secs(5 * attempt)).await;
    }
    assert!(!mangas.is_empty(), "search must return results");
    assert!(!has_next);
    assert!(
        !mangas[0].id.is_empty() && !mangas[0].id.starts_with('/'),
        "expected plugin-relative path as id, got: {}",
        mangas[0].id
    );
    let popular_id = mangas[0].id.clone();

    // get_manga_details
    let manga = source
        .get_manga_details(CancellationToken::new(), popular_id.clone())
        .await
        .unwrap();
    assert!(manga.title.as_deref().is_some_and(|t| !t.is_empty()));

    // get_chapter_list
    let chapters = source
        .get_chapter_list(CancellationToken::new(), popular_id.clone())
        .await
        .unwrap();
    assert!(!chapters.is_empty(), "chapter list must not be empty");
    let first = chapters[0].clone();
    assert!(
        first.url.is_some(),
        "chapter url must be resolved (image base)"
    );
    assert!(
        first
            .url
            .as_ref()
            .unwrap()
            .as_str()
            .contains(expected_url_host),
        "chapter url must point at the plugin site, got: {}",
        first.url.as_ref().unwrap()
    );

    // get_page_list (chapter html)
    let pages = source
        .get_page_list(
            CancellationToken::new(),
            popular_id.clone(),
            first.id.clone(),
            first.chapter_num,
        )
        .await
        .unwrap();
    assert_eq!(pages.len(), 1);
    let text = pages[0].text.as_ref().unwrap();
    assert!(text.starts_with("<!-- html -->\n"));

    // search
    let (results, _) = source
        .search_mangas(CancellationToken::new(), search_query.to_string(), 1)
        .await
        .unwrap();
    assert!(!results.is_empty(), "search must return results");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn lnreader_plugin_end_to_end_novelbuddy() {
    run_plugin_smoke(
        "novelbuddy.js",
        "novelbuddy",
        "NovelBuddy",
        "novelbuddy.me",
        "shadow slave",
    )
    .await;
}

/// Royal Road has no `resolveUrl`, so this also exercises the `site + path`
/// chapter URL fallback.
#[tokio::test]
async fn lnreader_plugin_end_to_end_royalroad() {
    run_plugin_smoke(
        "royalroad.js",
        "royalroad",
        "Royal Road",
        "royalroad.com",
        "apocalypse",
    )
    .await;
}
