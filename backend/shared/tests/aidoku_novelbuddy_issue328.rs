//! Live regression test for issue #328 (NovelBuddy chapter content).
//!
//! Network-dependent: gated behind `#[ignore]` so CI stays offline; run with:
//!
//! ```sh
//! cargo test -p shared --test aidoku_novelbuddy_issue328 -- --ignored --nocapture
//! ```

use std::sync::Arc;

use shared::{
    model::SourceId, settings::Settings, source::SourceBackend, source_manager::SourceManager, tls,
};
use tokio_util::sync::CancellationToken;

const INDEX_URL: &str = "https://aidoku-community.github.io/sources/index.min.json";
const SOURCE_ID: &str = "en.novelbuddy";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test; run with --ignored"]
async fn novelbuddy_chapter_contains_text_issue328() {
    let client = tls::client_builder().build().unwrap();
    let index: serde_json::Value = client
        .get(INDEX_URL)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let entry = index["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["id"].as_str() == Some(SOURCE_ID))
        .unwrap();
    let download_url = format!(
        "https://aidoku-community.github.io/sources/{}",
        entry["downloadURL"].as_str().unwrap()
    );
    let bytes = client
        .get(download_url)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let dir = std::env::temp_dir().join("rakuyomi-issue328-novelbuddy");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{SOURCE_ID}.aix"));
    std::fs::write(&file, &bytes).unwrap();

    let mut manager = SourceManager::from_folder(dir, Settings::default()).unwrap();
    let arc_manager = Arc::new(tokio::sync::Mutex::new(manager.clone()));
    manager
        .install_source(
            &SourceId::new(SOURCE_ID.to_string()),
            bytes.to_vec(),
            "issue328-test".into(),
            &arc_manager,
        )
        .unwrap();
    let source = manager
        .sources_by_id
        .get(&SourceId::new(SOURCE_ID.to_string()))
        .unwrap()
        .clone();

    let mangas = tokio::task::spawn_blocking({
        let source = source.clone();
        move || {
            let mut backend = match &source.backend {
                SourceBackend::Aidoku(source) => source.lock().unwrap(),
                _ => panic!("not an aidoku source"),
            };
            backend
                .get_manga_list(
                    CancellationToken::new(),
                    aidoku::Listing {
                        id: "latest".into(),
                        name: "Latest Updates".into(),
                        kind: aidoku::ListingKind::default(),
                    },
                )
                .unwrap()
        }
    })
    .await
    .unwrap();
    assert!(!mangas.is_empty(), "NovelBuddy returned no novels");

    let chapters = tokio::task::spawn_blocking({
        let source = source.clone();
        let manga_id = mangas[0].id.clone();
        move || {
            let mut backend = match &source.backend {
                SourceBackend::Aidoku(source) => source.lock().unwrap(),
                _ => panic!("not an aidoku source"),
            };
            backend
                .get_chapter_list(CancellationToken::new(), manga_id)
                .unwrap()
        }
    })
    .await
    .unwrap();
    assert!(!chapters.is_empty(), "NovelBuddy returned no chapters");

    let chapter = chapters[0].clone();
    let pages = tokio::task::spawn_blocking(move || {
        let mut backend = match &source.backend {
            SourceBackend::Aidoku(source) => source.lock().unwrap(),
            _ => panic!("not an aidoku source"),
        };
        backend
            .get_page_list(
                CancellationToken::new(),
                chapter.manga_id,
                chapter.id.clone(),
                chapter.chapter_num,
            )
            .unwrap()
    })
    .await
    .unwrap();
    assert!(!pages.is_empty(), "NovelBuddy returned no chapter pages");
    assert!(
        pages.iter().any(|page| {
            page.text.as_ref().is_some_and(|text| {
                let text = text.trim();
                !text.is_empty() && text != "(empty chapter)"
            })
        }),
        "NovelBuddy chapter pages contained no converted text"
    );
}
