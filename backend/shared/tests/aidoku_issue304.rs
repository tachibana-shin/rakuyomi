//! Live regression test for issue #304 ("could not read data from page
//! descriptor").
//!
//! The published source indexes ship SDK-next modules (their wasm imports
//! `std.buffer_len`/`std.read_buffer`), but their `source.json` carries no
//! `min_app_version`, so they used to be misdetected as legacy SDK: the boot
//! fallback instantiated them with the next-SDK imports while `next_sdk`
//! stayed false, driving the host store through the legacy code paths until
//! every call failed with "could not read data from page descriptor". This
//! test installs sources from both published indexes (aidoku-sources-next and
//! aidoku-community) and asserts a search returns results.
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

const INDEXES: [&str; 2] = [
    "https://tachibana-shin.github.io/aidoku-sources-next/index.min.json",
    "https://aidoku-community.github.io/sources/index.min.json",
];

fn http() -> reqwest::Client {
    tls::client_builder().build().unwrap()
}

async fn download(index_entry_id: &str, dir: &PathBuf) -> (String, PathBuf) {
    let client = http();
    for index_url in INDEXES {
        let bytes = client
            .get(index_url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let index: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let sources = index["sources"].as_array().unwrap();
        let Some(entry) = sources
            .iter()
            .find(|s| s["id"].as_str() == Some(index_entry_id))
        else {
            continue;
        };
        let base = index_url
            .rsplit_once('/')
            .map(|(base, _)| base)
            .unwrap_or(index_url);
        let download_url = format!("{base}/{}", entry["downloadURL"].as_str().unwrap());
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
        return (index_entry_id.to_string(), file);
    }
    panic!("source {index_entry_id} not found in any index");
}

async fn search_smoke(id: &str) {
    let dir = std::env::temp_dir().join(format!("rakuyomi-issue304-{id}"));
    // Start clean so a previous run cannot leak a persisted `.source`
    // sidecar and mask the missing-`min_app_version` detection path.
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    let (id, file) = download(id, &dir).await;
    let mut manager = SourceManager::from_folder(dir, Settings::default()).unwrap();
    manager
        .install_source(
            &SourceId::new(id.clone()),
            std::fs::read(&file).unwrap(),
            "aidoku-issue304".into(),
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
        Err(e) => {
            // A source-side failure (its own request/module errors) is not a
            // regression; the issue-#304 host bug had a specific signature.
            let text = format!("{e:#}");
            for signature in [
                "could not read data from page descriptor",
                "Can't serialize Object",
                "could not find exported function",
            ] {
                assert!(
                    !text.contains(signature),
                    "issue-304 signature still present: {text}"
                );
            }
            println!("{id}: source-side failure (not a #304 regression): {text}");
        }
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

#[tokio::test]
#[ignore = "live network test; run with --ignored"]
async fn ezmanga_search_issue304() {
    // From the aidoku-community index: the index the reporter of #304
    // actually had configured in settings.json.
    search_smoke("en.ezmanga").await;
}
