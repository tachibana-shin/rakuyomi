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

fn main() -> anyhow::Result<()> {
    // `net.rs`'s native `fetch` drives `reqwest` via `futures::executor::block_on`
    // (a plain, non-tokio executor — see that module's doc comment for why:
    // it doesn't need real concurrency, just something to poll the future to
    // completion). That only works if Tokio's own I/O/timer reactor is being
    // driven on a *different* thread in the meantime — in `server`, that's
    // exactly what happens (multi-threaded runtime, `net.rs` runs on a
    // `spawn_blocking` thread while the runtime's own worker threads keep
    // driving the reactor). A single-threaded runtime doesn't have a spare
    // thread for that: the one thread it has gets stuck inside the nested
    // `futures::executor::block_on` loop, which has no idea how to advance
    // Tokio's reactor, so any real network call just hangs forever
    // (confirmed empirically: instant with a synthetic no-network request,
    // hung indefinitely against a real site). Multi-threaded, even for this
    // single-shot worker, avoids that entirely.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async { shared::source::lnreader_worker_main() })
}
