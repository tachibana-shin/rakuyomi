use super::*;
use std::{io::Write, sync::Arc};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::JoinSet,
};

const IMAGE: &[u8] = b"\x89PNG\r\n\x1a\nfixture-image-body";

struct Fixture {
    address: std::net::SocketAddr,
    started: Arc<Notify>,
    closed: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let started = Arc::new(Notify::new());
        let closed = Arc::new(Notify::new());
        let task = tokio::spawn({
            let started = started.clone();
            let closed = closed.clone();
            async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        connection = listener.accept() => {
                            let (stream, _) = connection.unwrap();
                            connections.spawn(serve(stream, started.clone(), closed.clone()));
                        }
                        _ = connections.join_next(), if !connections.is_empty() => {}
                    }
                }
            }
        });
        Self {
            address,
            started,
            closed,
            task,
        }
    }

    fn request(&self, path: &str) -> Request {
        let mut request = Request::new(
            Method::GET,
            format!("http://{}{path}", self.address).parse().unwrap(),
        );
        request
            .headers_mut()
            .insert("cookie", "fixture=session".parse().unwrap());
        request
            .headers_mut()
            .insert("referer", "https://fixture.invalid/reader".parse().unwrap());
        request
            .headers_mut()
            .insert("authorization", "Bearer fixture-token".parse().unwrap());
        request
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(mut stream: TcpStream, started: Arc<Notify>, closed: Arc<Notify>) {
    let mut input = Vec::new();
    let mut byte = [0];
    while !input.ends_with(b"\r\n\r\n") {
        if stream.read_exact(&mut byte).await.is_err() {
            return;
        }
        input.push(byte[0]);
    }
    let request = String::from_utf8(input).unwrap().to_ascii_lowercase();
    let path = request.split_ascii_whitespace().nth(1).unwrap();
    let authorized = request.contains("cookie: fixture=session\r\n")
        && request.contains("referer: https://fixture.invalid/reader\r\n")
        && request.contains("authorization: bearer fixture-token\r\n");
    if path == "/post" {
        let mut body = [0; 4];
        stream.read_exact(&mut body).await.unwrap();
        let valid = request.starts_with("post ") && body == *b"data" && authorized;
        let response: &[u8] = if valid {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
        } else {
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        };
        let _ = stream.write_all(response).await;
    } else if (!authorized && !path.starts_with("/chapter/")) || path == "/forbidden" {
        let _ = stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    } else if path == "/redirect" {
        let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /image\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    } else if path.ends_with("/slow") {
        let _ = stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\npartial",
            )
            .await;
        started.notify_one();
        let _ = stream.read(&mut byte).await;
        closed.notify_one();
    } else if path == "/truncated" {
        let _ = stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\npartial",
            )
            .await;
    } else {
        let (encoding, payload) = if path == "/deflate" {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(IMAGE).unwrap();
            ("deflate", encoder.finish().unwrap())
        } else {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(IMAGE).unwrap();
            ("gzip", encoder.finish().unwrap())
        };
        let header = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Encoding: {encoding}\r\nContent-Length: {}\r\nX-Image-Context: one\r\nX-Image-Context: two\r\nConnection: close\r\n\r\n", payload.len());
        let _ = stream.write_all(header.as_bytes()).await;
        let _ = stream.write_all(&payload).await;
    }
}

fn downloader(directory: &std::path::Path) -> ImageDownloader {
    ImageDownloader {
        program: "aria2c".into(),
        temporary_root: directory.to_owned(),
        custom_request_client: OnceCell::new(),
    }
}

fn assert_clean(directory: &std::path::Path) {
    assert!(
        std::fs::read_dir(directory).unwrap().next().is_none(),
        "private download files survived the request"
    );
}

#[tokio::test]
#[ignore = "requires real aria2c 1.37 on PATH"]
async fn authenticated_redirect_preserves_final_headers_and_decoded_bytes() {
    let fixture = Fixture::start().await;
    let directory = tempfile::tempdir().unwrap();
    let downloader = downloader(directory.path());
    let token = CancellationToken::new();
    let response = downloader
        .fetch_with_aria2(&token, fixture.request("/redirect"), None)
        .await
        .unwrap();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.bytes, IMAGE);
    assert_eq!(
        response
            .headers
            .get_all("x-image-context")
            .iter()
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert!(!response
        .headers
        .contains_key(reqwest::header::CONTENT_ENCODING));
    assert_clean(directory.path());
    let deflated = downloader
        .fetch_with_aria2(&token, fixture.request("/deflate"), None)
        .await
        .unwrap();
    assert_eq!(deflated.bytes, IMAGE);
    assert!(!deflated
        .headers
        .contains_key(reqwest::header::CONTENT_ENCODING));
    assert!(!deflated
        .headers
        .contains_key(reqwest::header::CONTENT_LENGTH));
    assert_clean(directory.path());
    let forbidden = downloader
        .fetch_with_aria2(&token, fixture.request("/forbidden"), None)
        .await
        .unwrap();
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert!(forbidden.error_for_status().is_err());
    assert_clean(directory.path());
}

#[tokio::test]
#[ignore = "requires real aria2c 1.37 on PATH"]
async fn cancellation_and_timeout_close_transfer_and_remove_partial_files() {
    let fixture = Fixture::start().await;
    let directory = tempfile::tempdir().unwrap();
    let downloader = downloader(directory.path());
    let token = CancellationToken::new();
    let transfer = downloader.fetch_with_aria2(&token, fixture.request("/slow"), None);
    tokio::pin!(transfer);
    tokio::select! {
        _ = fixture.started.notified() => token.cancel(),
        result = &mut transfer => panic!("download completed before cancellation: {:?}", result.err()),
        _ = tokio::time::sleep(Duration::from_secs(3)) => panic!("aria2 did not request the image"),
    }
    assert!(transfer
        .await
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
    tokio::time::timeout(Duration::from_secs(2), fixture.closed.notified())
        .await
        .unwrap();
    assert_clean(directory.path());

    let mut request = fixture.request("/slow");
    *request.timeout_mut() = Some(Duration::from_millis(300));
    assert!(downloader
        .fetch_with_aria2(&CancellationToken::new(), request, None)
        .await
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    tokio::time::timeout(Duration::from_secs(2), fixture.closed.notified())
        .await
        .unwrap();
    assert_clean(directory.path());

    let mut request = fixture.request("/truncated");
    *request.timeout_mut() = Some(Duration::from_secs(2));
    assert!(
        downloader
            .fetch_with_aria2(&CancellationToken::new(), request, None)
            .await
            .is_err(),
        "a truncated 200 response became a complete page"
    );
    assert_clean(directory.path());
}

#[tokio::test]
async fn source_post_body_uses_compatible_transport() {
    let fixture = Fixture::start().await;
    let directory = tempfile::tempdir().unwrap();
    let downloader = downloader(directory.path());
    let mut request = fixture.request("/post");
    *request.method_mut() = Method::POST;
    *request.body_mut() = Some("data".into());
    let response = downloader
        .fetch(&CancellationToken::new(), request)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(response.bytes, b"ok");
    assert_clean(directory.path());
}

fn fixture_source(directory: &std::path::Path) -> crate::source::Source {
    let file = std::fs::File::create(directory.join("fixture.aix")).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    archive.start_file("Payload/source.json", options).unwrap();
    archive.write_all(br#"{"info":{"id":"test.download","lang":"en","name":"Download Fixture","version":"1.0.0"}}"#).unwrap();
    archive.start_file("Payload/main.wasm", options).unwrap();
    archive
        .write_all(&wat::parse_str(r#"(module (memory (export "memory") 1))"#).unwrap())
        .unwrap();
    archive.finish().unwrap();
    let manager = crate::source_manager::SourceManager::from_folder(
        directory.to_owned(),
        crate::settings::Settings::default(),
    )
    .unwrap();
    let shared_manager = Arc::new(tokio::sync::Mutex::new(manager.clone()));
    crate::source::Source::from_aix_file(&directory.join("fixture.aix"), &manager, &shared_manager)
        .unwrap()
}

#[tokio::test]
#[ignore = "requires real aria2c 1.37 on PATH"]
async fn chapter_packages_exact_pages_and_reports_progress() {
    use crate::{
        cbz_metadata::ComicInfo,
        chapter_downloader::download_chapter_pages_as_cbz,
        model::{ChapterId, MangaId, SourceId},
        source::model::Page,
    };
    use std::{
        io::{Cursor, Read},
        sync::atomic::{AtomicUsize, Ordering},
    };
    let fixture = Fixture::start().await;
    let directory = tempfile::tempdir().unwrap();
    let source = fixture_source(directory.path());
    let chapter = ChapterId::new(
        MangaId::new(SourceId::new("test.download".into()), "fixture".into()),
        "chapter".into(),
    );
    let pages = (0..2)
        .map(|index| Page {
            index,
            image_url: Some(fixture.request("/chapter/image").url().clone()),
            ..Page::default()
        })
        .collect();
    let done = Arc::new(AtomicUsize::new(0));
    let total = Arc::new(AtomicUsize::new(0));
    let mut output = Cursor::new(Vec::new());
    let errors = download_chapter_pages_as_cbz(
        &CancellationToken::new(),
        &mut output,
        ComicInfo {
            title: "Fixture Chapter".into(),
            page_count: 2,
            ..ComicInfo::default()
        },
        &source,
        pages,
        2,
        false,
        Some(Arc::new({
            let done = done.clone();
            let total = total.clone();
            move |completed, count| {
                done.store(completed as usize, Ordering::SeqCst);
                total.store(count as usize, Ordering::SeqCst);
            }
        })),
        &chapter,
    )
    .await
    .unwrap();
    assert!(
        errors.is_empty(),
        "chapter contains failed pages: {errors:?}"
    );
    assert_eq!(done.load(Ordering::SeqCst), 2);
    assert_eq!(total.load(Ordering::SeqCst), 2);
    let mut archive = zip::ZipArchive::new(output).unwrap();
    for name in ["00.png", "01.png"] {
        let mut bytes = Vec::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, IMAGE);
    }
    let mut xml = String::new();
    archive
        .by_name("ComicInfo.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let metadata: ComicInfo = quick_xml::de::from_str(&xml).unwrap();
    assert_eq!(metadata.title, "Fixture Chapter");
    assert_eq!(metadata.page_count, 2);
}

#[tokio::test]
#[ignore = "requires real aria2c 1.37 on PATH"]
async fn dropping_chapter_download_stops_its_page_transfer() {
    use crate::{
        cbz_metadata::ComicInfo,
        chapter_downloader::download_chapter_pages_as_cbz,
        model::{ChapterId, MangaId, SourceId},
        source::model::Page,
    };
    let fixture = Fixture::start().await;
    let directory = tempfile::tempdir().unwrap();
    let source = fixture_source(directory.path());
    let chapter = ChapterId::new(
        MangaId::new(SourceId::new("test.download".into()), "fixture".into()),
        "chapter".into(),
    );
    let token = CancellationToken::new();
    let pages = vec![Page {
        image_url: Some(fixture.request("/chapter/slow").url().clone()),
        ..Page::default()
    }];
    let task = tokio::spawn(async move {
        download_chapter_pages_as_cbz(
            &token,
            std::io::Cursor::new(Vec::new()),
            ComicInfo::default(),
            &source,
            pages,
            1,
            false,
            None,
            &chapter,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), fixture.started.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    // Dropping the parent must abort its producer and kill aria2, not leave a
    // detached worker downloading the rest of the chapter in the background.
    tokio::time::timeout(Duration::from_secs(2), fixture.closed.notified())
        .await
        .unwrap();
}
