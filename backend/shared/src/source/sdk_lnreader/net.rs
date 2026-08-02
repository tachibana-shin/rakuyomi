//! Native `fetch` binding for the LNReader JS runtime.
//!
//! Keeps the native surface minimal: a single `__native_fetch` function that
//! takes plain strings/JSON (no JS object traversal on the Rust side) and
//! returns a plain data object. The `fetch`/`Response`/`FormData`/
//! `URLSearchParams` *shapes* JS code expects live entirely in
//! [`super::js_runtime`]'s prelude, as thin JS wrappers around this one call
//! — real browser fetch semantics aren't needed, just enough for LNReader
//! plugins built on `@libs/fetch`.
//!
//! The request itself goes through `reqwest`, built with the same TLS/proxy
//! configuration already used elsewhere in the backend
//! (`crate::tls::client_builder`, also used by `wasm_store.rs` and
//! `chapter_downloader.rs`) — not a separate, unconfigured client.

use std::collections::HashMap;
use std::str::FromStr;

use boa_engine::{
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, ObjectInitializer},
    property::Attribute,
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue,
};
use boa_gc::{empty_trace, Finalize, Trace};
use futures::executor;
use reqwest::Method;
use serde::Deserialize;

pub(super) fn build_client() -> reqwest::Client {
    crate::tls::client_builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client for LNReader sources")
}

fn js_error(message: impl Into<String>) -> JsError {
    JsNativeError::typ().with_message(message.into()).into()
}

/// `reqwest::Client` has no `Trace`/`Finalize` impl of its own (it's an
/// external type with no `JsValue`s inside it), so it can't be captured by a
/// boa `NativeFunction` directly. This newtype gives it an empty trace, the
/// same pattern already used for `JsContext`/`Canvas` in `wasm_store.rs`.
#[derive(Clone)]
struct ClientHandle(reqwest::Client);
// Safety: contains no `JsValue`s, nothing for the GC to trace.
unsafe impl Trace for ClientHandle {
    empty_trace!();
}
impl Finalize for ClientHandle {}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum FetchBody {
    None,
    String { value: String },
    Multipart { entries: Vec<(String, String)> },
}

struct FetchResult {
    ok: bool,
    status: u16,
    status_text: String,
    final_url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn do_fetch(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    headers_json: &str,
    body_json: &str,
) -> anyhow::Result<FetchResult> {
    let method = Method::from_str(method).unwrap_or(Method::GET);
    let headers: HashMap<String, String> = serde_json::from_str(headers_json).unwrap_or_default();
    let body: FetchBody = serde_json::from_str(body_json)?;

    let mut builder = client.request(method, url);
    // Real browser fetch always sends a real browser User-Agent; without
    // one, many sites either block the request outright or serve a
    // different/degraded page (found empty scrape results against real
    // sources without this). Same default WASM sources get
    // (`wasm_imports::net::DEFAULT_USER_AGENT`), only applied if the plugin
    // didn't already set its own.
    if !headers.keys().any(|k| k.eq_ignore_ascii_case("user-agent")) {
        builder = builder.header(
            "User-Agent",
            crate::source::wasm_imports::net::DEFAULT_USER_AGENT,
        );
    }
    for (name, value) in &headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder = match body {
        FetchBody::None => builder,
        FetchBody::String { value } => builder.body(value),
        FetchBody::Multipart { entries } => {
            let mut form = reqwest::multipart::Form::new();
            for (name, value) in entries {
                form = form.text(name, value);
            }
            builder.multipart(form)
        }
    };

    let response = executor::block_on(builder.send())?;
    let ok = response.status().is_success();
    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or_default()
        .to_string();
    let final_url = response.url().to_string();
    let response_headers: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    let bytes = executor::block_on(response.bytes())?.to_vec();

    Ok(FetchResult {
        ok,
        status,
        status_text,
        final_url,
        headers: response_headers,
        body: bytes,
    })
}

/// `__native_fetch(url, method, headersJson, bodyJson) -> object`. Blocks on
/// the actual HTTP call (same pattern as `wasm_imports/net.rs::send`), and
/// returns a plain data object (`__ok`/`__status`/`__body`/... properties,
/// no methods) — `js_runtime`'s JS-level `Response` wrapper turns that into
/// the `.ok`/`.status`/`.text()`/`.json()` shape LNReader plugins expect.
fn native_fetch(
    client: &ClientHandle,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let url = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let method = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();
    let headers_json = args
        .get_or_undefined(2)
        .to_string(context)?
        .to_std_string_escaped();
    let body_json = args
        .get_or_undefined(3)
        .to_string(context)?
        .to_std_string_escaped();

    let FetchResult {
        ok,
        status,
        status_text,
        final_url,
        headers,
        body: raw_body,
    } = do_fetch(&client.0, &url, &method, &headers_json, &body_json)
        .map_err(|e| js_error(format!("fetch failed for {url}: {e}")))?;

    let body = String::from_utf8_lossy(&raw_body).into_owned();
    let mut headers_builder = ObjectInitializer::new(context);
    for (name, value) in &headers {
        headers_builder.property(
            js_string!(name.to_lowercase().as_str()),
            js_string!(value.as_str()),
            Attribute::all(),
        );
    }
    let headers_object = headers_builder.build();

    let response = ObjectInitializer::new(context)
        .property(js_string!("__ok"), JsValue::from(ok), Attribute::all())
        .property(
            js_string!("__status"),
            JsValue::from(status as f64),
            Attribute::all(),
        )
        .property(
            js_string!("__statusText"),
            js_string!(status_text.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("__url"),
            js_string!(final_url.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("__body"),
            js_string!(body.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("__headers"),
            JsValue::from(headers_object),
            Attribute::all(),
        )
        .build();

    Ok(JsValue::from(response))
}

/// Registers `__native_fetch` as a global function in `context`, backed by
/// `client`.
pub(super) fn register(context: &mut Context, client: reqwest::Client) {
    let handle = ClientHandle(client);
    let native = NativeFunction::from_copy_closure_with_captures(
        move |_this, args, client, context| native_fetch(client, args, context),
        handle,
    );
    let func = FunctionObjectBuilder::new(context.realm(), native)
        .name("__native_fetch")
        .length(4)
        .build();
    context
        .global_object()
        .set(js_string!("__native_fetch"), func, false, context)
        .expect("registering __native_fetch should not fail");
}
