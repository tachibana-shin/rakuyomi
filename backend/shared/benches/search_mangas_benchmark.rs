#[allow(unused_imports)]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use shared::{
    database::Database, settings::Settings, source_manager::SourceManager, usecases::search_mangas,
};
use std::{env, path::PathBuf};
use tokio_util::sync::CancellationToken;

pub fn search_mangas_benchmark(c: &mut Criterion) {
    let sources_path: PathBuf = env::var("BENCHMARK_SOURCES_PATH").unwrap().into();
    let query = env::var("BENCHMARK_QUERY").unwrap();
    let settings = Settings::default();

    let source_manager = SourceManager::from_folder(sources_path, settings).unwrap();

    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("search_mangas", |b| {
        b.to_async(&runtime).iter(|| async {
            let db = Database::new(&PathBuf::from("test.db")).await.unwrap();

            search_mangas(
                &source_manager,
                &db,
                CancellationToken::new(),
                query.clone(),
                60,
            )
            .await
            .unwrap();
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = search_mangas_benchmark
}
criterion_main!(benches);
