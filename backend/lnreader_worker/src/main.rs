//! Disposable subprocess that runs exactly one LNReader plugin operation
//! then exits. Spawned by `shared::source::sdk_lnreader::LnReaderSource`
//! (never invoked directly) — see `shared::source::sdk_lnreader::worker`'s
//! doc comment for why this needs to be its own process rather than an
//! in-process call or a worker thread: a native crash inside `boa_engine`
//! (confirmed via debugger on a source with a large catalog) takes down the
//! whole OS process it runs in, and only process-level isolation actually
//! contains that.
//!
//! Deployed as a standalone binary alongside `server`, same pattern as
//! `uds_http_request`/`cbz_metadata_reader`.

use std::thread;

/// Stack size for the thread that actually runs the worker loop (see
/// [`run_worker`]), instead of the platform's default thread/process stack.
/// `sdk_lnreader::worker::run()`'s call chain includes native Rust recursion
/// with no depth guard (e.g. `htmlparser2::walk()`, one Rust stack frame per
/// level of HTML nesting in a scraped page) and re-entrant native calls from
/// chained `boa_engine` Promise continuations — a native stack overflow was
/// one of two crash signatures observed against real large-catalog sources
/// (see `docs/lnreader/BOA_GC_BOUNDARY_FINDINGS.md`, Option F). 64 MiB is
/// generous headroom over any known real recursion depth here (at most a few
/// hundred frames) and costs nothing at rest on Linux: thread stack size is
/// reserved address space, not committed memory, until actually touched.
const WORKER_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Floor for Tokio's async worker thread pool, independent of the number of
/// CPU cores actually detected on the host running this binary. See
/// [`run_worker`]'s doc comment for why at least one *other* thread besides
/// the one driving the worker loop needs to stay free.
const MIN_WORKER_THREADS: usize = 2;

fn main() -> anyhow::Result<()> {
    // Run on a dedicated thread with an enlarged stack (see
    // `WORKER_STACK_SIZE`) rather than directly on `main`'s own thread, whose
    // stack size is set by the platform/environment (`ulimit -s`, commonly
    // ~8 MiB on Linux, potentially smaller or less predictable on an
    // e-reader/Android target) rather than by this binary.
    let handle = thread::Builder::new()
        .name("lnreader_worker".to_string())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(run_worker)
        .map_err(|e| anyhow::anyhow!("failed to spawn lnreader_worker's main thread: {e}"))?;

    handle
        .join()
        .map_err(|panic| anyhow::anyhow!("lnreader_worker's main thread panicked: {panic:?}"))?
}

/// Builds the Tokio runtime and drives the actual worker loop
/// (`sdk_lnreader::worker::run`) to completion — split out from `main` only
/// so it can run on the enlarged-stack thread `main` spawns, not because it
/// needs to be reusable.
fn run_worker() -> anyhow::Result<()> {
    // `net.rs`'s native `fetch` drives `reqwest` via `futures::executor::block_on`
    // (a plain, non-tokio executor — see that module's doc comment for why:
    // it doesn't need real concurrency, just something to poll the future to
    // completion), directly on whichever thread is running the worker loop —
    // unlike `server`'s WASM sources, which reach the equivalent call via
    // `tokio::task::spawn_blocking` (a separate, dedicated blocking-thread
    // pool, see `shared::source::mod::BlockingSource`'s facade), so their
    // Tokio worker-thread count isn't on the critical path here the same way.
    // That only works if Tokio's own I/O/timer reactor is being driven on a
    // *different* thread in the meantime: a single-threaded runtime doesn't
    // have a spare thread for that, so the one thread it has gets stuck
    // inside the nested `futures::executor::block_on` loop, which has no
    // idea how to advance Tokio's reactor, and any real network call just
    // hangs forever (confirmed empirically: instant with a synthetic
    // no-network request, hung indefinitely against a real site).
    // `Builder::new_multi_thread()` defaults to one worker thread per
    // detected CPU core, which is fine on a multi-core dev machine but not
    // guaranteed on the e-reader targets this project ships to (Kobo/Kindle-
    // class ARM SoCs are commonly single- or dual-core) — `MIN_WORKER_THREADS`
    // floors it explicitly rather than trusting whatever the host reports.
    let worker_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(MIN_WORKER_THREADS);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()?;
    runtime.block_on(async { shared::source::lnreader_worker_main() })
}
