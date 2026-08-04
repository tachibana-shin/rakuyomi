//! MangaYomi extension runtime: runs a single `*.dart` extension in a
//! dedicated worker thread carrying the d4rt_rs interpreter and the
//! MangaYomi bridge (see `bridge.rs` / `html.rs`).
//!
//! Mirrors MangaYomi's `DartExtensionService`: register the bridge, execute
//! `main(MSource)` to obtain the provider instance, then dispatch every
//! method call through instance `invoke`. The interpreter is `!Send`, so
//! all of it happens on the worker thread; the outside sees synchronous
//! calls that block until the result (or the timeout) is ready.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, bail, Context as _, Result};
use d4rt_rs::value::Value;
use d4rt_rs::Context;
use serde_json::{json, Value as JsonValue};

use crate::settings::SourceSettingValue;
use crate::source::mangayomi::bridge::{
    filter_list_value, map_set, nmap, register_bridge, value_to_json, wrap, BridgeClasses,
    BridgeState, StateRef,
};
use crate::source::mangayomi::html::MangaYomiDom;

/// How long a single extension method call may run before it is aborted.
pub const DEFAULT_INVOKE_TIMEOUT: Duration = Duration::from_secs(60);

/// Request sent to the worker thread. `args` is a JSON array serialising
/// the positional arguments of the method call.
struct WorkerRequest {
    method: String,
    args: String,
    reply: Sender<Result<String>>,
}

/// Runs one MangaYomi extension. All calls are synchronous from the outside;
/// each one blocks until the interpreter produces the result.
pub struct MangayomiRuntime {
    /// The extension source code, as installed (index.json `sourceCodeUrl`).
    code: String,
    /// The `index.json` entry of the extension, used to build the `MSource`
    /// argument for `main()`.
    metadata: JsonValue,
    /// Source preference values (`getPreferenceValue`), shared with the
    /// extension through the bridge and visible to the source so settings
    /// survive worker restarts.
    prefs: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
    timeout: Duration,
    worker: Mutex<Option<WorkerHandle>>,
}

struct WorkerHandle {
    tx: Sender<WorkerRequest>,
    thread: JoinHandle<()>,
}

impl MangayomiRuntime {
    pub fn new(
        code: String,
        metadata: JsonValue,
        prefs: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
        timeout: Duration,
    ) -> Result<Self> {
        let runtime = Self {
            code,
            metadata,
            prefs,
            timeout,
            worker: Mutex::new(None),
        };
        runtime.start_worker()?;
        Ok(runtime)
    }

    /// Calls a method of the provider instance (`getPopular`, `getDetail`,
    /// `getPageList`, `getFilterList`, `search`, `headers`, `supportsLatest`,
    /// ...) and returns the JSON value produced by the interpreter.
    pub fn invoke(&self, method: &str, args: Vec<JsonValue>) -> Result<JsonValue> {
        let mut attempts = 0;
        loop {
            let reply_rx = {
                let mut worker = self.worker.lock().unwrap();
                let needs_restart = worker
                    .as_ref()
                    .map(|w| w.thread.is_finished())
                    .unwrap_or(true);
                if needs_restart {
                    *worker = None;
                    drop(worker);
                    self.start_worker()?;
                    continue;
                }
                let (reply_tx, reply_rx) = channel();
                worker
                    .as_ref()
                    .unwrap()
                    .tx
                    .send(WorkerRequest {
                        method: method.to_string(),
                        args: serde_json::to_string(&args).unwrap_or_else(|_| "[]".to_string()),
                        reply: reply_tx,
                    })
                    .context("failed to send request to extension worker")?;
                reply_rx
            };

            match reply_rx.recv_timeout(self.timeout) {
                Ok(result) => {
                    let json = result?;
                    return serde_json::from_str(&json).context("extension returned invalid JSON");
                }
                Err(_) => {
                    // The worker is stuck (e.g. the extension entered an
                    // infinite loop). Restart it so the next call works.
                    log::warn!(
                        "MangaYomi extension call `{}` timed out; restarting the runtime",
                        method
                    );
                    let mut worker = self.worker.lock().unwrap();
                    *worker = None;
                    attempts += 1;
                    if attempts >= 2 {
                        bail!("extension call `{}` timed out twice", method);
                    }
                }
            }
        }
    }

    fn start_worker(&self) -> Result<()> {
        let code = self.code.clone();
        let metadata = self.metadata.clone();
        let prefs = self.prefs.clone();
        let timeout = self.timeout;

        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("mangayomi-worker".to_string())
            .spawn(move || {
                worker_main(rx, &code, &metadata, prefs, timeout);
            })
            .context("failed to spawn MangaYomi extension worker thread")?;
        *self.worker.lock().unwrap() = Some(WorkerHandle { tx, thread });
        Ok(())
    }
}

fn worker_main(
    rx: Receiver<WorkerRequest>,
    code: &str,
    metadata: &JsonValue,
    prefs: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
    timeout: Duration,
) {
    if let Err(err) = worker_loop(rx, code, metadata, prefs, timeout) {
        log::warn!("MangaYomi extension worker exited: {:#}", err);
    }
}

fn worker_loop(
    rx: Receiver<WorkerRequest>,
    code: &str,
    metadata: &JsonValue,
    prefs: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
    _timeout: Duration,
) -> Result<()> {
    let mut ctx = Context::new();
    ctx.grant(d4rt_rs::permission::Permission::Filesystem(
        d4rt_rs::permission::FilesystemPermission::Any,
    ));
    ctx.grant(d4rt_rs::permission::Permission::Network(
        d4rt_rs::permission::NetworkPermission::Any,
    ));

    let client = crate::tls::blocking_client_builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build HTTP client for MangaYomi worker")?;
    let state: StateRef = Rc::new(RefCell::new(BridgeState {
        client,
        dom: MangaYomiDom::new(),
        prefs,
    }));
    let classes = register_bridge(&mut ctx, state);

    let msource = build_msource(&classes, metadata);
    // Same preprocessing as MangaYomi's `DartExtensionService`.
    let code = code.replace("Client(source)", "Client()");
    let result = ctx
        .execute(&code, "main", vec![msource], HashMap::new())
        .map_err(|e| anyhow!("MangaYomi extension main() failed: {e}"))?;
    if matches!(result, Value::Future(_)) {
        ctx.pump_to_completion(result)
            .map_err(|e| anyhow!("MangaYomi extension main() failed to complete: {e}"))?;
    }
    log::debug!("MangaYomi extension worker started");

    while let Ok(request) = rx.recv() {
        let result = invoke_method(&mut ctx, &request.method, &request.args);
        if request.reply.send(result).is_err() {
            // The caller gave up (timeout); stop the worker so a stuck
            // script does not linger.
            break;
        }
    }
    Ok(())
}

/// Builds the `MSource` bridged instance passed to `main()` from the
/// `index.json` entry of the extension.
fn build_msource(classes: &BridgeClasses, metadata: &JsonValue) -> Value {
    let n = nmap();
    for key in [
        "id",
        "name",
        "baseUrl",
        "lang",
        "isFullData",
        "hasCloudflare",
        "dateFormat",
        "dateFormatLocale",
        "apiUrl",
        "additionalParams",
        "notes",
    ] {
        match metadata.get(key) {
            Some(JsonValue::String(s)) => map_set(&n, key, Value::str(s)),
            Some(JsonValue::Bool(b)) => map_set(&n, key, Value::Bool(*b)),
            Some(JsonValue::Number(num)) => {
                if let Some(i) = num.as_i64() {
                    map_set(&n, key, Value::Int(i));
                }
            }
            _ => {}
        }
    }
    wrap(&classes.msource, n)
}

fn invoke_method(ctx: &mut Context, method: &str, args_json: &str) -> Result<String> {
    let args: Vec<JsonValue> = serde_json::from_str(args_json).unwrap_or_default();
    let mut positional: Vec<Value> = args.iter().map(json_to_value).collect();
    // MangaYomi's `search(String query, int page, FilterList filterList)` takes
    // a `FilterList` instance; the source passes the filters as a plain array.
    if method == "search" && positional.len() == 3 {
        let filters = positional.pop().unwrap_or(Value::Null);
        positional.push(filter_list_value(filters));
    }
    let result = ctx
        .invoke(method, positional, HashMap::new())
        .map_err(|e| anyhow!("extension method `{method}` failed: {e}"))?;
    let result = if matches!(result, Value::Future(_)) {
        ctx.pump_to_completion(result)
            .map_err(|e| anyhow!("extension method `{method}` failed: {e}"))?
    } else {
        result
    };
    serde_json::to_string(&result_to_json(&result)).context("failed to serialise extension result")
}

/// Converts a method result to JSON. `MPages` (native representation is the
/// positional pair `[list, hasNextPage]`) becomes `{"list": ..., "hasNextPage": ...}`
/// so the source can map it onto `MangaPageResult`.
fn result_to_json(v: &Value) -> JsonValue {
    if let Value::Bridged(b) = v {
        let b = b.borrow();
        if b.bridged_class.name == "MPages" {
            if let Value::List(pair) = &b.native {
                let pair = pair.borrow();
                if let (Some(list), Some(has_next)) = (pair.first(), pair.get(1)) {
                    return json!({
                        "list": value_to_json(list),
                        "hasNextPage": value_to_json(has_next),
                    });
                }
            }
        }
        return value_to_json(&b.native);
    }
    value_to_json(v)
}

/// Converts a JSON argument into the interpreter's value representation.
fn json_to_value(v: &JsonValue) -> Value {
    match v {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(b) => Value::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Double(f)
            } else {
                Value::Null
            }
        }
        JsonValue::String(s) => Value::str(s),
        JsonValue::Array(a) => {
            Value::List(Rc::new(RefCell::new(a.iter().map(json_to_value).collect())))
        }
        JsonValue::Object(o) => {
            let m = nmap();
            for (k, val) in o {
                map_set(&m, k, json_to_value(val));
            }
            m
        }
    }
}
