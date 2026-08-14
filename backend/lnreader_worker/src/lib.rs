//! Minimal library surface for `lnreader_worker`.
//!
//! The crate exists almost entirely as a standalone subprocess binary
//! (`src/main.rs`) — see that file's doc comment for the process-isolation
//! rationale. The library half exists only so the end-to-end tests in
//! `shared::source::sdk_lnreader::tests` (which launch a real
//! `lnreader_worker` subprocess) can declare a path dependency on this crate
//! from `shared`'s `[dev-dependencies]`. Without a library target, cargo's
//! dev-dependency on a binary-only crate does not trigger the binary build,
//! and the tests panic with "couldn't find the lnreader_worker binary".
//!
//! No production code in `shared` links against this crate — the subprocess
//! is launched by absolute path at runtime, not imported. This `pub use` is
//! just enough for cargo to compile something here so the binary builds
//! alongside it.

/// Subprocess protocol version spoken by the `lnreader_worker` binary.
/// Exposed here purely so the dev-dependency forces a compile of this
/// crate (and therefore of the sibling binary). Not used by any production
/// code path — the subprocess protocol is matched positionally.
pub const WORKER_PROTOCOL_VERSION: u32 = 1;
