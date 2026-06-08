use std::path::PathBuf;

use anyhow::{Context, Result};
use log::warn;
use tokio::net::{TcpListener, UnixListener};

pub const DEFAULT_UNIX_SOCKET_PATH: &str = "/tmp/rakuyomi.sock";

/// Resolved transport the server will bind to. Both variants implement
/// `tokio::net::Listener` and can be passed to `axum::serve` directly.
pub enum ResolvedListener {
    Unix(UnixListener, PathBuf),
    Tcp(TcpListener),
}

impl ResolvedListener {
    pub fn describe(&self) -> String {
        match self {
            ResolvedListener::Unix(_, path) => format!("unix:{}", path.display()),
            ResolvedListener::Tcp(l) => format!("tcp:{}", l.local_addr().unwrap()),
        }
    }
}

/// Pick a listener based on environment variables.
///
/// Resolution order:
/// 1. `RAKUYOMI_USE_TCP=1` (with optional `RAKUYOMI_TCP_PORT`, default 8787) — TCP loopback
/// 2. `RAKUYOMI_UNIX_SOCKET_PATH` — custom Unix domain socket
/// 3. Default Unix domain socket at `/tmp/rakuyomi.sock`
pub async fn pick_listener() -> Result<ResolvedListener> {
    if let Some(port) = std::env::var("RAKUYOMI_TCP_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
    {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("failed to bind TCP 127.0.0.1:{}", port))?;
        return Ok(ResolvedListener::Tcp(listener));
    }

    let path = std::env::var("RAKUYOMI_UNIX_SOCKET_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_UNIX_SOCKET_PATH));

    let _ = std::fs::remove_file(&path).inspect_err(|e| {
        warn!(
            "could not remove existing socket path {}: {}",
            path.display(),
            e
        )
    });
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind unix socket at {}", path.display()))?;

    Ok(ResolvedListener::Unix(listener, path))
}
