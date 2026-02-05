#![allow(unreachable_code)]
#![allow(unreachable_patterns)]
#![allow(unreachable_code)]

#[allow(unused_imports)]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use futures::executor;
use pprof::criterion::{Output, PProfProfiler};
use shared::{
    cbz_metadata::ComicInfo, chapter_downloader::download_chapter_pages_as_cbz, settings::Settings,
    source::Source, source_manager::SourceManager,
};
use std::{collections::HashMap, env, io, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[allow(unused)]
pub fn chapter_downloader_benchmark(c: &mut Criterion) {
    let source_path: PathBuf = env::var("BENCHMARK_SOURCE_PATH").unwrap().into();
    let manga_id = env::var("BENCHMARK_MANGA_ID").unwrap();
    let chapter_id = env::var("BENCHMARK_CHAPTER_ID").unwrap();
    let settings = Settings::default();

    let metadata = ComicInfo {
        ..Default::default()
    };
    let arc_manager = Arc::new(Mutex::new(SourceManager::new(
        PathBuf::new(),
        HashMap::new(),
        settings,
    )));
    let manager = arc_manager.blocking_lock();
    let source = Source::from_aix_file(source_path.as_ref(), &manager, &arc_manager).unwrap();
    let pages = executor::block_on(source.get_page_list(
        CancellationToken::new(),
        manga_id,
        chapter_id,
        Some(0.0),
    ))
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("download_chapter_pages_as_cbz", |b| {
        b.to_async(&runtime).iter(async || {
            download_chapter_pages_as_cbz(
                &CancellationToken::new(),
                io::Cursor::new(Vec::new()),
                metadata.clone(),
                &source,
                pages.clone(),
                4,
            )
            .await;
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = chapter_downloader_benchmark
}
criterion_main!(benches);
