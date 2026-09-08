//! Offline regression test for the stale `Source::features` copy (blank
//! pages on Aidoku sources with encrypted images, e.g. MangaPlus).
//!
//! Commit a2d95f1 made the WASM engine boot lazy and moved the
//! `process_page_image` capability detection into
//! `BlockingSource::ensure_booted`, which updated only the inner
//! `BlockingSource`. The outer `Source` kept the load-time snapshot
//! (`false`), and the chapter downloader reads that outer copy, so every
//! Aidoku source that needs `process_page_image` (MangaPlus, mangago,
//! jmcomic, ...) saved encrypted bytes straight into CBZ files.
//!
//! This test builds two minimal `.aix` archives in-memory (one exporting
//! `process_page_image`, one not), boots both engines and asserts the flag
//! seen by the downloader (`Source::features.process_page_image()`) mirrors
//! the inner detection after the lazy boot.

use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use shared::{
    settings::Settings,
    source::{Source, SourceBackend},
    source_manager::SourceManager,
};
use url::Url;

const WAT_WITH_PROCESS_PAGE_IMAGE: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "process_page_image") (param i32 i32) (result i32)
    i32.const 0))
"#;

const WAT_WITHOUT_PROCESS_PAGE_IMAGE: &str = r#"
(module
  (memory (export "memory") 1))
"#;

fn write_aix(dir: &Path, id: &str, wat: &str) -> PathBuf {
    let wasm_bytes = wat::parse_str(wat).expect("valid wat module");
    let path = dir.join(format!("{id}.aix"));
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("Payload/source.json", opts).unwrap();
    zip.write_all(
        br#"{"info": {"id": "test.process_unit", "lang": "en", "name": "Test Process Unit", "version": "1.0.0"}}"#,
    )
    .unwrap();
    zip.start_file("Payload/main.wasm", opts).unwrap();
    zip.write_all(&wasm_bytes).unwrap();
    zip.finish().unwrap();
    path
}

fn setup(wat: &str, id: &str) -> Source {
    let dir = std::env::temp_dir().join(format!("rakuyomi-process-page-image-{id}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    let aix = write_aix(&dir, id, wat);

    let manager = SourceManager::from_folder(dir, Settings::default()).unwrap();
    let arc_manager = Arc::new(tokio::sync::Mutex::new(manager.clone()));
    // The source list is empty: `from_aix_file` is called directly on the
    // freshly written archive.
    Source::from_aix_file(&aix, &manager, &arc_manager).unwrap()
}

/// Boots the engine through a public wrapper that calls `ensure_booted`
/// first and then intentionally fails on the missing `get_image_request`
/// export, so a minimal wasm module suffices.
async fn boot(source: &Source) {
    let result = source
        .get_image_request(Url::parse("https://example.com/1.jpg").unwrap(), None)
        .await;
    assert!(
        result.is_err(),
        "expected missing get_image_request export on the minimal module"
    );
}

#[tokio::test]
async fn process_page_image_flag_mirrors_wasm_exports_after_lazy_boot() {
    // 1. Module exporting `process_page_image` (the MangaPlus shape).
    let source = setup(WAT_WITH_PROCESS_PAGE_IMAGE, "with-export");

    // Pre-boot snapshot: the outer copy starts false, like the inner one.
    assert!(!source.features.process_page_image());

    // Boot the engine and run the export detection.
    boot(&source).await;

    let inner = match &source.backend {
        SourceBackend::Aidoku(backend) => backend.lock().unwrap_or_else(|e| e.into_inner()),
        _ => panic!("expected an aidoku backend"),
    };
    assert!(
        inner.features.process_page_image(),
        "inner BlockingSource must detect the process_page_image export after boot"
    );
    drop(inner);
    assert!(
        source.features.process_page_image(),
        "outer Source::features must mirror the inner detection after lazy boot \
         (stale load-time copy regression: the chapter downloader reads this)"
    );

    // 2. Module without the export (plain sources must stay on the raw
    //    pass-through path, never running the wasm round-trip).
    let source = setup(WAT_WITHOUT_PROCESS_PAGE_IMAGE, "without-export");
    boot(&source).await;

    let inner = match &source.backend {
        SourceBackend::Aidoku(backend) => backend.lock().unwrap_or_else(|e| e.into_inner()),
        _ => panic!("expected an aidoku backend"),
    };
    assert!(
        !inner.features.process_page_image(),
        "module without the export must keep the flag false"
    );
    drop(inner);
    assert!(!source.features.process_page_image());
}
