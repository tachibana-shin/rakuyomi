//! Offline runner tests for LNReader plugins.
//!
//! A synthetic fixture plugin is served against a local HTTP server, so the
//! full runner pipeline (plugin load, invoke dispatch, libs bindings, storage
//! seeding, filters, url resolution, image requests) is verified without
//! touching the network. The live-site end-to-end smoke test lives in
//! `lnreader_source.rs`.

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use shared::{
    settings::Settings, source::lnreader::LnReaderSource, source::model::PublishingStatus,
    source::SourceBackend, source_collection::SourceCollection, source_manager::SourceManager,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

fn lnreader(source: &shared::source::Source) -> &LnReaderSource {
    match &source.backend {
        SourceBackend::LnReader(lnreader) => lnreader.as_ref(),
        _ => panic!("expected an LNReader source"),
    }
}

// ---------------------------------------------------------------------------
// Minimal fixture HTTP server
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
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
        // Bind inside the server thread: a listener created on the test's
        // current-thread runtime would have its readiness tracked by that
        // runtime's driver, which is not polled while a synchronous plugin
        // invoke is running, so the accept loop would never fire.
        let (addr_tx, addr_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handle = {
            let requests = requests.clone();
            std::thread::Builder::new()
                .name("lnreader-fixture-server".to_string())
                .spawn(move || {
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    runtime.block_on(async move {
                        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                        let _ = addr_tx.send(listener.local_addr().unwrap());
                        loop {
                            let (stream, _peer) = match listener.accept().await {
                                Ok(ok) => ok,
                                Err(_) => break,
                            };
                            let requests = requests.clone();
                            tokio::spawn(async move {
                                handle_connection(stream, &requests).await;
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
        format!("http://{}/", self.addr)
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

async fn handle_connection(mut stream: TcpStream, requests: &Arc<Mutex<Vec<RecordedRequest>>>) {
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
        method,
        path: path.clone(),
        query,
        body,
    });

    let (status, content_type, response) = route(&path);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(&response).await;
}

fn route(path: &str) -> (&'static str, &'static str, Vec<u8>) {
    let html = |content: &str| {
        (
            "200 OK",
            "text/html; charset=utf-8",
            content.as_bytes().to_vec(),
        )
    };
    match path {
        "/popular" => html(
            r#"<!DOCTYPE html><html><body>
<article><div class="series-genres"><a>Manhua</a></div>
<h2 class="novel-title"><a href="http://fixture/book/alpha">Alpha</a></h2>
<div class="novel-cover"><img data-breeze="http://fixture/cover/alpha.jpg"></div></article>
<article>
<h2 class="novel-title"><a href="http://fixture/book/beta">Beta</a></h2>
<div class="novel-cover"><img></div></article>
</body></html>"#,
        ),
        "/search" => (
            "200 OK",
            "application/json; charset=utf-8",
            br#"[{"name":"Abyss","path":"book/abyss"},{"name":"Abyss 2","path":"book/abyss2"}]"#
                .to_vec(),
        ),
        p if p.starts_with("/novel/") => html(
            r#"<html><body><h1>Alpha</h1><img class="cover" src="/cover/alpha.jpg">
<div class="summary">A test novel</div>
<span class="author">Auth A</span><span class="artist">Art B</span>
<span class="genres">Action, Romance</span><span class="status">Ongoing</span>
<div class="chapter-item"><a href="/book/alpha/1" data-time="2 hours ago">Ch 1</a></div>
<div class="chapter-item"><a href="/book/alpha/2" data-time="epoch">Ch 2</a></div>
<div class="chapter-item"><a href="/book/alpha/3" data-time="2024-01-15T10:30:00Z">Ch 3</a></div>
<div class="chapter-item"><a href="/book/alpha/4">Ch 4</a></div>
<div class="chapter-item"><a href="/book/alpha/5">Ch 5</a></div>
</body></html>"#,
        ),
        p if p.starts_with("/chapter/") => {
            html(r#"<div id="novel-content"><p>Chapter text</p></div>"#)
        }
        "/echo" => ("200 OK", "text/plain; charset=utf-8", b"ok".to_vec()),
        _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
    }
}

// ---------------------------------------------------------------------------
// Fixture plugins (site substituted with the live server port at runtime)
// ---------------------------------------------------------------------------

const FIXTURE_PLUGIN: &str = r#"var cheerio = require('cheerio');
var urlencode = require('urlencode');
var { gcm } = require('@libs/aes');
var { storage } = require('@libs/storage');
var { fetchApi, fetchProto } = require('@libs/fetch');
var { utf8ToBytes, bytesToUtf8 } = require('@libs/utils');
var { NovelStatus } = require('@libs/novelStatus');
var { FilterTypes } = require('@libs/filterInputs');
var { isUrlAbsolute } = require('@libs/isAbsoluteUrl');
var { defaultCover } = require('@libs/defaultCover');

var SITE = 'http://127.0.0.1:PORT/';
var AES_KEY = Uint8Array.from(atob('QkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkI='), function (c) { return c.charCodeAt(0); });
var AES_IV = Uint8Array.from(atob('JCQkJCQkJCQkJCQk'), function (c) { return c.charCodeAt(0); });
var AES_CT = atob('R/CvNLCpq1cLjEfJz0vj0kGUWKh230TT34Ii34T/GKXAahw=');

exports.default = {
  id: 'testplugin',
  name: 'Offline Test Plugin',
  site: SITE,
  version: '1.2.3',
  icon: 'icon.png',
  webStorageUtilized: true,
  filters: {
    status: { type: 'Picker', value: 0, options: [ { value: 0, label: 'All' }, { value: 1, label: 'Ongoing' } ] },
    genre: { type: 'Text', value: 'romance' },
  },
  pluginSettings: {
    show_images: { value: true },
    note: { value: 'hello' },
  },
  imageRequestInit: {
    method: 'POST',
    headers: { 'X-Test': '1' },
    body: 'abc',
  },
  resolveUrl: function (path, isNovel) {
    return SITE + (isNovel ? 'n/' : 'c/') + path;
  },
  popularNovels: async function (page, options) {
    var q = 'page=' + page + '&latest=' + (options.showLatestNovels ? 1 : 0) +
      '&status=' + options.filters.status.value +
      '&genre=' + encodeURIComponent(options.filters.genre.value);
    var text = await fetchApi(SITE + 'popular?' + q).then(function (r) { return r.text(); });
    var $ = cheerio.load(text);
    return $('article').map(function (i, el) {
      var a = $(el).find('h2 a');
      var href = a.attr('href');
      if (!href) return null;
      return {
        name: a.text(),
        path: href.replace(SITE, '').replace(/^\//, '').replace(/\/$/, ''),
        cover: $(el).find('img').attr('data-breeze') || defaultCover,
      };
    }).get().filter(Boolean);
  },
  searchNovels: async function (query, page) {
    var encoded = urlencode(query);
    var data = await fetchApi(SITE + 'search?q=' + encoded + '&page=' + page).then(function (r) { return r.json(); });
    return data.map(function (n) { return { name: n.name, path: n.path, cover: null }; });
  },
  parseNovel: async function (path) {
    var html = await fetchApi(SITE + 'novel/' + path).then(function (r) { return r.text(); });
    var $ = cheerio.load(html);
    var chapters = $('div.chapter-item > a').map(function (i, el) {
      var $el = $(el);
      var href = $el.attr('href');
      if (!href) return null;
      var t = $el.attr('data-time');
      if (t === 'epoch') t = 1705300000;
      return {
        name: $el.text().trim(),
        path: href.replace(SITE, '').replace(/^\//, '').replace(/\/$/, ''),
        releaseTime: t || undefined,
        chapterNumber: i + 1,
      };
    }).get().filter(Boolean);
    return {
      path: path,
      name: $('h1').text(),
      cover: $('img.cover').attr('src'),
      summary: $('div.summary').text(),
      author: $('span.author').text(),
      artist: $('span.artist').text(),
      genres: $('span.genres').text(),
      status: $('span.status').text(),
      totalPages: 2,
      chapters: chapters,
    };
  },
  parsePage: async function (path, page) {
    var novel = await this.parseNovel(path);
    return { chapters: novel.chapters.slice(3, 5) };
  },
  parseChapter: async function (path) {
    var plain = new TextDecoder().decode(gcm(AES_KEY, AES_IV).decrypt(Uint8Array.from(AES_CT, function (c) { return c.charCodeAt(0); })));
    var markers = [];
    if (plain === 'RakuYomi-decrypt-ok') markers.push('aes-ok');
    if (storage.get('show_images') === true) markers.push('seed-ok');
    storage.set('flag', 'set');
    if (storage.get('flag') === 'set') markers.push('flag-ok');
    if (isUrlAbsolute('http://x.com') && !isUrlAbsolute('/x')) markers.push('abs-ok');
    if (bytesToUtf8(utf8ToBytes('u')) === 'u') markers.push('utils-ok');
    if (btoa('RakuYomi') === 'UmFrdVlvbWk=' && atob('UmFrdVlvbWk=') === 'RakuYomi') markers.push('b64-ok');
    if (NovelStatus.Ongoing === 'Ongoing') markers.push('status-ok');
    if (FilterTypes.Picker === 'Picker') markers.push('picker-ok');
    var protoError = '';
    try { await fetchProto(SITE + 'chapter/' + path); } catch (e) { protoError = String(e.message || e); }
    if (protoError.indexOf('not supported') !== -1) markers.push('proto-ok');
    var echo = '';
    try {
      var fd = new FormData();
      fd.append('a', '1');
      fd.append('b', new Blob(['hi'], { type: 'text/plain' }));
      echo = await fetchApi(SITE + 'echo', { method: 'POST', body: fd }).then(function (r) { return r.text(); });
    } catch (e) { echo = 'echo-FAIL'; }
    if (echo === 'ok') markers.push('form-ok');
    var html = await fetchApi(SITE + 'chapter/' + path).then(function (r) { return r.text(); });
    return html + '<script data-markers="' + markers.join('|') + '"></script>';
  },
};
"#;

const FALLBACK_PLUGIN: &str = r#"exports.default = {
  id: 'fallbackplugin',
  name: 'Fallback Plugin',
  site: 'http://127.0.0.1:PORT/',
  version: '0.9.0',
  popularNovels: async function () { return []; },
  parseNovel: async function (path) {
    return {
      path: path,
      name: 'Fallback',
      chapters: [{ name: 'F1', path: 'book/f/1', chapterNumber: 1 }],
    };
  },
  parseChapter: async function (path) {
    return '<div id="novel-content"><p>fallback chapter</p></div>';
  },
};
"#;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn temp_sources_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rakuyomi-lnreader-{tag}-{}-{}",
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

fn install(manager: &mut SourceManager, id: &str, code: &str) -> shared::model::SourceId {
    let source_id = shared::model::SourceId::new(id.to_string());
    manager
        .install_lnreader_source(&source_id, code.as_bytes(), "LNReader".to_string())
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
    let plugin_code = FIXTURE_PLUGIN.replace(
        "PORT",
        base.trim_end_matches('/').rsplit(':').next().unwrap(),
    );

    let dir = temp_sources_dir("full");
    let mut manager = manager(&dir);
    let source_id = install(&mut manager, "testplugin", &plugin_code);

    let source = manager.get_by_id(&source_id).expect("source installed");
    let manifest = source.manifest();
    assert_eq!(manifest.info.id, "testplugin");
    assert_eq!(manifest.info.name, "Offline Test Plugin");
    assert_eq!(manifest.info.version, "1.2.3");

    // popular with the declared setting defaults (status picker default 0)
    let (mangas, _) = source
        .search_mangas(CancellationToken::new(), String::new(), 1)
        .await
        .unwrap();
    assert_eq!(mangas.len(), 2, "two novels in the fixture list");
    assert_eq!(
        server.query_for("/popular").expect("popular request"),
        "page=1&latest=0&status=0&genre=romance"
    );

    // Picker coercion: "Ongoing" -> index 1
    let v = match lnreader(source).invoke(
        "popular",
        serde_json::json!([1, { "status": "Ongoing", "genre": "romance" }, false]),
    ) {
        Ok(v) => v,
        Err(e) => panic!("Picker invoke failed: {e:#}"),
    };
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(
        server.query_for("/popular"),
        Some("page=1&latest=0&status=1&genre=romance".to_string()),
        "Picker label must be coerced to its index"
    );

    // showLatestNovels flag
    let v = lnreader(source)
        .invoke("popular", serde_json::json!([1, {}, true]))
        .unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert!(!v.as_array().unwrap().first().unwrap()["name"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(
        server.query_for("/popular"),
        Some("page=1&latest=1&status=0&genre=romance".to_string()),
        "showLatestNovels must flow into the plugin options"
    );

    // search with urlencode
    let (results, _) = source
        .search_mangas(CancellationToken::new(), "abyss & x".to_string(), 1)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(
        server.query_for("/search").unwrap(),
        "q=abyss%20%26%20x&page=1",
        "query must be urlencoded by @libs/urlencode"
    );

    // novel details
    let manga = source
        .get_manga_details(CancellationToken::new(), "book/alpha".to_string())
        .await
        .unwrap();
    assert_eq!(manga.title.as_deref(), Some("Alpha"));
    assert_eq!(
        manga.id.as_str(),
        "book/alpha",
        "details must round-trip the novel id through `path`"
    );
    assert_eq!(manga.description.as_deref(), Some("A test novel"));
    assert_eq!(manga.author.as_deref(), Some("Auth A"));
    assert_eq!(manga.artist.as_deref(), Some("Art B"));
    assert_eq!(
        manga
            .tags
            .as_ref()
            .map(|tags| tags.iter().map(String::as_str).collect::<Vec<_>>()),
        Some(vec!["Action", "Romance"])
    );
    assert_eq!(manga.status, PublishingStatus::Ongoing);
    // the fixture cover is a relative path; it must be resolved against the site
    assert_eq!(
        manga.cover_url.map(|u| u.to_string()).as_deref(),
        Some(format!("{base}cover/alpha.jpg").as_str())
    );

    // chapter list: 5 from parseNovel + 2 from parsePage (page 2)
    let chapters = source
        .get_chapter_list(CancellationToken::new(), "book/alpha".to_string())
        .await
        .unwrap();
    assert_eq!(chapters.len(), 7, "novel chapters plus parsePage page 2");
    let ch1 = &chapters[0];
    let ch5 = &chapters[6];
    assert_eq!(ch1.title.as_deref(), Some("Ch 1"));
    assert_eq!(ch5.title.as_deref(), Some("Ch 5"));
    assert!(
        ch1.date_uploaded.is_some(),
        "relative release time must be parsed"
    );
    assert!(
        ch1.date_uploaded.unwrap().timestamp() > chrono::Utc::now().timestamp() - 3 * 3600,
        "2 hours ago must resolve to a recent timestamp"
    );
    assert!(
        chapters[1].date_uploaded.is_some(),
        "epoch release time must be parsed"
    );
    assert_eq!(
        chapters[1].date_uploaded.unwrap().timestamp(),
        1_705_300_000
    );
    assert!(
        chapters[2].date_uploaded.is_some(),
        "ISO release time must be parsed"
    );
    assert!(
        chapters[3].date_uploaded.is_none(),
        "chapter without releaseTime stays None"
    );
    assert_eq!(ch1.chapter_num, Some(1.0));
    assert_eq!(
        ch1.url.clone().map(|u| u.to_string()).unwrap(),
        base.clone() + "c/book/alpha/1"
    );
    assert_eq!(
        ch5.url.clone().map(|u| u.to_string()).unwrap(),
        base.clone() + "c/book/alpha/5"
    );

    // chapter HTML + libs markers
    let pages = source
        .get_page_list(
            CancellationToken::new(),
            "book/alpha".to_string(),
            ch1.id.clone(),
            ch1.chapter_num,
        )
        .await
        .unwrap();
    assert_eq!(pages.len(), 1);
    let text = pages[0].text.as_ref().unwrap();
    assert!(text.starts_with("<!-- html -->\n"));
    for marker in [
        "aes-ok",
        "seed-ok",
        "flag-ok",
        "abs-ok",
        "utils-ok",
        "b64-ok",
        "status-ok",
        "picker-ok",
        "proto-ok",
        "form-ok",
    ] {
        assert!(
            text.contains(marker),
            "chapter HTML must contain lib marker {marker}"
        );
    }

    // FormData POST reached the server with a multipart body
    let echo = server
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path == "/echo")
        .expect("FormData POST recorded");
    assert!(echo.body.windows(2).any(|w| w == b"--"), "multipart body");
    assert!(
        String::from_utf8_lossy(&echo.body).contains("name=\"a\""),
        "multipart must contain the string field"
    );

    // image request from imageRequestInit
    let request = source
        .get_image_request(
            url::Url::parse(&format!("{base}cover/alpha.jpg")).unwrap(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(
        request.headers().get("x-test").map(|v| v.to_str().unwrap()),
        Some("1")
    );
    assert_eq!(
        request.body().and_then(|b| b.as_bytes()),
        Some(b"abc".as_slice())
    );

    // error paths
    assert!(
        lnreader(source)
            .invoke("bogus", serde_json::json!([]))
            .is_err(),
        "unknown plugin method must fail"
    );
    assert!(
        source
            .process_page_image(
                CancellationToken::new(),
                (
                    url::Url::parse("http://x/i.jpg").unwrap(),
                    Default::default()
                ),
                (reqwest::StatusCode::OK, Default::default()),
                tokio_util::bytes::Bytes::new(),
                None,
            )
            .await
            .is_err(),
        "process_page_image is unsupported for LNReader plugins"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn runner_url_fallback_without_resolve_url() {
    let server = FixtureServer::start().await;
    let base = server.base_url();
    let port = base
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .unwrap()
        .to_string();
    let plugin_code = FALLBACK_PLUGIN.replace("PORT", &port);

    let dir = temp_sources_dir("fallback");
    let mut manager = manager(&dir);
    let source_id = install(&mut manager, "fallbackplugin", &plugin_code);
    let source = manager.get_by_id(&source_id).unwrap();

    let chapters = source
        .get_chapter_list(CancellationToken::new(), "book/f".to_string())
        .await
        .unwrap();
    assert_eq!(chapters.len(), 1);
    assert_eq!(
        chapters[0].url.clone().map(|u| u.to_string()).unwrap(),
        format!("{base}book/f/1"),
        "chapter url must fall back to site + path"
    );

    let pages = source
        .get_page_list(
            CancellationToken::new(),
            "book/f".to_string(),
            chapters[0].id.clone(),
            chapters[0].chapter_num,
        )
        .await
        .unwrap();
    assert!(pages[0].text.as_ref().unwrap().contains("fallback chapter"));

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn runner_rejects_invalid_plugins() {
    let server = FixtureServer::start().await;
    let _ = server;

    let dir = temp_sources_dir("invalid");
    let mut manager = manager(&dir);

    // not valid JavaScript at all
    let source_id = shared::model::SourceId::new("broken".to_string());
    assert!(manager
        .install_lnreader_source(&source_id, b"this is not js !!!", "LNReader".to_string())
        .is_err());

    // valid JS but no default export
    let source_id = shared::model::SourceId::new("nodefault".to_string());
    assert!(manager
        .install_lnreader_source(
            &source_id,
            b"module.exports = { some: 'thing' };",
            "LNReader".to_string(),
        )
        .is_err());

    std::fs::remove_dir_all(&dir).ok();
}
