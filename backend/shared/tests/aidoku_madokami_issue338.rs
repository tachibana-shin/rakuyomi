//! Live regression test for issue #338 (wasm image requests missing cookies).
//!
//! The shared cookie store (`cookies.json`) is applied to regular wasm
//! `net.send` requests, but not to image requests, which are built on the
//! host side by `Source::get_image_request*`. Madokami is the concrete
//! casualty: HTML pages authenticate with HTTP Basic auth, but `/reader/image`
//! ignores `Authorization` and only serves the page bytes to requests carrying
//! the `laravel_session` cookie the Basic-auth login set.
//!
//! This test seeds a fake `laravel_session` cookie into the store, then builds
//! an image request for `manga.madokami.al` exactly like the chapter
//! downloader does, and asserts the session cookie made it onto the request —
//! behaviour that failed with `HTTP 401 Unauthorized` before the fix.
//!
//! Network-dependent: gated behind `#[ignore]` so CI stays offline; run with:
//!
//! ```sh
//! cargo test -p shared --test aidoku_madokami_issue338 -- --ignored --nocapture
//! ```

use std::{collections::HashMap, sync::Arc};

use shared::{
    cookie_store::{init_cookie_store, record_set_cookie_headers},
    settings::{Settings, SourceSettingValue},
    source::SourceBackend,
    source_manager::SourceManager,
    tls,
};
use url::Url;

const INDEX_URL: &str = "https://tachibana-shin.github.io/aidoku-sources-next/index.min.json";
const SOURCE_ID: &str = "en.madokami";
const IMAGE_HOST: &str = "manga.madokami.al";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live network test; run with --ignored"]
async fn image_request_carries_cookie_store_session_issue338() {
    let client = tls::client_builder().build().unwrap();

    // 1. Download the en.madokami source. Use the rakuyomi index: the
    //    aidoku-community published artifact is currently a stale 404.
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
    let base = INDEX_URL.rsplit_once('/').map(|(base, _)| base).unwrap();
    let download_url = format!("{base}/{}", entry["downloadURL"].as_str().unwrap());
    let bytes = client
        .get(download_url)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let dir = std::env::temp_dir().join("rakuyomi-issue338-madokami");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{SOURCE_ID}.aix")), &bytes).unwrap();

    // 2. Seed the shared cookie store with the session cookie Madokami's
    //    `/reader/image` endpoint requires (normally set by the Basic-auth
    //    login flow and persisted in `cookies.json`).
    init_cookie_store();
    let cookie_url = Url::parse(&format!("https://{IMAGE_HOST}/")).unwrap();
    record_set_cookie_headers(
        &cookie_url,
        &["laravel_session=deadbeefcafe; Path=/".to_string()],
    );

    // 3. Provide the Basic-auth credentials the extension reads via
    //    `defaults_get("login.username" / "login.password")`.
    let mut settings = Settings::default();
    settings.source_settings.insert(
        SOURCE_ID.to_string(),
        HashMap::from([
            (
                "login.username".to_string(),
                SourceSettingValue::String("rakuyomi-issue338".to_string()),
            ),
            (
                "login.password".to_string(),
                SourceSettingValue::String("rakuyomi-issue338".to_string()),
            ),
        ]),
    );

    let mut manager = SourceManager::from_folder(dir, settings).unwrap();
    let arc_manager = Arc::new(tokio::sync::Mutex::new(manager.clone()));
    manager
        .install_source(
            &shared::model::SourceId::new(SOURCE_ID.to_string()),
            bytes.to_vec(),
            "issue338-test".into(),
            &arc_manager,
        )
        .unwrap();
    let source = manager
        .sources_by_id
        .get(&shared::model::SourceId::new(SOURCE_ID.to_string()))
        .unwrap()
        .clone();

    // 4. Build an image request exactly like `chapter_downloader.rs` does.
    let image_url = Url::parse(&format!(
        "https://{IMAGE_HOST}/reader/image?path=issue338&file=1.jpg"
    ))
    .unwrap();
    let request = tokio::task::spawn_blocking({
        let source = source.clone();
        let image_url = image_url.clone();
        move || {
            let mut backend = match &source.backend {
                SourceBackend::Aidoku(backend) => backend.lock().unwrap(),
                _ => panic!("not an aidoku source"),
            };
            backend.get_image_request(image_url, None)
        }
    })
    .await
    .unwrap()
    .unwrap_or_else(|err| panic!("get_image_request failed: {err:#}"));

    // 5. The store's session cookie must be on the wire: before the fix the
    //    image request carried only what the extension set (no Cookie), so
    //    Madokami answered `HTTP 401 Unauthorized`.
    let cookie = request
        .headers()
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        cookie.contains("laravel_session=deadbeefcafe"),
        "image request is missing the cookie-store session cookie, got Cookie: {cookie:?}"
    );
    eprintln!("OK: image request carries Cookie {{ {cookie} }}");
    eprintln!(
        "note: the published {} module exports no get_image_request/modify_image_request, \
         so the built request is url + UA + cookie only",
        SOURCE_ID
    );
}
