#[allow(unused_imports)]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pprof::criterion::{Output, PProfProfiler};
use shared::{
    database::Database, settings::Settings, source_manager::SourceManager, usecases::search_mangas,
};
use std::sync::Arc;
use std::{env, path::PathBuf};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub fn search_mangas_benchmark(c: &mut Criterion) {
    let sources_path: PathBuf = env::var("BENCHMARK_SOURCES_PATH").unwrap().into();
    let query = env::var("BENCHMARK_QUERY").unwrap();
    let settings = Settings::default();

    let arc_manager = Arc::new(Mutex::new(
        SourceManager::from_folder(sources_path, settings).unwrap(),
    ));
    {
        let mut manager = arc_manager.blocking_lock();
        manager.sources_by_id = manager.load_all_sources(&arc_manager).unwrap();
    }

    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("search_mangas", |b| {
        b.to_async(&runtime).iter(|| async {
            let db = Database::new(&PathBuf::from("test.db")).await.unwrap();
            let source_manager: &SourceManager = &arc_manager.blocking_lock();

            search_mangas(
                source_manager,
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
