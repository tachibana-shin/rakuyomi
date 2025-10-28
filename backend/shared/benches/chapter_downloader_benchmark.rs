#[allow(unused_imports)]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use futures::executor;
use pprof::criterion::{Output, PProfProfiler};
use shared::{
    cbz_metadata::ComicInfo, chapter_downloader::download_chapter_pages_as_cbz, settings::Settings,
    source::Source,
};
use std::{env, io, path::PathBuf};
use tokio_util::sync::CancellationToken;

pub fn chapter_downloader_benchmark(c: &mut Criterion) {
    let source_path: PathBuf = env::var("BENCHMARK_SOURCE_PATH").unwrap().into();
    let manga_id = env::var("BENCHMARK_MANGA_ID").unwrap();
    let chapter_id = env::var("BENCHMARK_CHAPTER_ID").unwrap();
    let settings = Settings::default();

    let metadata = ComicInfo {
        ..Default::default()
    };
    let source = Source::from_aix_file(source_path.as_ref(), settings).unwrap();
    let pages = executor::block_on(source.get_page_list(
        CancellationToken::new(),
        manga_id,
        chapter_id,
        todo!(),
    ))
    .unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("download_chapter_pages_as_cbz", |b| {
        b.to_async(&runtime).iter(|| {
            download_chapter_pages_as_cbz(
                io::Cursor::new(Vec::new()),
                metadata.clone(),
                &source,
                pages.clone(),
                4,
            )
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = chapter_downloader_benchmark
}
criterion_main!(benches);
