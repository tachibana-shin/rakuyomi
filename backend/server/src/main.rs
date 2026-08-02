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
        .build()?;

    runtime.block_on(server::run(args.home_path))
}
