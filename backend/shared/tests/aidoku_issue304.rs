//! Live regression test for issue #304 ("could not read data from page
//! descriptor").
//!
//! The aidoku-sources-next index publishes SDK-next modules (their wasm
//! imports `std.buffer_len`/`std.read_buffer`), but their `source.json`
//! carries no `min_app_version`, so they used to be misdetected as legacy
//! SDK: the boot fallback instantiated them with the next-SDK imports while
//! `next_sdk` stayed false, driving the host store through the legacy code
//! paths until every call failed with "could not read data from page
//! descriptor". This test installs two such sources from the live index and
//! asserts a search returns results.
//!
//! Network-dependent: gated behind `#[ignore]` so CI stays offline; run with:
//!
//! ```sh
//! cargo test -p shared --test aidoku_issue304 -- --ignored --nocapture
//! ```

use std::{path::PathBuf, sync::Arc};

use shared::{
    model::SourceId, settings::Settings, source::SourceBackend, source_manager::SourceManager, tls,
};
use tokio_util::sync::CancellationToken;

const INDEX_URL: &str = "https://tachibana-shin.github.io/aidoku-sources-next/index.min.json";

fn http() -> reqwest::Client {
    tls::client_builder().build().unwrap()
}

async fn download(index_entry_id: &str, dir: &PathBuf) -> (String, PathBuf) {
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
        .find(|s| s["id"].as_str() == Some(index_entry_id))
        .unwrap();
    let download_url = format!(
        "https://tachibana-shin.github.io/aidoku-sources-next/{}",
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
    let file = dir.join(format!("{index_entry_id}.aix"));
    std::fs::write(&file, bytes).unwrap();
    (index_entry_id.to_string(), file)
}

async fn search_smoke(id: &str) {
    let dir = std::env::temp_dir().join(format!("rakuyomi-issue304-{id}"));
    std::fs::create_dir_all(&dir).unwrap();
    let (id, file) = download(id, &dir).await;
    let mut manager = SourceManager::from_folder(dir, Settings::default()).unwrap();
    manager
        .install_source(
            &SourceId::new(id.clone()),
            std::fs::read(&file).unwrap(),
            "aidoku-sources-next".into(),
            &Arc::new(tokio::sync::Mutex::new(manager.clone())),
        )
        .unwrap();

    let source = manager
        .sources_by_id
        .get(&SourceId::new(id.clone()))
        .unwrap()
        .clone();
    let token = CancellationToken::new();
    let result = tokio::task::spawn_blocking(move || {
        let mut backend = match &source.backend {
            SourceBackend::Aidoku(s) => s.lock().unwrap(),
            _ => panic!("not an aidoku source"),
        };
        let result = backend.search_mangas(token, "one piece".to_string(), 1);
        // The module boots on the first call; an SDK-next module must have
        // been driven with the next-SDK host paths (issue #304).
        assert!(backend.next_sdk, "SDK-next module must boot as next SDK");
        result
    })
    .await
    .unwrap();

    match result {
        Ok((mangas, has_next)) => {
            assert!(!mangas.is_empty(), "expected search results");
            println!("{id}: {} results, has_next={has_next}", mangas.len());
        }
        Err(e) => panic!("search failed for {id}: {e:#}"),
    }
}

#[tokio::test]
#[ignore = "live network test; run with --ignored"]
async fn mangadex_search_issue304() {
    search_smoke("multi.mangadex").await;
}

#[tokio::test]
#[ignore = "live network test; run with --ignored"]
async fn weebcentral_search_issue304() {
    search_smoke("en.weebcentral").await;
}
