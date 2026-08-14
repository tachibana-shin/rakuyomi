use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    home_path: PathBuf,
}

fn main() -> Result<()> {
    server::log_startup();

    let args = Args::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("rakuyomi-main")
        // boa_engine's tree-walking interpreter (used for Aidoku-next
        // sources' small JS contexts, and now for LNReader sources'
        // full plugin execution) can use meaningfully more stack per
        // JS call than the 2 MiB default, especially in debug builds —
        // give blocking-pool threads (where all `Source` operations
        // actually run, see `wrap_blocking_source_fn!` in
        // `shared/src/source/mod.rs`) more headroom to avoid a stack
        // overflow on complex sources.
        .thread_stack_size(16 * 1024 * 1024)
        // Every `Source` operation runs on the blocking pool (see
        // `wrap_blocking_source_fn!` above), each holding one of the 16 MiB
        // stacks just requested. Tokio's default `max_blocking_threads` is
        // 512, which would let unbounded concurrent Source calls (e.g. a
        // large batch download) balloon to gigabytes of stack memory on
        // e-reader hardware that typically has well under 1 GiB total RAM.
        // Excess `spawn_blocking` calls beyond this cap simply queue for a
        // free thread rather than erroring, so this also bounds concurrent
        // Source operations, not just worst-case memory.
        .max_blocking_threads(8)
        .build()?;

    runtime.block_on(server::run(args.home_path))
}
