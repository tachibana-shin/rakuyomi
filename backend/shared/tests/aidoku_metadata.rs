use std::{
    io::Write,
    sync::{mpsc, Arc},
    time::Duration,
};

use shared::{
    settings::Settings,
    source::{Source, SourceBackend},
    source_manager::SourceManager,
};

#[tokio::test]
async fn source_metadata_remains_available_while_the_wasm_operation_is_busy() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metadata.aix");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("Payload/source.json", options).unwrap();
    archive.write_all(br#"{"info":{"id":"test.metadata","lang":"en","name":"Metadata test","version":"1.0.0"}}"#).unwrap();
    archive.start_file("Payload/main.wasm", options).unwrap();
    archive
        .write_all(&wat::parse_str("(module (memory (export \"memory\") 1))").unwrap())
        .unwrap();
    archive.finish().unwrap();

    let manager = SourceManager::from_folder(dir.path().to_owned(), Settings::default()).unwrap();
    let shared_manager = Arc::new(tokio::sync::Mutex::new(manager.clone()));
    let source = Source::from_aix_file(&path, &manager, &shared_manager).unwrap();
    let backend = match &source.backend {
        SourceBackend::Aidoku(backend) => backend.clone(),
        _ => unreachable!("the fixture is an Aidoku source"),
    };
    // A source request owns this mutex until its blocking operation exits.
    let busy_operation = backend
        .lock()
        .map_err(|_| anyhow::anyhow!("source VM lock was poisoned"))?;
    let (ready_tx, ready_rx) = mpsc::channel();
    let (metadata_tx, metadata_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        metadata_tx.send(source.manifest()).unwrap();
    });
    ready_rx.recv().unwrap();
    let while_busy = metadata_rx.recv_timeout(Duration::from_secs(1));
    // Always release and join, including the failing-before case.
    drop(busy_operation);
    reader.join().unwrap();

    let manifest = while_busy.expect("metadata access blocked behind the busy source operation");
    assert_eq!(manifest.info.id, "test.metadata");
    assert_eq!(manifest.info.name, "Metadata test");
    Ok(())
}
