//! Offline regression tests for global language changes reaching Aidoku sources.
//! The test source returns defaults.get("languages") in Manga.authors, allowing
//! the test to observe the actual WASM import without HTTP requests.

#![cfg(feature = "all")]

use std::{io::Write, sync::Arc};

use shared::{
    model::SourceId,
    settings::{Settings, SourceList},
    source::{Source, SourceBackend},
    source_manager::SourceManager,
};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const SOURCE_ID: &str = "test.languages";
const WAT: &str = r#"
(module
  (import "defaults" "get" (func $get (param i32 i32) (result i32)))
  (import "std" "buffer_len" (func $len (param i32) (result i32)))
  (import "std" "read_buffer" (func $read (param i32 i32 i32) (result i32)))
  (import "std" "destroy" (func $destroy (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "languages")
  ;; Postcard Manga: empty key/title, no cover/artists, Some(authors).
  (data (i32.const 108) "\00\00\00\00\01")
  (func (export "start"))
  (func (export "free_result") (param i32))
  (func (export "get_manga_update") (param i32 i32 i32) (result i32)
    (local $rid i32) (local $size i32)
    (local.set $rid (call $get (i32.const 0) (i32.const 9)))
    (local.set $size (call $len (local.get $rid)))
    ;; Insert the encoded Vec<String> as authors, then nine empty/default fields.
    (drop (call $read (local.get $rid) (i32.const 113) (local.get $size)))
    (memory.fill (i32.add (i32.const 113) (local.get $size)) (i32.const 0) (i32.const 9))
    (call $destroy (local.get $rid))
    ;; Aidoku result header: length including the eight-byte header, capacity.
    (i32.store (i32.const 100) (i32.add (local.get $size) (i32.const 22)))
    (i32.store (i32.const 104) (i32.add (local.get $size) (i32.const 22)))
    (i32.const 100)))
"#;

fn setup() -> (Arc<Mutex<SourceManager>>, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    archive.start_file("Payload/source.json", options).unwrap();
    archive.write_all(br#"{"info":{"id":"test.languages","name":"Languages","version":1,"languages":["en","pt","pt-BR"],"minAppVersion":"0.7.1"}}"#).unwrap();
    archive.start_file("Payload/main.wasm", options).unwrap();
    archive.write_all(&wat::parse_str(WAT).unwrap()).unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let settings = Settings {
        languages: vec!["en".into()],
        ..Default::default()
    };
    let manager = Arc::new(Mutex::new(
        SourceManager::from_folder(dir.path().to_path_buf(), settings).unwrap(),
    ));
    manager
        .try_lock()
        .unwrap()
        .install_source(
            &SourceId::new(SOURCE_ID.into()),
            bytes,
            "test".into(),
            &manager,
        )
        .unwrap();
    (manager, dir)
}

async fn source(manager: &Arc<Mutex<SourceManager>>) -> Source {
    manager.lock().await.sources_by_id[&SourceId::new(SOURCE_ID.into())].clone()
}

async fn read_languages(source: &Source) -> Vec<String> {
    source
        .get_manga_update_next(
            CancellationToken::new(),
            aidoku::Manga::default(),
            false,
            true,
        )
        .await
        .unwrap()
        .authors
        .unwrap()
}

async fn change_languages(manager: &Arc<Mutex<SourceManager>>, languages: &[&str]) {
    let mut guard = manager.lock().await;
    let mut settings = guard.settings.clone();
    settings.languages = languages.iter().map(|lang| (*lang).into()).collect();
    guard.update_settings(settings, manager).unwrap();
}

#[tokio::test]
async fn language_changes_reach_already_booted_aidoku_sources() {
    let (manager, _dir) = setup();
    assert_eq!(read_languages(&source(&manager).await).await, ["en"]);
    for languages in [&["en", "pt", "pt-br"][..], &["pt-br"], &[]] {
        change_languages(&manager, languages).await;
        assert_eq!(read_languages(&source(&manager).await).await, languages);
    }
}

#[tokio::test]
async fn language_changes_before_first_boot_reach_aidoku_sources() {
    let (manager, _dir) = setup();
    change_languages(&manager, &["pt-br"]).await;
    assert_eq!(read_languages(&source(&manager).await).await, ["pt-br"]);
}

#[tokio::test]
async fn unrelated_global_changes_preserve_loaded_aidoku_sources() {
    let (manager, _dir) = setup();
    let before = source(&manager).await;
    assert_eq!(read_languages(&before).await, ["en"]);
    {
        let mut guard = manager.lock().await;
        let mut settings = guard.settings.clone();
        settings.source_lists.push(SourceList {
            url: "https://example.com/index.json".parse().unwrap(),
            source_type: Default::default(),
        });
        guard.update_settings(settings, &manager).unwrap();
    }
    let after = source(&manager).await;
    match (&before.backend, &after.backend) {
        (SourceBackend::Aidoku(before), SourceBackend::Aidoku(after)) => {
            assert!(Arc::ptr_eq(before, after));
        }
        _ => panic!("expected Aidoku sources"),
    }
    assert_eq!(read_languages(&after).await, ["en"]);
}
