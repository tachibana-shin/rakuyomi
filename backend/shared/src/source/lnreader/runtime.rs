use anyhow::{anyhow, bail, Context as _, Result};
use base64::Engine;
use rquickjs::{Context, Ctx, Exception, Function, Promise, Runtime};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Write as _,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// How long a single plugin method call may run before it is aborted.
pub const DEFAULT_INVOKE_TIMEOUT: Duration = Duration::from_secs(60);

/// Grace period given to the interrupt handler to abort the running script
/// before the caller gives up on the reply channel.
const INTERRUPT_LEEWAY: Duration = Duration::from_secs(2);

/// The embedded JS runtime (`libs.js`) plus the minified packages it can
/// lazily load through its `require()` implementation.
pub(crate) static LIBS_JS: &str = include_str!("assets/libs.js");

/// Drives a `reqwest` future to completion from the LNReader worker thread.
///
/// The worker is a plain OS thread, so there is no tokio runtime context
/// there; this drives the future on a dedicated current-thread runtime owned
/// by whichever thread performs the fetch. Using a self-contained runtime
/// also keeps the call correct when it happens to run inside an existing
/// tokio context.
fn block_on_reactor<F>(fut: F) -> F::Output
where
    F: std::future::Future,
{
    thread_local! {
        static WORKER_REACTOR: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for LNReader worker");
    }
    WORKER_REACTOR.with(|rt| rt.block_on(fut))
}

struct WorkerRequest {
    method: String,
    args: String,
    reply: Sender<Result<String>>,
}

/// Handles a single LNReader plugin inside a dedicated thread running a
/// QuickJS runtime. All plugin calls are synchronous from the outside: each
/// host function blocks until the underlying async work completes.
pub struct LnReaderRuntime {
    plugin_id: String,
    plugin_code: String,
    site: String,
    user_agent: String,
    /// Storage shared between the JS host functions; survives worker restarts.
    storage: Arc<Mutex<HashMap<String, String>>>,
    timeout: Duration,
    worker: Mutex<Option<WorkerHandle>>,
}

struct WorkerHandle {
    tx: Sender<WorkerRequest>,
    thread: JoinHandle<()>,
}

impl LnReaderRuntime {
    pub fn new(
        plugin_id: String,
        plugin_code: String,
        site: String,
        user_agent: String,
        timeout: Duration,
    ) -> Result<Self> {
        // The worker is not spawned here: `invoke` starts (or restarts) it
        // on demand, so a plugin only occupies a thread + JS context while a
        // call is actually running.
        Ok(Self {
            plugin_id,
            plugin_code,
            site,
            user_agent,
            storage: Arc::new(Mutex::new(HashMap::new())),
            timeout,
            worker: Mutex::new(None),
        })
    }

    /// Stops the worker thread, if any. The runtime is fully reusable: the
    /// next `invoke` spawns a fresh worker.
    pub fn stop_worker(&self) {
        let Ok(mut worker) = self.worker.lock() else {
            return;
        };
        *worker = None;
    }

    /// Inserts raw storage entries (`pluginId_DB_key` → JSON item) before any
    /// plugin method is invoked. This mirrors the LNReader app seeding the
    /// plugin's `@libs/storage` with its pluginSettings values.
    pub fn seed_storage(&self, entries: Vec<(String, String)>) {
        let mut storage = self.storage.lock().unwrap();
        for (key, value) in entries {
            storage.insert(key, value);
        }
    }

    /// Calls a plugin method (`props`, `search`, `popular`, `novel`, `page`,
    /// `chapter`, `resolveUrl`) and returns the JSON string produced by the
    /// JS runtime.
    pub fn invoke(&self, method: &str, args: &str) -> Result<String> {
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
                        args: args.to_string(),
                        reply: reply_tx,
                    })
                    .context("failed to send request to plugin worker")?;
                reply_rx
            };

            match reply_rx.recv_timeout(self.timeout) {
                Ok(result) => return result,
                Err(_) => {
                    // The worker is stuck (e.g. a plugin entered an infinite
                    // loop that even the interrupt handler could not break).
                    // Restart it so the next call works.
                    log::warn!(
                        "LNReader plugin call `{}` timed out; restarting the plugin runtime",
                        method
                    );
                    let mut worker = self.worker.lock().unwrap();
                    *worker = None;
                    attempts += 1;
                    if attempts >= 2 {
                        bail!("plugin call `{}` timed out twice", method);
                    }
                }
            }
        }
    }

    fn start_worker(&self) -> Result<()> {
        let plugin_id = self.plugin_id.clone();
        let plugin_code = self.plugin_code.clone();
        let site = self.site.clone();
        let user_agent = self.user_agent.clone();
        let storage = self.storage.clone();
        let timeout = self.timeout;

        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("lnreader-worker".to_string())
            .spawn(move || {
                worker_main(
                    rx,
                    &plugin_id,
                    &plugin_code,
                    &site,
                    &user_agent,
                    storage,
                    timeout,
                )
            })
            .context("failed to spawn LNReader plugin worker thread")?;
        *self.worker.lock().unwrap() = Some(WorkerHandle { tx, thread });
        Ok(())
    }
}

fn worker_main(
    rx: Receiver<WorkerRequest>,
    plugin_id: &str,
    plugin_code: &str,
    site: &str,
    user_agent: &str,
    storage: Arc<Mutex<HashMap<String, String>>>,
    timeout: Duration,
) {
    let result = worker_loop(
        rx,
        plugin_id,
        plugin_code,
        site,
        user_agent,
        storage,
        timeout,
    );
    if let Err(err) = result {
        log::warn!("LNReader plugin worker exited: {:#}", err);
    }
}

fn worker_loop(
    rx: Receiver<WorkerRequest>,
    plugin_id: &str,
    plugin_code: &str,
    site: &str,
    user_agent: &str,
    storage: Arc<Mutex<HashMap<String, String>>>,
    timeout: Duration,
) -> Result<()> {
    let runtime = Runtime::new().context("failed to create QuickJS runtime")?;
    let interrupt_deadline: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    {
        let deadline = interrupt_deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            matches!(
                *deadline.lock().unwrap(),
                Some(d) if Instant::now() >= d
            )
        })));
    }

    let context = Context::full(&runtime).context("failed to create QuickJS context")?;
    context
        .with(|ctx| {
            register_globals(
                &ctx,
                plugin_id,
                plugin_code,
                site,
                user_agent,
                storage.clone(),
            )
        })
        .context("failed to initialise plugin runtime")?;
    log::debug!("LNReader plugin worker started");

    while let Ok(request) = rx.recv() {
        let result = context.with(|ctx| {
            let deadline = if timeout > INTERRUPT_LEEWAY {
                timeout - INTERRUPT_LEEWAY
            } else {
                timeout
            };
            *interrupt_deadline.lock().unwrap() = Some(Instant::now() + deadline);
            let result = invoke_method(&ctx, &request.method, &request.args);
            *interrupt_deadline.lock().unwrap() = None;
            result
        });
        if request.reply.send(result).is_err() {
            // The caller gave up (timeout); stop the worker so a stuck
            // script does not linger.
            break;
        }
    }
    Ok(())
}

fn invoke_method(ctx: &Ctx<'_>, method: &str, args: &str) -> Result<String> {
    let globals = ctx.globals();
    let invoke: Function = globals
        .get("__rakuyomiInvoke")
        .context("missing __rakuyomiInvoke")?;
    let promise: Promise = invoke
        .call((method.to_string(), args.to_string()))
        .context("failed to call plugin method")?;
    // `finish` drives the QuickJS job queue until the promise settles. All
    // host functions are synchronous, so the promise always settles here.
    match promise.finish::<String>() {
        Ok(value) => Ok(value),
        Err(e) => {
            let value = ctx.catch();
            let js_detail = value
                .as_object()
                .and_then(|obj| Exception::from_object(obj.clone()))
                .map(|ex| {
                    let message = ex.message().unwrap_or_default();
                    let stack = ex.stack().unwrap_or_default();
                    if stack.trim().is_empty() {
                        message
                    } else {
                        format!("{message}\n{stack}")
                    }
                })
                .unwrap_or_else(|| format!("{:#}", e));
            bail!("plugin method `{}` failed: {}", method, js_detail);
        }
    }
}

fn register_globals(
    ctx: &Ctx<'_>,
    plugin_id: &str,
    plugin_code: &str,
    site: &str,
    user_agent: &str,
    storage: Arc<Mutex<HashMap<String, String>>>,
) -> Result<()> {
    let globals = ctx.globals();
    globals.set("RAKUYOMI_PLUGIN_CODE", plugin_code.to_string())?;
    globals.set("RAKUYOMI_PLUGIN_ID", plugin_id.to_string())?;
    globals.set("RAKUYOMI_PLUGIN_SITE", site.to_string())?;
    globals.set("RAKUYOMI_USER_AGENT", user_agent.to_string())?;

    globals.set("__rakuyomiFetch", Function::new(ctx.clone(), host_fetch))?;
    globals.set("__rakuyomiDecode", Function::new(ctx.clone(), host_decode))?;
    globals.set(
        "__rakuyomiEncodeUtf8",
        Function::new(ctx.clone(), host_encode_utf8),
    )?;
    globals.set("__rakuyomiLog", Function::new(ctx.clone(), host_log))?;
    globals.set("__rakuyomiSleep", Function::new(ctx.clone(), host_sleep))?;
    globals.set("__rakuyomiUuid", Function::new(ctx.clone(), host_uuid))?;
    globals.set(
        "__rakuyomiPluginId",
        Function::new(ctx.clone(), host_plugin_id(plugin_id.to_string())),
    )?;
    globals.set(
        "__rakuyomiStorageGet",
        Function::new(ctx.clone(), host_storage_get(storage.clone())),
    )?;
    globals.set(
        "__rakuyomiStorageSet",
        Function::new(ctx.clone(), host_storage_set(storage.clone())),
    )?;
    globals.set(
        "__rakuyomiStorageRemove",
        Function::new(ctx.clone(), host_storage_remove(storage.clone())),
    )?;
    globals.set(
        "__rakuyomiStorageClear",
        Function::new(ctx.clone(), host_storage_clear(storage.clone())),
    )?;
    globals.set(
        "__rakuyomiStorageKeys",
        Function::new(ctx.clone(), host_storage_keys(storage)),
    )?;
    // Native replacements for the pure-JS shims that libs.js used to bundle.
    // Registered before `libs.js` so the bundler's own attachments would
    // override them; the TS sources no longer define these at all.
    globals.set("atob", Function::new(ctx.clone(), host_atob))?;
    globals.set("btoa", Function::new(ctx.clone(), host_btoa))?;

    ctx.eval::<(), _>(LIBS_JS)
        .map_err(|e| anyhow!("failed to evaluate libs.js: {}", e))?;

    let load_plugin: Function = globals
        .get("__rakuyomiLoadPlugin")
        .context("missing __rakuyomiLoadPlugin")?;
    load_plugin
        .call::<_, bool>(())
        .map_err(|e| anyhow!("plugin load failed: {}", e))?;
    Ok(())
}

/// Implements the JS `fetch` global. The response is encoded as a JSON string
/// of the shape `{ ok, status, url, headers, bodyB64 }`.
fn host_fetch(ctx: Ctx<'_>, url: String, init_json: String) -> rquickjs::Result<String> {
    do_fetch(url, &init_json).map_err(|e| Exception::throw_message(&ctx, &format!("{:#}", e)))
}

fn do_fetch(url: String, init_json: &str) -> Result<String> {
    let init: Value = serde_json::from_str(init_json).unwrap_or(Value::Null);
    let client = crate::tls::client_builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build HTTP client")?;

    let method = init
        .get("method")
        .and_then(Value::as_str)
        .and_then(|m| reqwest::Method::from_bytes(m.as_bytes()).ok())
        .unwrap_or(reqwest::Method::GET);
    let mut builder = client
        .request(method, &url)
        .header(reqwest::header::ACCEPT, "*/*");
    if let Some(headers) = init.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                builder = builder.header(name.as_str(), value);
            }
        }
    }
    if let Some(body) = init.get("body").and_then(Value::as_str) {
        builder = builder.body(body.to_string());
    }
    if let Some(body_b64) = init.get("bodyB64").and_then(Value::as_str) {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(body_b64) {
            builder = builder.body(bytes);
        }
    }
    if let Some(form) = init.get("formData").and_then(Value::as_array) {
        // The JS side serializes `FormData` into an array of entries; rebuild
        // a multipart body from them, mirroring the app's fetch wrapper.
        const BOUNDARY: &str = "----rakuyomi-form-boundary";
        let mut bytes: Vec<u8> = Vec::new();
        for entry in form {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if entry
                .get("isBlob")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let b64 = entry
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let data = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap_or_default();
                let content_type = entry
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("application/octet-stream");
                let filename = entry
                    .get("filename")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("blob");
                write!(
                    bytes,
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
                )
                .unwrap();
                bytes.extend_from_slice(&data);
                bytes.extend_from_slice(b"\r\n");
            } else {
                let value = entry
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                write!(
                    bytes,
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .unwrap();
            }
        }
        bytes.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
        builder = builder
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(bytes);
    }

    let mut request = builder
        .build()
        .with_context(|| format!("failed to build request for {}", url))?;

    // Apply the same per-domain user-agent / cookie overrides as the WASM
    // sources, so that cookie-synced sessions work for LNReader plugins too.
    if let Some(host) = request.url().host_str() {
        let (override_ua, cookie_value) =
            crate::cookie_store::get_user_agent_and_cookie_header(host);
        if let Some(ua) = override_ua {
            if let Ok(header) = reqwest::header::HeaderValue::from_str(&ua) {
                request
                    .headers_mut()
                    .insert(reqwest::header::USER_AGENT, header);
            }
        }
        if let Some(cookies) = cookie_value {
            if let Ok(header) = reqwest::header::HeaderValue::from_str(&cookies) {
                request
                    .headers_mut()
                    .insert(reqwest::header::COOKIE, header);
            }
        }
    }

    // `client.execute` synchronously sets up the per-request timeout timers,
    // so it must run inside the reactor too.
    let response = block_on_reactor(async move { client.execute(request).await })
        .with_context(|| format!("request to {} failed", url))?;
    let status = response.status().as_u16();
    let ok = response.status().is_success();
    let final_url = response.url().to_string();
    // `Set-Cookie` headers land in the shared RakuYomi store (the single
    // cookie source, like reqwest's cookie jar).
    crate::cookie_store::record_response_cookies(&response);
    let mut headers = serde_json::Map::new();
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            headers.insert(name.as_str().to_string(), json!(value));
        }
    }
    let body = block_on_reactor(response.bytes()).context("failed to read response body")?;
    let body_b64 = base64::engine::general_purpose::STANDARD.encode(&body);

    Ok(json!({
        "ok": ok,
        "status": status,
        "url": final_url,
        "headers": headers,
        "bodyB64": body_b64,
    })
    .to_string())
}

fn host_decode(ctx: Ctx<'_>, b64: String, encoding: String) -> rquickjs::Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| Exception::throw_message(&ctx, &format!("invalid base64: {}", e)))?;
    decode_bytes(&bytes, &encoding).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))
}

fn host_encode_utf8(s: String) -> rquickjs::Result<String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(s.as_bytes()))
}

// ---------------------------------------------------------------------------
// Native `atob` / `btoa` globals
//
// These replace the pure-JS implementations that were bundled in libs.js, so
// plugins no longer depend on a JS shim for the two most commonly used
// native functions. The semantics match the previous shim for well-formed
// input (optional padding, whitespace and other non-alphabet characters
// dropped), so existing plugins keep working unchanged.
// ---------------------------------------------------------------------------

/// `btoa(s)`: latin1 -style base64 encode, truncating every UTF-16 code unit
/// to a byte (the old shim's `b64Encode(strToBytes(s, "latin1"))`).
fn lib_btoa(s: &str) -> String {
    let bytes: Vec<u8> = s.encode_utf16().map(|u| (u & 255) as u8).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// `atob(s)`: lenient base64 decode into a latin1 string, like the old
/// shim's `bytesToStr(b64Decode(s), "latin1")`. The base64 crate has no
/// forgiving mode (it rejects non-alphabet characters outright), so the
/// characters outside the alphabet are dropped first, then the remainder is
/// decoded with optional padding and trailing bits tolerated.
fn lib_atob(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        .collect();
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::general_purpose::GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    engine
        .decode(cleaned.as_bytes())
        .unwrap_or_default()
        .into_iter()
        .map(|byte| byte as char)
        .collect()
}

fn host_atob(data: String) -> rquickjs::Result<String> {
    Ok(lib_atob(&data))
}

fn host_btoa(data: String) -> rquickjs::Result<String> {
    Ok(lib_btoa(&data))
}

fn host_log(level: String, message: String) {
    match level.as_str() {
        "error" => log::error!("[lnreader] {}", message),
        "warn" => log::warn!("[lnreader] {}", message),
        _ => log::info!("[lnreader] {}", message),
    }
}

fn host_sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn host_uuid() -> rquickjs::Result<String> {
    // UUID v4-shaped, generated from a random-ish seed. Plugins only use this
    // for cache keys, so collision resistance is not critical.
    let digest = {
        use sha2::Digest;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let mut hasher = sha2::Sha256::new();
        hasher.update(now.as_nanos().to_le_bytes());
        hasher.update(std::process::id().to_le_bytes());
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&hasher.finalize());
        seed
    };
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

fn host_plugin_id(plugin_id: String) -> impl Fn() -> String {
    move || plugin_id.clone()
}

fn host_storage_get(
    storage: Arc<Mutex<HashMap<String, String>>>,
) -> impl Fn(String) -> Option<String> {
    move |key: String| storage.lock().unwrap().get(&key).cloned()
}

fn host_storage_set(storage: Arc<Mutex<HashMap<String, String>>>) -> impl Fn(String, String) {
    move |key: String, item: String| {
        storage.lock().unwrap().insert(key, item);
    }
}

fn host_storage_remove(storage: Arc<Mutex<HashMap<String, String>>>) -> impl Fn(String) {
    move |key: String| {
        storage.lock().unwrap().remove(&key);
    }
}

fn host_storage_clear(storage: Arc<Mutex<HashMap<String, String>>>) -> impl Fn() {
    move || {
        storage.lock().unwrap().clear();
    }
}

fn host_storage_keys(storage: Arc<Mutex<HashMap<String, String>>>) -> impl Fn() -> String {
    move || {
        let keys: Vec<String> = storage.lock().unwrap().keys().cloned().collect();
        serde_json::to_string(&keys).unwrap_or_else(|_| "[]".to_string())
    }
}

/// Decodes bytes using the given encoding name (as accepted by `TextDecoder`).
fn decode_bytes(bytes: &[u8], encoding: &str) -> Result<String> {
    let encoding = encoding.to_lowercase().replace('_', "-");
    let encoding = encoding.trim();
    match encoding {
        "" | "utf-8" | "utf8" => Ok(String::from_utf8_lossy(bytes).into_owned()),
        "utf-16le" | "utf16le" | "unicode" => {
            let mut u16s = Vec::with_capacity(bytes.len() / 2);
            for chunk in bytes.chunks_exact(2) {
                u16s.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            Ok(String::from_utf16_lossy(&u16s))
        }
        "utf-16be" | "utf16be" => {
            let mut u16s = Vec::with_capacity(bytes.len() / 2);
            for chunk in bytes.chunks_exact(2) {
                u16s.push(u16::from_be_bytes([chunk[0], chunk[1]]));
            }
            Ok(String::from_utf16_lossy(&u16s))
        }
        // latin1 / windows-1252: bytes map directly to code points.
        "latin1" | "latin-1" | "iso-8859-1" | "windows-1252" | "cp1252" => {
            Ok(bytes.iter().map(|&b| b as char).collect())
        }
        other => bail!("unsupported encoding `{}`", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atob_btoa_roundtrip() {
        for s in [
            "",
            "M",
            "Hello, World!",
            "MangaYomi/LNReader plugin",
            "a\u{0}b\u{FF}c",
        ] {
            assert_eq!(lib_atob(&lib_btoa(s)), s, "roundtrip of {s:?}");
        }
    }

    #[test]
    fn btoa_matches_standard_base64() {
        assert_eq!(lib_btoa("Hello, World!"), "SGVsbG8sIFdvcmxkIQ==");
        assert_eq!(lib_btoa("M"), "TQ==");
        assert_eq!(lib_btoa(""), "");
    }

    #[test]
    fn btoa_truncates_utf16_units_like_latin1() {
        // "é" is U+00E9 -> 0xE9; "€" is U+20AC -> 0xAC (low byte), matching
        // the shim's `charCodeAt & 255` per UTF-16 code unit.
        assert_eq!(lib_btoa("é"), "6Q==");
        assert_eq!(lib_atob("6Q=="), "é");
        assert_eq!(lib_btoa("€"), "rA==");
    }

    #[test]
    fn atob_is_lenient_like_the_js_shim() {
        // Missing padding: the missing chars read as "A" (value 0), so
        // "Zg" decodes to "f". Standard atob would reject this.
        assert_eq!(lib_atob("Zg"), "f");
        // Invalid characters are dropped before decoding.
        assert_eq!(lib_atob("T Q =="), lib_atob("TQ=="));
        // High bytes come back as latin1 characters, not failure.
        assert_eq!(lib_atob("rA=="), "¬");
    }

    /// The native `atob`/`btoa` must be callable from JS inside a real
    /// rquickjs context, i.e. the JS->Rust argument and Rust->JS return
    /// marshaling of the registered host functions must be correct.
    #[test]
    fn atob_btoa_marshaled_through_quickjs() {
        let context = Context::full(&Runtime::new().unwrap()).unwrap();
        context.with(|ctx| {
            let globals = ctx.globals();
            globals
                .set("atob", Function::new(ctx.clone(), host_atob))
                .unwrap();
            globals
                .set("btoa", Function::new(ctx.clone(), host_btoa))
                .unwrap();
            let out: String = ctx
                .eval(
                    r#"
                    JSON.stringify({
                        types: [typeof atob, typeof btoa],
                        enc: btoa("Hello, World!"),
                        dec: atob("SGVsbG8sIFdvcmxkIQ=="),
                        uniEnc: btoa("é"),
                        uniDec: atob("6Q=="),
                        lenient: atob("Zg"),
                        empty: [btoa(""), atob("")]
                    })
                    "#,
                )
                .expect("js assertions must run");
            let result: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(result["types"], json!(["function", "function"]));
            assert_eq!(result["enc"], "SGVsbG8sIFdvcmxkIQ==");
            assert_eq!(result["dec"], "Hello, World!");
            assert_eq!(result["uniEnc"], "6Q==");
            assert_eq!(result["uniDec"], "é");
            assert_eq!(result["lenient"], "f");
            assert_eq!(result["empty"], json!(["", ""]));
        });
    }

    /// The `b64Encode`/`b64Decode` JS helpers (b64.ts) are thin wrappers over
    /// the native `atob`/`btoa` host globals; their byte round-trip through a
    /// real rquickjs context must be correct.
    #[test]
    fn b64_helpers_through_native_atob_btoa() {
        let context = Context::full(&Runtime::new().unwrap()).unwrap();
        context.with(|ctx| {
            let globals = ctx.globals();
            globals
                .set("atob", Function::new(ctx.clone(), host_atob))
                .unwrap();
            globals
                .set("btoa", Function::new(ctx.clone(), host_btoa))
                .unwrap();
            let out: String = ctx
                .eval(
                    r#"
                    function b64Encode(bytes) {
                      let out = "";
                      for (let i = 0; i < bytes.length; i++) out += String.fromCharCode(bytes[i]);
                      return btoa(out);
                    }
                    function b64Decode(str) {
                      const out = atob(String(str));
                      const bytes = new Uint8Array(out.length);
                      for (let i = 0; i < out.length; i++) bytes[i] = out.charCodeAt(i) & 255;
                      return bytes;
                    }
                    function enc(arr) { return b64Encode(Uint8Array.from(arr)); }
                    function dec(s) { return Array.from(b64Decode(s)); }
                    JSON.stringify({
                        hello: enc([72, 105]),
                        empty: enc([]),
                        one: enc([255]),
                        dec: dec("SGk="),
                        roundtrip: dec(enc([0, 1, 2, 254, 255])),
                        lenient: dec("T Q =="),
                        whitespace: dec("  UmFr dQ== "),
                        unpadded: dec("Zg"),
                        highBytes: dec(enc([128, 255, 0]))
                    })
                    "#,
                )
                .expect("js assertions must run");
            let result: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(result["hello"], "SGk=");
            assert_eq!(result["empty"], "");
            assert_eq!(result["one"], "/w==");
            assert_eq!(result["dec"], json!([72, 105]));
            assert_eq!(result["roundtrip"], json!([0, 1, 2, 254, 255]));
            assert_eq!(result["lenient"], json!([77]));
            assert_eq!(result["whitespace"], json!([82, 97, 107, 117]));
            assert_eq!(result["unpadded"], json!([102]));
            assert_eq!(result["highBytes"], json!([128, 255, 0]));
        });
    }
}
