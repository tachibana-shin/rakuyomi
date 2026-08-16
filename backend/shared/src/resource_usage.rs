use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::Serialize;

/// How long the per-source VM memory tracking stays on after the last
/// poll of the usage API. Tracking is demand-driven: the workers only
/// compute memory estimates while something is watching, and the data is
/// discarded once this much time has passed without a poll.
pub const USAGE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Runtime resource accounting for a single source: how often it was called,
/// how long calls took, and — for wasm sources — how much linear memory the
/// engine holds.
#[derive(Debug, Default, Clone, Serialize)]
pub struct SourceUsage {
    /// How many source calls have been recorded so far.
    pub invokes: u64,
    /// Sum of all recorded call durations in milliseconds.
    pub total_duration_ms: u64,
    /// Duration of the most recent call in milliseconds.
    pub last_duration_ms: u64,
    /// Peak wasm linear memory size in bytes (only tracked for wasm sources).
    pub peak_wasm_memory_bytes: u64,
    /// Error message of the last failed call, if any.
    pub last_error: Option<String>,
}

/// Thread-safe per-source usage registry. Cloning is cheap (Arc-backed); each
/// [`Source`](crate::source::Source) holds its own handle.
#[derive(Debug, Default, Clone)]
pub struct ResourceRegistry {
    inner: Arc<Mutex<HashMap<String, SourceUsage>>>,
    /// When the usage API last asked for tracking (`None` = no observer).
    last_activity: Arc<Mutex<Option<Instant>>>,
}

impl ResourceRegistry {
    /// Marks the registry as being observed, keeping memory tracking alive
    /// until [`ResourceRegistry::usage`] finds it idle past the timeout.
    pub fn mark_active(&self) {
        if let Ok(mut last_activity) = self.last_activity.lock() {
            *last_activity = Some(Instant::now());
        }
    }

    /// True while the usage API is being polled (or was polled recently).
    pub fn is_active(&self) -> bool {
        self.last_activity
            .lock()
            .ok()
            .and_then(|last_activity| *last_activity)
            .is_some_and(|t| t.elapsed() < USAGE_IDLE_TIMEOUT)
    }

    /// How long the registry has been unobserved, if ever active.
    pub fn idle_for(&self) -> Option<Duration> {
        self.last_activity
            .lock()
            .ok()
            .and_then(|last_activity| *last_activity)
            .map(|t| t.elapsed())
    }

    /// Records a finished source call.
    pub fn record(&self, source_id: &str, outcome: Result<(), String>, elapsed: Duration) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let usage = inner.entry(source_id.to_string()).or_default();
        usage.invokes += 1;
        usage.total_duration_ms += elapsed.as_millis() as u64;
        usage.last_duration_ms = elapsed.as_millis() as u64;
        if let Err(error) = outcome {
            usage.last_error = Some(error);
        }
    }

    /// Records the current wasm linear memory size, keeping the peak.
    pub fn record_wasm_memory(&self, source_id: &str, bytes: u64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let usage = inner.entry(source_id.to_string()).or_default();
        usage.peak_wasm_memory_bytes = usage.peak_wasm_memory_bytes.max(bytes);
    }

    /// Returns the recorded usage for a source, if any. When the registry
    /// has been idle past [`USAGE_IDLE_TIMEOUT`] — the view closed, the
    /// workers stopped capturing long ago — the recorded memory data is
    /// discarded on the first read, so a reopened view starts from zero.
    /// Call statistics survive: they cost nothing, only the memory capture
    /// is dropped.
    pub fn usage(&self, source_id: &str) -> Option<SourceUsage> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        if self
            .idle_for()
            .is_some_and(|idle| idle >= USAGE_IDLE_TIMEOUT)
        {
            *self.last_activity.lock().unwrap_or_else(|e| e.into_inner()) = None;
            for usage in inner.values_mut() {
                usage.peak_wasm_memory_bytes = 0;
            }
        }
        inner.get(source_id).cloned()
    }
}
