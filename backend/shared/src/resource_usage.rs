use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;

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
}

impl ResourceRegistry {
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

    /// Returns the recorded usage for a source, if any.
    pub fn usage(&self, source_id: &str) -> Option<SourceUsage> {
        self.inner.lock().ok()?.get(source_id).cloned()
    }
}
