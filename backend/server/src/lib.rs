//! Rakuyomi's HTTP server. Built as both a binary (for Kindle / Kobo /
//! reMarkable / desktop Linux) and a `cdylib` (loaded as a native library
//! by the Android companion app via JNI).
//!
//! Two transports are supported, chosen at runtime via environment
//! variables:
//!
//! * Unix domain socket at `/tmp/rakuyomi.sock` — used on Kindle, Kobo,
//!   reMarkable, desktop Linux.
//! * TCP loopback on `127.0.0.1:8787` — used on Android, where Unix
//!   domain sockets between apps are not available.
//!
//! See [`listener::pick_listener`] for the exact resolution rules.

pub mod build_info;
pub mod error;
pub mod job;
pub mod listener;
pub mod manga;
pub mod model;
pub mod playlists;
pub mod settings;
pub mod source;
pub mod source_extractor;
pub mod state;
pub mod update;

mod app;

pub use app::{build_router, build_state, init_logging, log_startup, run, serve};
pub use error::{AppError, ErrorResponse};
pub use listener::{pick_listener, ResolvedListener};

#[cfg(feature = "ffi")]
pub mod jni;
