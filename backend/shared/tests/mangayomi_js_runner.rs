//! Offline runner tests for MangaYomi JavaScript extensions.
//!
//! Like `mangayomi_runner.rs` but for the QuickJS-backed runtime: a
//! `DefaultExtension` fixture in the style of the mangayomi-extensions
//! JavaScript pack is served against a local HTTP server, driving the whole
//! install (`sourceCodeLanguage: 1` stores `<id>.mangayomi.js`), `MSource`
//! bootstrap, sync getters vs async promise methods, the DOM bridge
//! (`Document`/`Element`), the bridge HTTP client, `SharedPreferences` and
//! settings/reference merging.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use shared::{
    model::SourceId,
    settings::{Settings, SourceSettingValue},
    source::{mangayomi::MangayomiSource, model::SettingDefinition, Source, SourceBackend},
    source_collection::SourceCollection,
    source_manager::SourceManager,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

fn mangayomi(source: &Source) -> &MangayomiSource {
    match &source.backend {
        SourceBackend::Mangayomi(mangayomi) => mangayomi.as_ref(),
        _ => panic!("expected a MangaYomi source"),
    }
}

// ---------------------------------------------------------------------------
// Minimal fixture HTTP server (same pattern as `mangayomi_runner.rs`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct RecordedRequest {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
}

struct FixtureServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    _handle: std::thread::JoinHandle<()>,
}

impl FixtureServer {
    async fn start() -> Self {
        let (addr_tx, addr_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handle = {
            let requests = requests.clone();
            std::thread::Builder::new()
                .name("mangayomi-js-fixture-server".to_string())
                .spawn(move || {
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    runtime.block_on(async move {
                        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                        let addr = listener.local_addr().unwrap();
                        let base = format!("http://{}", addr);
                        let _ = addr_tx.send(addr);
                        loop {
                            let (stream, _peer) = match listener.accept().await {
                                Ok(ok) => ok,
                                Err(_) => break,
                            };
                            let requests = requests.clone();
                            let base = base.clone();
                            tokio::spawn(async move {
                                handle_connection(stream, &requests, &base).await;
                            });
                        }
                    });
                })
                .unwrap()
        };
        let addr = addr_rx.await.unwrap();

        FixtureServer {
            addr,
            requests,
            _handle: handle,
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn query_for(&self, path: &str) -> Option<String> {
        self.requests()
            .iter()
            .rfind(|r| r.path == path)
            .map(|r| r.query.clone())
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<RecordedRequest>>>,
    base: &str,
) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = match stream.read(&mut tmp).await {
            Ok(n) if n > 0 => n,
            _ => break,
        };
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(buf.len());
    let header_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let content_length: usize = lines
        .find_map(|l| {
            let lower = l.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let mut tmp = [0u8; 4096];
        let n = match stream.read(&mut tmp).await {
            Ok(n) if n > 0 => n,
            _ => break,
        };
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = buf
        .get(header_end..header_end + content_length)
        .unwrap_or_default()
        .to_vec();

    requests.lock().unwrap().push(RecordedRequest {
        method: method.clone(),
        path: path.clone(),
        query,
        body: body.clone(),
    });

    let (status, content_type, response) = route(&method, &path, base);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(&response).await;
}

fn route(method: &str, path: &str, base: &str) -> (&'static str, &'static str, Vec<u8>) {
    let html = |content: &str| {
        (
            "200 OK",
            "text/html; charset=utf-8",
            content
                .replace("127.0.0.1:PORT", base.trim_start_matches("http://"))
                .as_bytes()
                .to_vec(),
        )
    };
    match (method, path) {
        _ => match path {
            "/page/1" => html(
                r#"<!DOCTYPE html><html><body>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/one">One Piece</a>
<img src="/uploads/one.jpg"></div>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/two">Two</a>
<img src="/uploads/two.jpg"></div>
</body></html>"#,
            ),
            "/latest/1" => html(
                r#"<!DOCTYPE html><html><body>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/latest-one">Latest One</a>
<img src="/uploads/latest.jpg"></div>
</body></html>"#,
            ),
            "/search" => html(
                r#"<!DOCTYPE html><html><body>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/nami">Nami</a>
<img src="/uploads/nami.jpg"></div>
</body></html>"#,
            ),
            "/manga/one" => html(
                r#"<html><body>
<h1 class="post-title">One Piece</h1>
<div class="summary__content">Pirate adventure story.</div>
<div class="summary_image"><img src="/uploads/cover.jpg"></div>
<div class="genres-content"><a>Action</a><a>Adventure</a></div>
<li class="wp-manga-chapter"><a href="/manga/one/ch/1">Chapter 1</a></li>
<li class="wp-manga-chapter"><a href="/manga/one/ch/2">Chapter 2</a></li>
</body></html>"#,
            ),
            "/manga/one/ch/1" => html(
                r#"<html><body>
<div class="page-break"><img src="/img/p1.jpg"></div>
<div class="page-break"><img src="/img/p2.jpg"></div>
</body></html>"#,
            ),
            "/override/page/1" => html(
                r#"<!DOCTYPE html><html><body>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/one">One Piece</a>
<img src="/uploads/one.jpg"></div>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/two">Two</a>
<img src="/uploads/two.jpg"></div>
</body></html>"#,
            ),
            _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
        },
    }
}

// ---------------------------------------------------------------------------
// Fixture extension (site substituted with the live port)
// ---------------------------------------------------------------------------

// Mirrors the JavaScript extensions from mangayomi-extensions: a
// `DefaultExtension` subclass using `new Client({useDartHttpClient: true})`,
// `new Document(html)`, `SharedPreferences` and the `MProvider.source`
// (MSource) injected into `RAKUYOMI_SOURCE`.
const FIXTURE_EXTENSION: &str = r#"
class DefaultExtension extends MProvider {
    get baseUrl() {
        return this.source.baseUrl;
    }
    get supportsLatest() {
        return true;
    }
    getHeaders(url) {
        return { Referer: this.source.baseUrl };
    }
    get siteBase() {
        return new SharedPreferences().get("site") || this.source.baseUrl;
    }
    async _get(url) {
        const res = await new Client({ useDartHttpClient: true }).get(
            url,
            this.getHeaders(url)
        );
        return res.body;
    }
    _collect(html, selector) {
        const document = new Document(html);
        let mangaList = [];
        for (const el of document.select(selector)) {
            mangaList.push({
                name: el.selectFirst("a.title").text,
                link: el.selectFirst("a.title").attr("href"),
                imageUrl: el.selectFirst("img").attr("src"),
            });
        }
        return mangaList;
    }
    async getPopular(page) {
        const html = await this._get(this.siteBase + "/page/" + page);
        return { list: this._collect(html, "div.item"), hasNextPage: page === 1 };
    }
    async getLatestUpdates(page) {
        const html = await this._get(this.siteBase + "/latest/" + page);
        return { list: this._collect(html, "div.item"), hasNextPage: false };
    }
    async search(query, page, filterList) {
        const html = await this._get(this.siteBase + "/search?q=" + query + "&page=" + page);
        return { list: this._collect(html, "div.item"), hasNextPage: false };
    }
    async getDetail(url) {
        const res = await new Client({ useDartHttpClient: true }).get(
            url,
            this.getHeaders(url)
        );
        const document = new Document(res.body);
        let manga = {};
        manga.name = document.selectFirst("h1.post-title").text;
        manga.link = url;
        manga.description = document.selectFirst("div.summary__content").text;
        manga.imageUrl = document.selectFirst("div.summary_image img").attr("src");
        manga.genre = document.select("div.genres-content a").map(function (e) {
            return e.text;
        });
        manga.status = 0;
        let chapters = [];
        for (const element of document.select("li.wp-manga-chapter")) {
            const ch = element.selectFirst("a");
            if (ch != null) {
                chapters.push({ url: ch.attr("href"), name: ch.text });
            }
        }
        manga.chapters = chapters;
        return manga;
    }
    async getPageList(url) {
        const target = url.startsWith("http")
            ? url
            : this.source.baseUrl + (url.startsWith("/") ? "" : "/") + url;
        const html = await this._get(target);
        const document = new Document(html);
        return document.select("div.page-break img").map(function (e) {
            return e.attr("src");
        });
    }
    getFilterList() {
        return [];
    }
    getSourcePreferences() {
        return [
            {
                key: "site",
                editTextPreference: { title: "Site", value: "" }
            },
            {
                key: "show_notice",
                switchPreferenceCompat: { title: "Show notice", value: true }
            }
        ];
    }
}
"#;

// ---------------------------------------------------------------------------
// Mangafire-like fixture (mirrors the real Mangafire extension shape)
// ---------------------------------------------------------------------------

// Getter `getFilterList()` returns group filters with **no `state` on the
// "Length" select**; `search` indexes `filters[0..4]` unconditionally and
// reads `filters[3].values[filters[3].state].value`. The runtime must feed it
// the normalised defaults the app's `*.fromJson` round-trip would apply.
const MANGAFIRE_LIKE_EXTENSION: &str = r#"
class DefaultExtension extends MProvider {
    get baseUrl() {
        return this.source.baseUrl;
    }
    async search(query, page, filters) {
        var slug = "language=en&page=" + page;
        var isFiltersAvailable = filters || filters.length > 0;
        if (isFiltersAvailable) {
            for (const filter of filters[0].state) {
                if (filter.state == true) slug += "&type%5B%5D=" + filter.value;
            }
            for (const filter of filters[1].state) {
                if (filter.state == 1) slug += "&genre%5B%5D=" + filter.value;
            }
            for (const filter of filters[2].state) {
                if (filter.state == true) slug += "&status%5B%5D=" + filter.value;
            }
            slug += "&minchap=" + filters[3].values[filters[3].state].value;
            slug += "&sort=" + filters[4].values[filters[4].state].value;
        }
        const res = await new Client({ useDartHttpClient: true }).get(
            this.source.baseUrl + "/mf-search?" + slug
        );
        return { list: [], hasNextPage: false };
    }
    getFilterList() {
        return [
            {
                type_name: "GroupFilter",
                name: "Type",
                state: [["Manga", "manga"], ["Manhwa", "manhwa"]].map(
                    (x) => ({ type_name: "CheckBox", name: x[0], value: x[1] })
                ),
            },
            {
                type_name: "GroupFilter",
                name: "Genre",
                state: [["Action", "1"]].map(
                    (x) => ({ type_name: "TriState", name: x[0], value: x[1] })
                ),
            },
            {
                type_name: "GroupFilter",
                name: "Status",
                state: [["Releasing", "releasing"]].map(
                    (x) => ({ type_name: "CheckBox", name: x[0], value: x[1] })
                ),
            },
            {
                type_name: "SelectFilter",
                type: "length",
                name: "Length",
                values: [
                    [">= 1 chapters", "1"],
                    [">= 3 chapters", "3"],
                ].map((x) => ({ type_name: "SelectOption", name: x[0], value: x[1] })),
            },
            {
                type_name: "SelectFilter",
                type: "sort",
                name: "Sort",
                state: 3,
                values: [
                    ["Added", "recently_added"],
                    ["Updated", "recently_updated"],
                    ["Trending", "trending"],
                    ["Most Relevance", "most_relevance"],
                ].map((x) => ({ type_name: "SelectOption", name: x[0], value: x[1] })),
            },
        ];
    }
}
"#;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn temp_sources_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rakuyomi-mangayomi-js-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn manager(dir: &std::path::Path) -> Arc<tokio::sync::Mutex<SourceManager>> {
    Arc::new(tokio::sync::Mutex::new(
        SourceManager::from_folder(dir.to_path_buf(), Settings::default()).unwrap(),
    ))
}

fn install(
    manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    code: &str,
    metadata: &str,
) -> SourceId {
    let source_id = SourceId::new("638504049".to_string());
    manager
        .blocking_lock()
        .install_mangayomi_source(&source_id, code, metadata, "MangaYomi".to_string(), manager)
        .unwrap();
    source_id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn js_runner_full_offline() {
    let server = FixtureServer::start().await;
    let base = server.base_url();
    let port = base
        .trim_start_matches("http://")
        .rsplit(':')
        .next()
        .unwrap();
    let html = |s: &str| s.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}"));

    let dir = temp_sources_dir("full");
    let manager = manager(&dir);
    let source_id = tokio::task::block_in_place(|| {
        install(
            &manager,
            &FIXTURE_EXTENSION.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}")),
            &html(
                r#"{"id": 638504049, "name": "Madara JS Fixture", "lang": "en", "baseUrl": "http://127.0.0.1:PORT", "version": "1.2.0", "sourceCodeUrl": "https://example.com/madara.js", "sourceCodeLanguage": 1}"#,
            ),
        )
    });

    // The `sourceCodeLanguage: 1` index entry must be stored with the `.js`
    // suffix and loadable through the shared pipeline.
    let source = tokio::task::block_in_place(|| {
        manager
            .blocking_lock()
            .get_by_id(&source_id)
            .expect("source installed")
            .clone()
    });
    let manifest = source.manifest();
    assert_eq!(manifest.info.id, "638504049");
    assert_eq!(manifest.info.name, "Madara JS Fixture");
    assert_eq!(manifest.info.lang.as_deref(), Some("en"));
    assert_eq!(
        manifest.info.version,
        serde_json::Value::String("1.2.0".into())
    );
    assert_eq!(manifest.info.url.as_deref(), Some(base.as_str()));
    // `source_of_source` comes from the sidecar meta file written at
    // install time (the source list key), taking precedence over the
    // `sourceCodeUrl` found in the metadata JSON.
    assert_eq!(manifest.source_of_source.as_deref(), Some("MangaYomi"));

    let source = mangayomi(&source);
    assert!(source.supports_latest, "sync getter supportsLatest");
    assert!(!source.features.process_page_image);

    // Extension-declared preferences become the settings definitions and are
    // merged into the shared settings map (and therefore visible to the
    // extension through `SharedPreferences`).
    let defs = &source.setting_definitions;
    assert_eq!(defs.len(), 2);
    let settings = source.settings.lock().unwrap();
    assert!(matches!(
        settings.get(&"show_notice".to_string()),
        Some(SourceSettingValue::Bool(true))
    ));
    assert!(matches!(
        settings.get(&"site".to_string()),
        Some(SourceSettingValue::String(s)) if s.is_empty()
    ));
    drop(settings);

    // Popular list: the extension's `Client.get` hits the fixture, the DOM
    // bridge extracts the list, relative images resolve against the base URL.
    let mangas = source
        .get_manga_list(
            CancellationToken::new(),
            shared::aidoku::Listing {
                id: "popular".to_string(),
                name: "popular".to_string(),
                kind: Default::default(),
            },
        )
        .unwrap();
    assert_eq!(mangas.len(), 2, "two mangas in the fixture list");
    assert_eq!(mangas[0].title.as_deref(), Some("One Piece"));
    assert_eq!(mangas[0].id, format!("{base}/manga/one"));
    assert_eq!(
        mangas[0].cover_url.clone().map(|u| u.to_string()),
        Some(format!("{base}/uploads/one.jpg")),
        "relative image must resolve against the base URL"
    );
    assert_eq!(mangas[1].title.as_deref(), Some("Two"));

    // Latest listing through getLatestUpdates
    let latest = source
        .get_manga_list(
            CancellationToken::new(),
            shared::aidoku::Listing {
                id: "latest".to_string(),
                name: "latest".to_string(),
                kind: Default::default(),
            },
        )
        .unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].title.as_deref(), Some("Latest One"));

    // Search with an explicit query
    let (results, has_next) = source
        .search_mangas(CancellationToken::new(), "nami".to_string(), 1)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title.as_deref(), Some("Nami"));
    assert!(!has_next);
    assert_eq!(
        server.query_for("/search"),
        Some("q=nami&page=1".to_string()),
        "query and page must flow into the extension's search request"
    );

    // Empty query means "browse" and goes through getPopular
    let (browse, has_next) = source
        .search_mangas(CancellationToken::new(), String::new(), 1)
        .unwrap();
    assert_eq!(browse.len(), 2);
    assert!(
        has_next,
        "fixture getPopular returns hasNextPage=true on page 1"
    );

    // Manga details
    let manga = source
        .get_manga_details(CancellationToken::new(), format!("{base}/manga/one"))
        .unwrap();
    assert_eq!(manga.id, format!("{base}/manga/one"));
    assert_eq!(manga.title.as_deref(), Some("One Piece"));
    assert_eq!(
        manga.description.as_deref(),
        Some("Pirate adventure story.")
    );
    assert_eq!(
        manga.tags.as_deref(),
        Some(&["Action".to_string(), "Adventure".to_string()][..])
    );
    assert_eq!(
        manga.cover_url.map(|u| u.to_string()),
        Some(format!("{base}/uploads/cover.jpg"))
    );

    // Chapters from the detail page
    let chapters = source
        .get_chapter_list(CancellationToken::new(), format!("{base}/manga/one"))
        .unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title.as_deref(), Some("Chapter 1"));
    assert_eq!(chapters[0].id, "/manga/one/ch/1");
    assert_eq!(chapters[0].source_order, 0);
    assert_eq!(chapters[1].title.as_deref(), Some("Chapter 2"));
    assert_eq!(chapters[1].source_order, 1);

    // Pages fetched via the absolute chapter URL
    let pages = source
        .get_page_list(
            CancellationToken::new(),
            format!("{base}/manga/one"),
            chapters[0].id.clone(),
            None,
        )
        .unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages[0].image_url.clone().map(|u| u.to_string()),
        Some(format!("{base}/img/p1.jpg"))
    );
    assert_eq!(
        pages[1].image_url.clone().map(|u| u.to_string()),
        Some(format!("{base}/img/p2.jpg"))
    );

    // Image requests carry the extension's `getHeaders` (Referer) plus a
    // default user agent.
    let request = source
        .get_image_request(
            url::Url::parse(&format!("{base}/img/p1.jpg")).unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::GET);
    let referer = request
        .headers()
        .get(reqwest::header::REFERER)
        .map(|v| v.to_str().unwrap().to_string());
    assert_eq!(referer.as_deref(), Some(base.as_str()));
    let ua = request
        .headers()
        .get(reqwest::header::USER_AGENT)
        .map(|v| v.to_str().unwrap().to_string());
    assert!(ua.is_some() && ua.unwrap().starts_with("Mozilla/5.0"));

    // Error paths
    assert!(
        source.invoke("bogus", serde_json::json!([])).is_err(),
        "unknown extension method must fail"
    );

    // Stored settings reach the extension through `SharedPreferences`: with
    // the empty default, `siteBase` falls back to `source.baseUrl`; once a
    // value is stored, the extension requests `/override/page/1`.
    source.settings.lock().unwrap().set(
        "site",
        SourceSettingValue::String(format!("{base}/override")),
    );
    let mangas = source
        .get_manga_list(
            CancellationToken::new(),
            shared::aidoku::Listing {
                id: "popular".to_string(),
                name: "popular".to_string(),
                kind: Default::default(),
            },
        )
        .unwrap();
    assert_eq!(
        server.query_for("/override/page/1"),
        Some(String::new()),
        "stored setting must override the base URL used by the extension"
    );
    assert_eq!(mangas.len(), 2);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn js_runner_stores_js_suffix() {
    // Even an install whose suffix would (incorrectly) default to Dart must
    // honour the `sourceCodeLanguage` in the index entry.
    let dir = temp_sources_dir("suffix");
    let manager = manager(&dir);
    let code = r#"class DefaultExtension extends MProvider { getFilterList() { return []; } }"#;
    let metadata = r#"{"id": 638504050, "name": "Suffix Check", "lang": "en", "baseUrl": "http://example.com", "version": "1.0.0", "sourceCodeLanguage": 1}"#;
    let source_id = install(&manager, code, metadata);
    let source = manager
        .blocking_lock()
        .get_by_id(&source_id)
        .expect("source installed")
        .clone();
    assert_eq!(source.manifest().info.id, "638504050");
    let mangayomi = mangayomi(&source);
    assert_eq!(mangayomi.base_url, "http://example.com");
    assert!(
        manager
            .blocking_lock()
            .mangayomi_js_source_path(&source_id)
            .exists(),
        "sourceCodeLanguage: 1 must be stored with the .mangayomi.js suffix"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn js_runner_rejects_invalid_code() {
    // A JS extension whose `DefaultExtension` fails to evaluate must surface
    // an error on the first invoke instead of hanging.
    let dir = temp_sources_dir("bad-js");
    let manager = manager(&dir);
    let source_id = install(
        &manager,
        "this is not javascript ((",
        r#"{"id": 638504051, "name": "Broken", "lang": "en", "baseUrl": "http://example.com", "version": "1.0.0", "sourceCodeLanguage": 1}"#,
    );
    let source = manager
        .blocking_lock()
        .get_by_id(&source_id)
        .expect("source installed")
        .clone();
    let mangayomi = mangayomi(&source);
    assert!(
        mangayomi
            .invoke("getSourcePreferences", serde_json::json!([]))
            .is_err(),
        "syntax errors must surface as invoke failures"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn js_runner_invoke_timeout() {
    // An extension that never settles must fail promptly via the interrupt
    // handler instead of blocking forever.
    let code = r#"
class DefaultExtension extends MProvider {
    async getPopular(page) {
        await new Promise(function (resolve, reject) {
            setTimeout(resolve, 60000);
        });
        return { list: [], hasNextPage: false };
    }
}
"#;
    let runtime = shared::source::mangayomi::js::MangayomiJsRuntime::new(
        code.to_string(),
        serde_json::json!({
            "id": 638504052,
            "name": "Slow",
            "lang": "en",
            "baseUrl": "http://example.com",
            "version": "1.0.0",
            "sourceCodeLanguage": 1
        }),
        Arc::new(Mutex::new(
            shared::source::source_settings::SourceSettings::new(
                "638504052".to_string(),
                &[],
                &HashMap::new(),
                &Arc::new(tokio::sync::Mutex::new(SourceManager::new(
                    PathBuf::new(),
                    HashMap::new(),
                    Settings::default(),
                ))),
            )
            .unwrap(),
        )),
        Duration::from_millis(500),
    )
    .expect("runtime starts");
    let value = runtime.invoke("getPopular", vec![serde_json::json!(1)]);
    assert!(value.is_err(), "blocked extension call must time out");
}

#[test]
fn js_runner_flat_source_preference_gets_defaults() {
    // Mirrors `runner_flat_source_preference_gets_defaults` for the Dart
    // runtime: a JS extension may construct preferences as flat objects
    // (`{ key, value, ... }`) instead of the `SourcePreference`-style
    // nested maps. Defaults must be collected so `SharedPreferences`
    // lookups resolve instead of falling back.
    const FLAT_PREF: &str = r#"
class DefaultExtension extends MProvider {
    getBaseUrl() {
        return new SharedPreferences().get("override_baseurl") || this.source.baseUrl;
    }
    getSourcePreferences() {
        return [
            {
                key: "override_baseurl",
                title: "Override BaseUrl",
                value: "https://flat.example",
                text: "https://flat.example",
            },
        ];
    }
}
"#;
    let dir = temp_sources_dir("flat-pref");
    let manager = manager(&dir);
    let source_id = install(
        &manager,
        FLAT_PREF,
        r#"{"id": 638504049, "name": "Flat Pref JS", "lang": "en", "baseUrl": "http://meta.example", "version": "1.0.0", "sourceCodeUrl": "https://example.com/flat.js", "sourceCodeLanguage": 1}"#,
    );
    let source = manager
        .blocking_lock()
        .get_by_id(&source_id)
        .expect("source installed")
        .clone();
    let source = mangayomi(&source);

    let defs = &source.setting_definitions;
    assert_eq!(defs.len(), 1);
    match &defs[0] {
        SettingDefinition::Text { key, default, .. } => {
            assert_eq!(key, "override_baseurl");
            assert_eq!(default.as_deref(), Some("https://flat.example"));
        }
        other => panic!("expected Text, got {other:?}"),
    }
    let settings = source.settings.lock().unwrap();
    assert!(matches!(
        settings.get(&"override_baseurl".to_string()),
        Some(SourceSettingValue::String(s)) if s == "https://flat.example"
    ));
    drop(settings);

    let base_url = source.invoke("getBaseUrl", serde_json::json!([])).unwrap();
    assert_eq!(base_url.as_str(), Some("https://flat.example"));

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn js_runner_mangafire_like_search_filters() {
    // Mangafire-style search indexes `filters[0..4]` and its "Length" select
    // omits `state`; the runtime must pass the normalised filter list (state
    // 0) instead of an empty array, and must not crash on the missing state.
    let server = FixtureServer::start().await;
    let base = server.base_url();
    let port = base
        .trim_start_matches("http://")
        .rsplit(':')
        .next()
        .unwrap()
        .to_string();

    let dir = temp_sources_dir("mangafire");
    let manager = manager(&dir);
    let source_id = tokio::task::block_in_place(|| {
        install(
            &manager,
            &MANGAFIRE_LIKE_EXTENSION.replace("127.0.0.1:PORT", &port),
            &format!(
                r#"{{"id": 638504049, "name": "Mangafire Like", "lang": "en", "baseUrl": "http://127.0.0.1:{port}", "version": "1.0.0", "sourceCodeLanguage": 1}}"#
            ),
        )
    });
    let source = tokio::task::block_in_place(|| {
        manager
            .blocking_lock()
            .get_by_id(&source_id)
            .expect("source installed")
            .clone()
    });
    let source = mangayomi(&source);
    source
        .search_mangas(CancellationToken::new(), "one piece".to_string(), 1)
        .expect("search must not crash on the filter list");
    let query = server
        .query_for("/mf-search")
        .expect("extension must hit /mf-search");
    assert!(
        query.contains("minchap=1"),
        "Length select without state must default to 0, got: {query}"
    );
    assert!(
        query.contains("sort=most_relevance"),
        "Sort select state must be preserved, got: {query}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
