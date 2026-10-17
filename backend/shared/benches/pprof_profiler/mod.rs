//! Criterion profiler adapter writing a pprof flamegraph on `--profile-time`.
//!
//! `pprof::criterion::PProfProfiler` still implements the criterion 0.5
//! `Profiler` trait, so adapt its `ProfilerGuard` to criterion 0.8 here.

use criterion::profiler::Profiler;
use pprof::ProfilerGuard;
use std::{fs::File, os::raw::c_int, path::Path};

pub struct PProfProfiler {
    frequency: c_int,
    active_profiler: Option<ProfilerGuard<'static>>,
}

impl PProfProfiler {
    pub fn new(frequency: c_int) -> Self {
        Self {
            frequency,
            active_profiler: None,
        }
    }
}

impl Profiler for PProfProfiler {
    fn start_profiling(&mut self, _benchmark_id: &str, _benchmark_dir: &Path) {
        self.active_profiler = Some(ProfilerGuard::new(self.frequency).unwrap());
    }

    fn stop_profiling(&mut self, _benchmark_id: &str, benchmark_dir: &Path) {
        std::fs::create_dir_all(benchmark_dir).unwrap();
        let output_file = File::create(benchmark_dir.join("flamegraph.svg")).unwrap();

        if let Some(profiler) = self.active_profiler.take() {
            profiler
                .report()
                .build()
                .unwrap()
                .flamegraph(output_file)
                .expect("Error while writing flamegraph");
        }
    }
}
