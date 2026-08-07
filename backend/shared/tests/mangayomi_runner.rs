//! Offline runner tests for MangaYomi extensions.
//!
//! A synthetic fixture extension (a `MProvider` subclass in the style of the
//! mangayomi-extensions repo) is served against a local HTTP server, so the
//! full runner pipeline (install, `main(MSource)` bootstrap, method dispatch,
//! DOM parsing, HTTP via the bridge client, preference seeding, model
//! conversion) is verified without touching the network.

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
    source::{
        mangayomi::{runtime::MangayomiRuntime, MangayomiSource},
        model::{PublishingStatus, SettingDefinition},
        Source, SourceBackend,
    },
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
// Minimal fixture HTTP server (same pattern as `lnreader_runner.rs`)
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
                .name("mangayomi-fixture-server".to_string())
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

    let (status, content_type, response) = route(&method, &path, &body, base);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(&response).await;
}

fn route(
    method: &str,
    path: &str,
    body: &[u8],
    base: &str,
) -> (&'static str, &'static str, Vec<u8>) {
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
        ("POST", "/echo") => ("200 OK", "application/json", body.to_vec()),
        ("GET", "/") => html(
            r#"<html><body>
<div class="c-tabs-item__content">
<div class="post-title"><a href="http://127.0.0.1:PORT/manga/one">One Piece</a></div>
<img data-src="https://cdn.example.com/one.jpg">
</div>
</body></html>"#,
        ),
        ("POST", "/wp-admin/admin-ajax.php") => html(
            r#"<html><body>
<div class="listing-chapters_wrap"><ul class="main version-chap">
<li class="wp-manga-chapter"><a href="http://127.0.0.1:PORT/manga/one/ch/1">Chapter 1</a></li>
<li class="wp-manga-chapter"><a href="http://127.0.0.1:PORT/manga/one/ch/2">Chapter 2</a></li>
</ul></div>
</body></html>"#,
        ),
        _ => match path {
            "/page/1" => html(
                r#"<!DOCTYPE html><html><body>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/one">One Piece</a>
<img src="https://cdn.example.com/one.jpg"></div>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/two">Two</a>
<img src="https://cdn.example.com/two.jpg"></div>
</body></html>"#,
            ),
            "/latest/1" => html(
                r#"<!DOCTYPE html><html><body>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/latest-one">Latest One</a>
<img src="https://cdn.example.com/latest.jpg"></div>
</body></html>"#,
            ),
            "/search" => html(
                r#"<!DOCTYPE html><html><body>
<div class="item"><a class="title" href="http://127.0.0.1:PORT/manga/nami">Nami</a>
<img src="https://cdn.example.com/nami.jpg"></div>
</body></html>"#,
            ),
            "/manga/one" => html(
                r#"<html><body>
<h1 class="post-title">One Piece</h1>
<div class="summary__content">Pirate adventure story.</div>
<div class="summary-content">OnGoing</div>
<div class="summary_image"><img src="https://cdn.example.com/cover.jpg"></div>
<div class="genres-content"><a>Action</a><a>Adventure</a></div>
<li class="wp-manga-chapter"><a href="/manga/one/ch/1">Chapter 1</a></li>
<li class="wp-manga-chapter"><a href="/manga/one/ch/2">Chapter 2</a></li>
</body></html>"#,
            ),
            "/manga/hiatus" => html(
                r#"<html><body>
<h1 class="post-title">On Hold</h1>
<div class="summary__content">Slowed to a crawl.</div>
<div class="summary-content">Hiatus</div>
<div class="summary_image"><img src="https://cdn.example.com/hold.jpg"></div>
<li class="wp-manga-chapter"><a href="/manga/hiatus/ch/1">Chapter 1</a></li>
</body></html>"#,
            ),
            "/manga/one/ch/1" => html(
                r#"<html><body>
<div class="page-break"><img src="https://cdn.example.com/p1.jpg"></div>
<div class="page-break"><img src="https://cdn.example.com/p2.jpg"></div>
</body></html>"#,
            ),
            "/novel/one" => html(
                r#"<html><body>
<h1 class="post-title">Super Gene</h1>
<li class="chapter-row"><a href="/novel/one/ch/1">Chapter 1</a></li>
<li class="chapter-row"><a href="/novel/one/ch/2">Chapter 2</a></li>
</body></html>"#,
            ),
            "/novel/one/ch/1" => html(
                r#"<html><body>
<div class="chapter-content"><p>It was a dark and stormy night.</p></div>
</body></html>"#,
            ),
            "/novel/one/ch/2" => html(
                r#"<html><body>
<div class="chapter-content"><p>Meanwhile, in the Arctic.</p></div>
</body></html>"#,
            ),
            "/xpath/directory/1.htm" => html(
                r#"<html><body>
<div class="manga-list-1-list">
<li><a title="Xpath One" href="http://127.0.0.1:PORT/xpath/manga/one"><img class="manga-list-1-cover" src="https://cdn.example.com/x1.jpg"></a></li>
<li><a title="Xpath Two" href="http://127.0.0.1:PORT/xpath/manga/two"><img class="manga-list-1-cover" src="https://cdn.example.com/x2.jpg"></a></li>
<li><a title="Xpath Three" href="http://127.0.0.1:PORT/xpath/manga/three"><img class="manga-list-1-cover" src="https://cdn.example.com/x3.jpg"></a></li>
</div>
</body></html>"#,
            ),
            "/xpath/manga/one" => html(
                r#"<html><body>
<div class="detail-name"><h1>Xpath One</h1></div>
<div class="fullcontent">Pirate adventure story.</div>
<div class="detail-info-right-say"><a>Author Name</a></div>
<div class="detail-info-right-tag-list"><a>Action</a><a>Adventure</a></div>
<ul class="chapter-list">
<li><a href="http://127.0.0.1:PORT/xpath/manga/one/ch/1">Chapter 1</a></li>
<li><a href="http://127.0.0.1:PORT/xpath/manga/one/ch/2">Chapter 2</a></li>
</ul>
</body></html>"#,
            ),
            "/xpath/manga/one/ch/1" => html(
                r#"<html><body>
<div id="readerarea"><p><img src="https://cdn.example.com/p1.jpg"></p><p><img src="https://cdn.example.com/p2.jpg"></p></div>
</body></html>"#,
            ),
            _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
        },
    }
}

// ---------------------------------------------------------------------------
// Fixture extension + metadata (site substituted with the live port)
// ---------------------------------------------------------------------------

const FIXTURE_EXTENSION: &str = r#"
import 'package:mangayomi/bridge_lib.dart';

class Madara extends MProvider {
  Madara({required this.source});
  MSource source;
  final Client client = Client();

  String get baseUrl => source.baseUrl;
  bool get supportsLatest => true;

  @override
  Future<MPages> getPopular(int page) async {
    final res = (await client.get(Uri.parse("${source.baseUrl}/page/$page"))).body;
    final document = parseHtml(res);
    List<MManga> mangaList = [];
    for (final el in document.select("div.item")) {
      MManga manga = MManga();
      manga.name = el.selectFirst("a.title").text;
      manga.link = el.selectFirst("a.title").getHref;
      manga.imageUrl = el.selectFirst("img").getSrc;
      mangaList.add(manga);
    }
    return MPages(mangaList, page == 1);
  }

  @override
  Future<MPages> getLatestUpdates(int page) async {
    final res = (await client.get(Uri.parse("${source.baseUrl}/latest/$page"))).body;
    final document = parseHtml(res);
    List<MManga> mangaList = [];
    for (final el in document.select("div.item")) {
      MManga manga = MManga();
      manga.name = el.selectFirst("a.title").text;
      manga.link = el.selectFirst("a.title").getHref;
      manga.imageUrl = el.selectFirst("img").getSrc;
      mangaList.add(manga);
    }
    return MPages(mangaList, false);
  }

  @override
  Future<MPages> search(String query, int page, FilterList filterList) async {
    String url = "${source.baseUrl}/search?q=$query&page=$page";
    for (final filter in filterList.filters) {
      if (filter.type == "OrderByFilter" && filter.state != 0) {
        url += "&order=${filter.values[filter.state].value}";
      }
    }
    final res = (await client.get(Uri.parse(url))).body;
    final document = parseHtml(res);
    List<MManga> mangaList = [];
    for (final el in document.select("div.item")) {
      MManga manga = MManga();
      manga.name = el.selectFirst("a.title").text;
      manga.link = el.selectFirst("a.title").getHref;
      manga.imageUrl = el.selectFirst("img").getSrc;
      mangaList.add(manga);
    }
    return MPages(mangaList, false);
  }

  @override
  Future<MManga> getDetail(String url) async {
    final statusList = [
      {
        "OnGoing": 0,
        "Complete": 1,
        "Hiatus": 2,
        "Canceled": 3,
        "PublishingFinished": 4,
      }
    ];
    final res = (await client.get(Uri.parse(url))).body;
    final document = parseHtml(res);
    MManga manga = MManga();
    manga.name = document.selectFirst("h1.post-title").text;
    manga.link = url;
    manga.description = document.selectFirst("div.summary__content").text;
    manga.status = parseStatus(document.selectFirst("div.summary-content").text, statusList);
    manga.imageUrl = document.selectFirst("div.summary_image img").getSrc;
    manga.genre = document.select("div.genres-content a").map((e) => e.text).toList();
    List<MChapter> chapters = [];
    for (final element in document.select("li.wp-manga-chapter")) {
      var ch = element.selectFirst("a");
      if (ch != null) {
        var chapter = MChapter();
        chapter.url = ch.getHref;
        chapter.name = ch.text;
        chapters.add(chapter);
      }
    }
    manga.chapters = chapters;
    return manga;
  }

  @override
  Future<Map<String, dynamic>> postEcho() async {
    final res = await client.post(Uri.parse("${source.baseUrl}/echo"),
        body: {"hello": "world"});
    return {
      "statusCode": res.statusCode,
      "body": res.body,
      "contentType": res.headers["content-type"],
    };
  }

  @override
  Future<List<String>> getPageList(String url) async {
    // Resolve relative chapter URLs (as mangayomi extensions do) into the
    // absolute URL the HTTP client needs.
    final target = url.startsWith("http")
        ? url
        : "${baseUrl}${url.startsWith('/') ? '' : '/'}$url";
    final res = (await client.get(Uri.parse(target))).body;
    final document = parseHtml(res);
    return document.select("div.page-break img").map((e) => e.getSrc).toList();
  }

  @override
  List<dynamic> getFilterList() {
    return [
      TextFilter("GenreListFilter", "Genre", ""),
      SelectFilter("OrderByFilter", "Order By", 0, [
        SelectFilterOption("Relevance", ""),
        SelectFilterOption("Latest", "latest"),
      ]),
    ];
  }

  @override
  List<dynamic> getSourcePreferences() {
    return [
      SourcePreference(key: 'show_notice', switchPreferenceCompat: SwitchPreferenceCompat(title: 'Show notice', value: true)),
      SourcePreference(key: 'view_mode', listPreference: ListPreference(title: 'View mode', entries: ['List', 'Grid'], entryValues: ['list', 'grid'], valueIndex: 1)),
    ];
  }
}

Madara main(MSource source) => Madara(source: source);
"#;

// A light-novel style extension overrides `getHtmlContent` and leaves
// `getPageList` unimplemented; the source must route novel chapters through
// the former and produce a single text page.
const NOVEL_FIXTURE_EXTENSION: &str = r#"
import 'package:mangayomi/bridge_lib.dart';

class RoyalRoad extends MProvider {
  RoyalRoad({required this.source});
  MSource source;
  final Client client = Client();

  String get baseUrl => source.baseUrl;

  @override
  Future<MManga> getDetail(String url) async {
    final res = (await client.get(Uri.parse(url))).body;
    final document = parseHtml(res);
    MManga manga = MManga();
    manga.name = document.selectFirst("h1.post-title").text;
    manga.link = url;
    List<MChapter> chapters = [];
    for (final element in document.select("li.chapter-row")) {
      var chapter = MChapter();
      chapter.url = element.selectFirst("a").getHref;
      chapter.name = element.selectFirst("a").text;
      chapters.add(chapter);
    }
    manga.chapters = chapters;
    return manga;
  }

  @override
  Future<String> getHtmlContent(String name, String url) async {
    final target = url.startsWith("http")
        ? url
        : "${baseUrl}${url.startsWith('/') ? '' : '/'}$url";
    final res = (await client.get(Uri.parse(target))).body;
    final document = parseHtml(res);
    return document.selectFirst("div.chapter-content").text;
  }

  @override
  List<dynamic> getFilterList() {
    return [
      SortFilter("OrderByFilter", "Order by", SortState(0, false), [
        SelectFilterOption("Relevance", ""),
        SelectFilterOption("Popularity", "popularity"),
      ]),
      SeparatorFilter(),
    ];
  }

  @override
  List<dynamic> getSourcePreferences() {
    return [];
  }
}

RoyalRoad main(MSource source) => RoyalRoad(source: source);
"#;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn temp_sources_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rakuyomi-mangayomi-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn manager(dir: &std::path::Path) -> SourceManager {
    SourceManager::from_folder(dir.to_path_buf(), Settings::default()).unwrap()
}

fn install(manager: &mut SourceManager, code: &str, metadata: &str) -> SourceId {
    let source_id = SourceId::new("638504049".to_string());
    manager
        .install_mangayomi_source(&source_id, code, metadata, "MangaYomi".to_string())
        .unwrap();
    source_id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn runner_full_offline() {
    let server = FixtureServer::start().await;
    let base = server.base_url();

    // The fixture HTML embeds absolute URLs for the popular list and the
    // metadata baseUrl for the extension's own requests. Substitute the real
    // port everywhere.
    let port = base
        .trim_start_matches("http://")
        .rsplit(':')
        .next()
        .unwrap();
    let html = |s: &str| s.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}"));

    let dir = temp_sources_dir("full");
    let mut manager = manager(&dir);
    let source_id = install(
        &mut manager,
        &FIXTURE_EXTENSION.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}")),
        &html(
            r#"{"id": 638504049, "name": "Madara Fixture", "lang": "en", "baseUrl": "http://127.0.0.1:PORT", "version": "1.2.0", "sourceCodeUrl": "https://example.com/madara.dart"}"#,
        ),
    );

    let source = manager.get_by_id(&source_id).expect("source installed");
    let manifest = source.manifest();
    assert_eq!(manifest.info.id, "638504049");
    assert_eq!(manifest.info.name, "Madara Fixture");
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

    let source = mangayomi(source);
    assert!(source.supports_latest);
    assert!(!source.features.process_page_image);

    // Extension-declared preferences become the settings definitions, with
    // defaults collected into the shared settings map.
    let defs = &source.setting_definitions;
    assert_eq!(defs.len(), 2);
    match &defs[0] {
        SettingDefinition::Switch {
            title,
            key,
            default,
        } => {
            assert_eq!(title, "Show notice");
            assert_eq!(key, "show_notice");
            assert!(*default);
        }
        other => panic!("expected Switch, got {other:?}"),
    }
    match &defs[1] {
        SettingDefinition::Select {
            title,
            key,
            values,
            titles,
            default,
        } => {
            assert_eq!(title, "View mode");
            assert_eq!(key, "view_mode");
            assert_eq!(values, &vec!["list".to_string(), "grid".to_string()]);
            assert_eq!(
                titles.as_deref(),
                Some(&vec!["List".to_string(), "Grid".to_string()][..])
            );
            assert_eq!(default.as_deref(), Some("grid"));
        }
        other => panic!("expected Select, got {other:?}"),
    }
    let settings = source.settings.lock().unwrap();
    assert!(matches!(
        settings.get("show_notice"),
        Some(SourceSettingValue::Bool(true))
    ));
    assert!(matches!(
        settings.get("view_mode"),
        Some(SourceSettingValue::String(s)) if s == "grid"
    ));
    drop(settings);

    // Raw filter list: positional constructor args are mapped onto the
    // model field names (`type`, `name`, `state`, ...) like in the app.
    let filters = source
        .invoke("getFilterList", serde_json::json!([]))
        .unwrap();
    let filters = filters.as_array().unwrap();
    assert_eq!(filters.len(), 2);
    assert_eq!(filters[0]["type"], serde_json::json!("GenreListFilter"));
    assert_eq!(filters[1]["type"], serde_json::json!("OrderByFilter"));

    // Popular list
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
        mangas[0].url.clone().map(|u| u.to_string()).unwrap(),
        format!("{base}/manga/one")
    );
    assert_eq!(
        mangas[0].cover_url.clone().map(|u| u.to_string()),
        Some("https://cdn.example.com/one.jpg".to_string())
    );
    assert_eq!(mangas[1].title.as_deref(), Some("Two"));

    // Latest listing goes through getLatestUpdates when supported
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

    // Search
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

    // Manga details: the id round-trips into the extension's HTTP request
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
        manga.status,
        PublishingStatus::Ongoing,
        "status text 'OnGoing' must map through parseStatus + MStatus to Ongoing"
    );
    assert_eq!(
        manga.tags.as_deref(),
        Some(&["Action".to_string(), "Adventure".to_string()][..])
    );
    assert_eq!(
        manga.cover_url.map(|u| u.to_string()),
        Some("https://cdn.example.com/cover.jpg".to_string())
    );

    // A second status label exercises a different parseStatus mapping
    // ("Hiatus" -> statusList value 2 -> MStatus.onHiatus -> Hiatus).
    let hiatus = source
        .get_manga_details(CancellationToken::new(), format!("{base}/manga/hiatus"))
        .unwrap();
    assert_eq!(hiatus.title.as_deref(), Some("On Hold"));
    assert_eq!(
        hiatus.status,
        PublishingStatus::Hiatus,
        "'Hiatus' must map to Hiatus (not Cancelled/Unknown)"
    );

    // Chapters come from the detail page
    let chapters = source
        .get_chapter_list(CancellationToken::new(), format!("{base}/manga/one"))
        .unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title.as_deref(), Some("Chapter 1"));
    assert_eq!(chapters[0].id, "/manga/one/ch/1");
    assert_eq!(
        chapters[0].url.clone().map(|u| u.to_string()).unwrap(),
        format!("{base}/manga/one/ch/1")
    );
    assert_eq!(chapters[0].source_order, 0);
    assert_eq!(chapters[1].title.as_deref(), Some("Chapter 2"));
    assert_eq!(chapters[1].source_order, 1);

    // Pages: chapter id (an absolute URL) is fetched directly
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
        Some("https://cdn.example.com/p1.jpg".to_string())
    );
    assert_eq!(
        pages[1].image_url.clone().map(|u| u.to_string()),
        Some("https://cdn.example.com/p2.jpg".to_string())
    );

    // Image requests get the default user agent
    let request = source
        .get_image_request(
            url::Url::parse("https://cdn.example.com/p1.jpg").unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::GET);
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

    // Filter state flows into the search URL: the fixture reads
    // `filterList.filters`, matches the OrderByFilter type and appends
    // `&order=<values[state].value>`.
    let filtered = source
        .invoke(
            "search",
            serde_json::json!([
                "nami",
                1,
                [{
                    "typeName": "SelectFilter",
                    "type": "OrderByFilter",
                    "name": "Order By",
                    "state": 1,
                    "values": [
                        {"typeName": "SelectFilterOption", "name": "Relevance", "value": ""},
                        {"typeName": "SelectFilterOption", "name": "Latest", "value": "latest"}
                    ]
                }]
            ]),
        )
        .unwrap();
    assert_eq!(filtered["list"].as_array().unwrap().len(), 1);
    assert_eq!(
        server.query_for("/search"),
        Some("q=nami&page=1&order=latest".to_string()),
        "the SelectFilter state (index 1 -> 'latest') must reach the request URL"
    );

    // Client.post serialises the named `body` map as JSON, and the Response
    // exposes statusCode/body/headers to the extension.
    let echo = source.invoke("postEcho", serde_json::json!([])).unwrap();
    assert_eq!(echo["statusCode"], serde_json::json!(200));
    assert_eq!(echo["body"], serde_json::json!("{\"hello\":\"world\"}"));
    assert_eq!(echo["contentType"], serde_json::json!("application/json"));

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn runner_rejects_invalid_metadata() {
    let dir = temp_sources_dir("invalid-meta");
    let mut manager = manager(&dir);

    // Metadata without an id must be rejected before any code runs
    let source_id = SourceId::new("badmeta".to_string());
    assert!(manager
        .install_mangayomi_source(
            &source_id,
            b"some code",
            r#"{"name": "No id", "baseUrl": "http://example.com"}"#,
            "MangaYomi".to_string(),
        )
        .is_err());

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn runner_madara_search_with_not_has_selector() {
    // Madara-style sources (e.g. MangaSushi) select search results with
    // `div.c-tabs-item__content` and filter badges via
    // `a:not(:has(span.manga-title-badges))`.
    const MADARA_SEARCH_EXTENSION: &str = r#"
import 'package:mangayomi/bridge_lib.dart';

class MadaraSearch extends MProvider {
  MadaraSearch({required this.source});
  MSource source;
  final Client client = Client();

  String get baseUrl => source.baseUrl;

  String? extractImageUrl(MElement? imageElement) {
    if (imageElement == null) return "";
    return imageElement.attr("data-src");
  }

  MPages mangaFromElements(List<MElement> elements) {
    List<MManga> mangaList = [];
    for (final el in elements) {
      final postTitle = el.selectFirst(
        "div.post-title a:not(:has(span.manga-title-badges))",
      );
      final image = extractImageUrl(el.selectFirst("img"));
      MManga manga = MManga();
      manga.name = postTitle.text;
      manga.imageUrl = substringBefore(image, " ");
      manga.link = postTitle.getHref;
      mangaList.add(manga);
    }
    return MPages(mangaList, true);
  }

  @override
  Future<MPages> search(String query, int page, FilterList filterList) async {
    String url = "${baseUrl}/?s=$query&post_type=wp-manga";
    final res = (await client.get(Uri.parse(url))).body;
    final document = parseHtml(res);
    return mangaFromElements(document.select("div.c-tabs-item__content"));
  }
}

MadaraSearch main(MSource source) => MadaraSearch(source: source);
"#;

    let server = FixtureServer::start().await;
    let base = server.base_url();
    let port = base
        .trim_start_matches("http://")
        .rsplit(':')
        .next()
        .unwrap();
    let html = |s: &str| s.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}"));

    let dir = temp_sources_dir("madara-search");
    let mut manager = manager(&dir);
    let source_id = install(
        &mut manager,
        MADARA_SEARCH_EXTENSION,
        &html(
            r#"{"id": 638504049, "name": "Madara Search", "lang": "en", "baseUrl": "http://127.0.0.1:PORT", "version": "1.0.0", "sourceCodeUrl": "https://example.com/madara.dart"}"#,
        ),
    );

    let source = manager.get_by_id(&source_id).expect("source installed");
    let source = mangayomi(source);

    let (mangas, has_next) = source
        .search_mangas(CancellationToken::new(), "one".to_string(), 1)
        .unwrap();
    assert!(has_next);
    assert_eq!(mangas.len(), 1, "search must find the c-tabs-item__content");
    assert_eq!(mangas[0].title.as_deref(), Some("One Piece"));
    assert_eq!(
        mangas[0].url.as_ref().map(|u| u.as_str()),
        Some(format!("{base}/manga/one").as_str())
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn runner_flat_source_preference_gets_defaults() {
    // Real extensions (e.g. the mangareader/madara multisrc) construct their
    // preferences directly: `EditTextPreference(key: ..., value: ...)` is a
    // flat map. The runtime must still collect the default so
    // `getPreferenceValue`/`override_baseurl` resolve instead of returning
    // null and producing a `null/?s=...` request.
    const FLAT_PREF: &str = r#"
import 'package:mangayomi/bridge_lib.dart';

class FlatPref extends MProvider {
  FlatPref({required this.source});
  MSource source;
  final Client client = Client();

  @override
  String get baseUrl => getPreferenceValue(source.id, "override_baseurl");

  @override
  List<dynamic> getSourcePreferences() {
    return [
      EditTextPreference(
        key: "override_baseurl",
        title: "Override BaseUrl",
        value: "https://flat.example",
        dialogTitle: "Override BaseUrl",
        text: "https://flat.example",
      ),
    ];
  }
}

FlatPref main(MSource source) => FlatPref(source: source);
"#;

    let dir = temp_sources_dir("flat-pref");
    let mut manager = manager(&dir);
    let source_id = install(
        &mut manager,
        FLAT_PREF,
        r#"{"id": 638504049, "name": "Flat Pref", "lang": "en", "baseUrl": "http://meta.example", "version": "1.0.0", "sourceCodeUrl": "https://example.com/flat.dart"}"#,
    );
    let source = manager.get_by_id(&source_id).expect("source installed");
    let source = mangayomi(source);

    // The flat EditTextPreference must become a Text setting with the default
    // collected into the shared settings map.
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
        settings.get("override_baseurl"),
        Some(SourceSettingValue::String(s)) if s == "https://flat.example"
    ));
    drop(settings);

    // And `getPreferenceValue(source.id, "override_baseurl")` must now
    // resolve to the default base URL, not null.
    let base_url = source.invoke("baseUrl", serde_json::json!([])).unwrap();
    assert_eq!(base_url.as_str(), Some("https://flat.example"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn runner_worker_crashes_on_invalid_code() {
    // A worker whose `main()` fails dies quickly; the next invoke must
    // surface the failure instead of hanging or returning garbage.
    let runtime = MangayomiRuntime::new(
        "not dart at all !!!".to_string(),
        serde_json::json!({"id": 1, "name": "x", "baseUrl": "http://x"}),
        Arc::new(Mutex::new(HashMap::new())),
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(runtime.invoke("getSourcePreferences", vec![]).is_err());
}

#[tokio::test]
async fn runner_light_novel_uses_get_html_content() {
    let server = FixtureServer::start().await;
    let base = server.base_url();
    let port = base
        .trim_start_matches("http://")
        .rsplit(':')
        .next()
        .unwrap();
    let html = |s: &str| s.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}"));

    let dir = temp_sources_dir("novel");
    let mut manager = manager(&dir);
    let source_id = install(
        &mut manager,
        &NOVEL_FIXTURE_EXTENSION.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}")),
        &html(
            r#"{"id": 638504049, "name": "RoyalRoad Fixture", "lang": "en", "baseUrl": "http://127.0.0.1:PORT", "version": "1.0.0", "sourceCodeUrl": "https://example.com/royalroad.dart", "itemType": 2}"#,
        ),
    );

    let source = manager.get_by_id(&source_id).expect("source installed");
    let source = mangayomi(source);
    assert_eq!(source.item_type, 2);

    // Light novel sources expose no page images; the raw chapter HTML
    // becomes a single text page.
    let chapters = source
        .get_chapter_list(CancellationToken::new(), format!("{base}/novel/one"))
        .unwrap();
    assert_eq!(chapters.len(), 2);

    let pages = source
        .get_page_list(
            CancellationToken::new(),
            format!("{base}/novel/one"),
            chapters[0].id.clone(),
            None,
        )
        .unwrap();
    assert_eq!(pages.len(), 1, "a novel chapter is a single text page");
    assert!(pages[0].image_url.is_none());
    let text = pages[0].text.as_deref().unwrap();
    assert!(
        text.starts_with("<!-- html -->\n"),
        "text pages are marked as raw HTML for the downloader"
    );
    assert!(text.contains("It was a dark and stormy night."));
    assert_eq!(pages[0].chapter_id, chapters[0].id);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn runner_rejects_anime_extension() {
    let dir = temp_sources_dir("anime");
    let mut manager = manager(&dir);

    // Anime extensions (`itemType: 1`) are not supported: install must fail
    // before the Dart code is even parsed.
    let source_id = SourceId::new("anime1".to_string());
    let err = manager
        .install_mangayomi_source(
            &source_id,
            b"void main() {}",
            r#"{"id": "anime1", "name": "AniList", "lang": "en", "baseUrl": "http://anilist.co", "version": "1.0.0", "sourceCodeUrl": "https://example.com/anilist.dart", "itemType": 1}"#,
            "MangaYomi".to_string(),
        )
        .expect_err("anime extensions must be rejected");
    assert!(
        err.to_string().contains("anime"),
        "error should mention anime: {err}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// An extension in the style of the MangaHere source: all scraping goes
// through the top-level `xpath()` helper, `xpath(...).first`, the
// `parseHtml(...).xpath(...)`/`xpathFirst(...)` methods and element-level
// `xpathFirst`.
const XPATH_FIXTURE_EXTENSION: &str = r#"
import 'package:mangayomi/bridge_lib.dart';

class XpathMadara extends MProvider {
  XpathMadara({required this.source});
  MSource source;
  final Client client = Client();

  String get baseUrl => source.baseUrl;
  bool get supportsLatest => true;

  @override
  Future<MPages> getPopular(int page) async {
    final res = (await client.get(Uri.parse("${source.baseUrl}/xpath/directory/$page.htm"))).body;
    final names = xpath(res, '//*[ contains(@class, "manga-list-1-list")]/li/a/@title');
    final images = xpath(res, '//*[ contains(@class, "manga-list-1-list")]/li/a/img[@class="manga-list-1-cover"]/@src');
    final urls = xpath(res, '//*[ contains(@class, "manga-list-1-list")]/li/a/@href');
    List<MManga> mangaList = [];
    for (var i = 0; i < names.length; i++) {
      MManga manga = MManga();
      manga.name = names[i];
      manga.imageUrl = images[i];
      manga.link = urls[i];
      mangaList.add(manga);
    }
    return MPages(mangaList, true);
  }

  @override
  Future<MPages> getLatestUpdates(int page) async {
    return getPopular(page);
  }

  @override
  Future<MManga> getDetail(String url) async {
    final res = (await client.get(Uri.parse(url))).body;
    MManga manga = MManga();
    manga.name = xpath(res, '//*[@class="detail-name"]/text()').first;
    manga.description = xpath(res, '//*[@class="fullcontent"]/text()').first;
    manga.author = xpath(res, '//*[@class="detail-info-right-say"]/a/text()').first;
    manga.genre = xpath(res, '//*[@class="detail-info-right-tag-list"]/a/text()');
    manga.status = parseStatus('ongoing', [
      {"OnGoing": 0, "Complete": 1},
    ]);
    List<MChapter> chapters = [];
    final document = parseHtml(res);
    final docTitles = document.xpath('//*[@class="chapter-list"]/li/a/@href');
    for (var i = 0; i < docTitles.length; i++) {
      var chapter = MChapter();
      chapter.url = docTitles[i];
      chapter.name = document.selectFirst("a").xpathFirst("text()");
      chapters.add(chapter);
    }
    manga.chapters = chapters;
    return manga;
  }

  @override
  Future<MPages> search(String query, int page, FilterList filterList) async {
    return getPopular(page);
  }

  @override
  Future<List<String>> getPageList(String url) async {
    final res = (await client.get(Uri.parse(url))).body;
    return xpath(res, '//*[@id="readerarea"]/p/img/@src');
  }

  @override
  List<dynamic> getFilterList() {
    return [];
  }

  @override
  List<dynamic> getSourcePreferences() {
    return [];
  }
}

XpathMadara main(MSource source) => XpathMadara(source: source);
"#;

#[tokio::test]
async fn runner_xpath_extension() {
    let server = FixtureServer::start().await;
    let base = server.base_url();

    let port = base
        .trim_start_matches("http://")
        .rsplit(':')
        .next()
        .unwrap();
    let html = |s: &str| s.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}"));

    let dir = temp_sources_dir("xpath");
    let mut manager = manager(&dir);
    let source_id = install(
        &mut manager,
        &XPATH_FIXTURE_EXTENSION.replace("127.0.0.1:PORT", &format!("127.0.0.1:{port}")),
        &html(
            r#"{"id": 434984458, "name": "Xpath Fixture", "lang": "en", "baseUrl": "http://127.0.0.1:PORT", "version": "1.0.0", "sourceCodeUrl": "https://example.com/xpath.dart"}"#,
        ),
    );

    let source = manager.get_by_id(&source_id).expect("source installed");
    let source = mangayomi(source);

    // Popular list: three `<a>` entries scraped through the top-level
    // `xpath()` helper (the same queries the MangaHere source uses).
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
    assert_eq!(mangas.len(), 3);
    assert_eq!(mangas[0].title.as_deref(), Some("Xpath One"));
    assert_eq!(
        mangas[0].cover_url.as_ref().map(|u| u.to_string()),
        Some("https://cdn.example.com/x1.jpg".to_string())
    );
    assert_eq!(mangas[1].title.as_deref(), Some("Xpath Two"));
    assert_eq!(mangas[2].title.as_deref(), Some("Xpath Three"));
    assert_eq!(
        mangas[0].id,
        format!("{base}/xpath/manga/one"),
        "the href extracted by xpath becomes the manga id"
    );

    // Details: `.first` on a single-node `text()` result, `xpathFirst` on
    // an element and document-level `xpath` for chapter URLs.
    let manga = source
        .get_manga_details(CancellationToken::new(), format!("{base}/xpath/manga/one"))
        .unwrap();
    assert_eq!(manga.title.as_deref(), Some("Xpath One"));
    assert_eq!(
        manga.description.as_deref(),
        Some("Pirate adventure story.")
    );
    assert_eq!(manga.author.as_deref(), Some("Author Name"));
    assert_eq!(
        manga.tags.as_deref(),
        Some(&["Action".to_string(), "Adventure".to_string()][..])
    );
    assert_eq!(
        manga.status,
        PublishingStatus::Ongoing,
        "parseStatus maps 'ongoing' to Ongoing"
    );

    let chapters = source
        .get_chapter_list(CancellationToken::new(), format!("{base}/xpath/manga/one"))
        .unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].id, format!("{base}/xpath/manga/one/ch/1"));
    assert_eq!(chapters[1].id, format!("{base}/xpath/manga/one/ch/2"));

    // Chapter pages: a multi-node `@src` result becomes the page list.
    let pages = source
        .get_page_list(
            CancellationToken::new(),
            format!("{base}/xpath/manga/one"),
            chapters[0].id.clone(),
            None,
        )
        .unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages[0].image_url.as_ref().map(|u| u.to_string()),
        Some("https://cdn.example.com/p1.jpg".to_string())
    );
    assert_eq!(
        pages[1].image_url.as_ref().map(|u| u.to_string()),
        Some("https://cdn.example.com/p2.jpg".to_string())
    );

    std::fs::remove_dir_all(&dir).ok();
}
