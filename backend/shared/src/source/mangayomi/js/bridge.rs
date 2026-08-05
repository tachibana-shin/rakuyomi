//! Host side of the MangaYomi JavaScript bridge.
//!
//! Mirrors the mangayomi app's `eval/javascript/*.dart` handlers: the JS
//! polyfill calls `sendMessage(name, argsJson)` and the host dispatches to
//! the matching handler, returning the same strings the app would produce
//! (raw values for element keys / preferences, JSON for arrays, the full
//! `Response.toJson()` serialisation for HTTP).
//!
//! All handlers run on the worker thread; the mutable bridge state
//! ([`JsBridge`]) lives in a thread-local, mirroring the Dart backend's
//! `STATE` thread-local.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context as _, Result};
use dom_query::{NodeId, NodeRef};
use rquickjs::{Ctx, Exception};
use serde_json::{json, Value};

use crate::settings::SourceSettingValue;
use crate::source::mangayomi::html::{attr_regex, MangaYomiDom};

use super::crypto;

/// Shared per-worker bridge state (never leaves the worker thread).
pub(crate) struct JsBridge {
    pub client: reqwest::blocking::Client,
    pub dom: MangaYomiDom,
    /// Element registry: polyfill key -> `(document, handle)`. `None` slots
    /// mirror the app storing `null` elements (e.g. a `selectFirst` miss).
    pub elements: HashMap<u32, Option<(usize, usize)>>,
    pub next_key: u32,
    /// Source preference values (`SharedPreferences`), shared with the
    /// source so settings updates survive worker restarts.
    pub prefs: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
}

thread_local! {
    static BRIDGE: RefCell<Option<Rc<RefCell<JsBridge>>>> = const { RefCell::new(None) };
}

fn state() -> Rc<RefCell<JsBridge>> {
    BRIDGE.with(|s| {
        s.borrow()
            .as_ref()
            .expect("mangayomi js bridge state")
            .clone()
    })
}

/// Installs the bridge state on the worker thread before any JS runs.
pub(crate) fn install(bridge: JsBridge) {
    BRIDGE.with(|s| *s.borrow_mut() = Some(Rc::new(RefCell::new(bridge))));
}

/// The `sendMessage` host function. Returns the string the app's handler
/// would return; JS errors surface as thrown exceptions.
pub fn host_send_message(
    ctx: Ctx<'_>,
    name: String,
    args_json: String,
) -> rquickjs::Result<String> {
    dispatch(&name, &args_json)
        .map_err(|e| Exception::throw_message(&ctx, &format!("sendMessage({name}): {:#}", e)))
}

fn dispatch(name: &str, args_json: &str) -> Result<String> {
    let args: Value = serde_json::from_str(args_json)
        .unwrap_or(Value::Null)
        .as_array()
        .cloned()
        .map(Value::Array)
        .unwrap_or(Value::Array(Vec::new()));
    match name {
        "log" => host_log(&args),
        "http_head" | "http_get" | "http_post" | "http_put" | "http_delete" | "http_patch" => {
            host_http(name, &args)
        }
        "get" => host_pref_get(&args),
        "getString" => host_pref_get_string(&args),
        "setString" => host_pref_set_string(&args),
        "cryptoHandler" => Ok(crypto::crypto_handler(
            &arg_str(&args, 0)?,
            &arg_str(&args, 1)?,
            &arg_str(&args, 2)?,
            arg_bool(&args, 3).unwrap_or(true),
        )),
        "encryptAESCryptoJS" => Ok(crypto::encrypt_aes_crypto_js(
            &arg_str(&args, 0)?,
            &arg_str(&args, 1)?,
        )),
        "decryptAESCryptoJS" => Ok(crypto::decrypt_aes_crypto_js(
            &arg_str(&args, 0)?,
            &arg_str(&args, 1)?,
        )),
        "decryptAESGCM" => Ok(crypto::decrypt_aes_gcm(
            &arg_str(&args, 0)?,
            &arg_str(&args, 1)?,
            &arg_str(&args, 2)?,
            &arg_str(&args, 3)?,
        )),
        "deobfuscateJsPassword" => Ok(crypto::deobfuscate_js_password(&arg_str(&args, 0)?)),
        "unpackJs" | "unpackJsAndCombine" => Ok(crypto::unpack_js(&arg_str(&args, 0)?)),
        // Not provided by the host environment; the extensions that call
        // these expect real values only on the app.
        "parseDates" | "parseEpub" | "parseEpubChapter" => Ok(String::new()),
        "evaluateJavascriptViaWebview" => Ok("false".to_string()),
        // DOM bridge (see `js_dom.rs`).
        "get_doc_element" => dom::get_doc_element(&args),
        "get_doc_string" => dom::get_doc_string(&args),
        "get_element_string" => dom::get_element_string(&args),
        "doc_select_first" => dom::doc_select_first(&args),
        "ele_selectFirst" => dom::ele_select_first(&args),
        "ele_element_sibling" => dom::ele_element_sibling(&args),
        "ele_attr" => dom::ele_attr(&args),
        "doc_attr" => dom::doc_attr(&args),
        "ele_has_attr" => dom::ele_has_attr(&args),
        "doc_has_attr" => dom::doc_has_attr(&args),
        // xpath is stubbed (no extension uses it, same as the Dart backend).
        "doc_xpath_first" | "ele_xpathFirst" | "xpathFirst" => Ok(String::new()),
        "doc_xpath" | "ele_xpath" | "xpath" => Ok("[]".to_string()),
        "doc_get_elements_by" => dom::doc_get_elements_by(&args),
        "ele_get_elements_by" => dom::ele_get_elements_by(&args),
        "doc_get_element_by_id" => dom::doc_get_element_by_id(&args),
        "doc_select" => dom::doc_select(&args),
        "ele_select" => dom::ele_select(&args),
        other => Err(anyhow!("unhandled message `{other}`")),
    }
}

fn arg_str(args: &Value, index: usize) -> Result<String> {
    match args.get(index) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        Some(Value::Null) | None => Ok(String::new()),
        Some(other) => Ok(other.to_string()),
    }
}

fn arg_bool(args: &Value, index: usize) -> Option<bool> {
    args.get(index).and_then(Value::as_bool)
}

fn host_log(args: &Value) -> Result<String> {
    let message = arg_str(args, 0)?;
    log::info!("[mangayomi-js] {}", message);
    Ok("null".to_string())
}

fn host_pref_get(args: &Value) -> Result<String> {
    let key = arg_str(args, 0)?;
    let prefs = state().borrow().prefs.clone();
    let value = prefs.lock().unwrap().get(&key).cloned();
    // The app returns the raw stored value (null when missing); the JS side
    // uses `|| fallback`, so missing keys must stay falsy.
    Ok(match value {
        Some(SourceSettingValue::String(s)) => s,
        Some(SourceSettingValue::Bool(b)) => b.to_string(),
        Some(SourceSettingValue::Vec(v)) => serde_json::to_string(&v).unwrap_or_default(),
        Some(SourceSettingValue::Int(i)) => i.to_string(),
        Some(SourceSettingValue::Float(f)) => f.to_string(),
        Some(SourceSettingValue::Data(_)) | Some(SourceSettingValue::Null) | None => String::new(),
    })
}

fn host_pref_get_string(args: &Value) -> Result<String> {
    let key = arg_str(args, 0)?;
    let default = arg_str(args, 1)?;
    let prefs = state().borrow().prefs.clone();
    let value = prefs.lock().unwrap().get(&key).cloned();
    Ok(match value {
        Some(SourceSettingValue::String(s)) => s,
        Some(SourceSettingValue::Bool(b)) => b.to_string(),
        _ => default,
    })
}

fn host_pref_set_string(args: &Value) -> Result<String> {
    let key = arg_str(args, 0)?;
    let value = arg_str(args, 1)?;
    let prefs = state().borrow().prefs.clone();
    prefs
        .lock()
        .unwrap()
        .insert(key, SourceSettingValue::String(value));
    Ok("null".to_string())
}

fn host_http(message: &str, args: &Value) -> Result<String> {
    let method = message.trim_start_matches("http_");
    let url = arg_str(args, 2)?;
    if url.is_empty() {
        return Err(anyhow!("missing request url"));
    }
    let bridge = state();
    let bridge = bridge.borrow();
    let client = &bridge.client;

    let method = reqwest::Method::from_bytes(method.to_uppercase().as_bytes())
        .map_err(|_| anyhow!("invalid HTTP method `{method}`"))?;
    let mut builder = client
        .request(method.clone(), &url)
        .header(reqwest::header::ACCEPT, "*/*");

    let headers = args.get(3);
    let mut sent_headers: HashMap<String, String> = HashMap::new();
    if let Some(Value::Object(map)) = headers {
        for (name, value) in map {
            if let Some(value) = value.as_str() {
                builder = builder.header(name.as_str(), value);
                sent_headers.insert(name.clone(), value.to_string());
            }
        }
    }

    // The app JSON-encodes the body when the content type is JSON.
    let body = args.get(4);
    let content_type = sent_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_lowercase())
        .unwrap_or_default();
    if !matches!(body, None | Some(Value::Null)) {
        let body = body.expect("checked");
        if content_type.contains("application/json") {
            let encoded =
                serde_json::to_string(body).context("failed to serialise JSON request body")?;
            builder = builder.body(encoded);
        } else {
            match body {
                Value::String(s) => builder = builder.body(s.clone()),
                Value::Array(bytes) if bytes.iter().all(|b| b.is_u64()) => {
                    let bytes: Vec<u8> = bytes
                        .iter()
                        .filter_map(|b| b.as_u64().map(|b| b as u8))
                        .collect();
                    builder = builder.body(bytes);
                }
                other => {
                    let encoded =
                        serde_json::to_string(other).context("failed to serialise request body")?;
                    builder = builder.body(encoded);
                }
            }
        }
    }

    let mut request = builder
        .build()
        .with_context(|| format!("failed to build request for {url}"))?;

    // Per-domain user-agent / cookie overrides, uniform with the other
    // backends so cookie-synced sessions work.
    if let Some(host) = request.url().host_str() {
        let (override_ua, cookie_value) =
            crate::cookie_store::get_user_agent_and_cookie_header(host);
        if let Some(ua) = override_ua {
            if let Ok(header) = reqwest::header::HeaderValue::from_str(&ua) {
                request
                    .headers_mut()
                    .insert(reqwest::header::USER_AGENT, header);
                sent_headers.insert("user-agent".to_string(), ua);
            }
        }
        if let Some(cookies) = cookie_value {
            if let Ok(header) = reqwest::header::HeaderValue::from_str(&cookies) {
                request
                    .headers_mut()
                    .insert(reqwest::header::COOKIE, header);
                sent_headers.insert("cookie".to_string(), cookies);
            }
        }
    }

    let response = client
        .execute(request)
        .with_context(|| format!("request to {url} failed"))?;
    let status_code = response.status().as_u16();
    let is_redirect = response.status().is_redirection();
    let reason_phrase = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let final_url = response.url().to_string();
    let mut response_headers: HashMap<String, String> = HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            response_headers.insert(name.to_string(), value.to_string());
        }
    }
    let body_bytes = response.bytes().unwrap_or_default();
    let body = if method == reqwest::Method::HEAD {
        String::new()
    } else {
        String::from_utf8_lossy(&body_bytes).into_owned()
    };

    Ok(json!({
        "body": body,
        "headers": response_headers,
        "isRedirect": is_redirect,
        "persistentConnection": null,
        "reasonPhrase": reason_phrase,
        "statusCode": status_code,
        "request": {
            "contentLength": null,
            "finalized": null,
            "followRedirects": null,
            "headers": sent_headers,
            "maxRedirects": null,
            "method": message.trim_start_matches("http_").to_uppercase(),
            "persistentConnection": null,
            "url": final_url,
        },
    })
    .to_string())
}

// ---------------------------------------------------------------------------
// DOM handlers (port of the app's `JsDomSelector`)
// ---------------------------------------------------------------------------

mod dom {
    use super::*;

    /// Registers an element (or a null slot) and returns its polyfill key.
    fn register(st: &mut JsBridge, doc_id: usize, node: Option<NodeId>) -> String {
        st.next_key += 1;
        let slot = node.map(|id| (doc_id, st.dom.store_id(doc_id, id)));
        st.elements.insert(st.next_key, slot);
        st.next_key.to_string()
    }

    fn register_many(st: &mut JsBridge, doc_id: usize, nodes: Vec<NodeId>) -> String {
        let keys: Vec<String> = nodes
            .into_iter()
            .map(|id| register(st, doc_id, Some(id)))
            .collect();
        serde_json::to_string(&keys).unwrap_or_else(|_| "[]".to_string())
    }

    /// Parses `html` into a fresh document and returns its root node id.
    fn parse_root(html: &str) -> (usize, NodeId) {
        let st = state();
        let doc_id = st.borrow_mut().dom.parse(html);
        let root = st
            .borrow()
            .dom
            .node(doc_id, 0)
            .expect("mangayomi js: missing document root")
            .id;
        (doc_id, root)
    }

    /// Resolves a polyfill key to its node (scoped to the owning document).
    fn node_of(st: &JsBridge, key: u32) -> Option<NodeRef<'_>> {
        let (doc_id, handle) = st.elements.get(&key).copied().flatten()?;
        st.dom.node(doc_id, handle)
    }

    fn element_string(st: &JsBridge, key: u32, kind: &str) -> String {
        let Some((doc_id, handle)) = st.elements.get(&key).copied().flatten() else {
            return String::new();
        };
        let Some(node) = st.dom.node(doc_id, handle) else {
            return String::new();
        };
        match kind {
            "text" => node.text().trim().to_string(),
            "innerHtml" => node.inner_html().to_string(),
            "outerHtml" => node.html().to_string(),
            "className" => node.attr("class").unwrap_or_default().to_string(),
            "localName" => node.node_name().unwrap_or_default().to_string(),
            "namespaceUri" => String::new(),
            "getSrc" => node
                .attr("src")
                .map(|v| v.to_string())
                .or_else(|| attr_regex(&node.html(), "src"))
                .unwrap_or_default(),
            "getImg" => node
                .attr("img")
                .map(|v| v.to_string())
                .or_else(|| attr_regex(&node.html(), "img"))
                .unwrap_or_default(),
            "getHref" => node
                .attr("href")
                .map(|v| v.to_string())
                .or_else(|| attr_regex(&node.html(), "href"))
                .unwrap_or_default(),
            "getDataSrc" => node
                .attr("data-src")
                .map(|v| v.to_string())
                .or_else(|| attr_regex(&node.html(), "data-src"))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn select_ids(st: &JsBridge, node: NodeRef<'_>, selector: &str) -> Vec<NodeId> {
        st.dom.select(node, selector)
    }

    fn select_first_id(st: &JsBridge, node: NodeRef<'_>, selector: &str) -> Option<NodeId> {
        st.dom.select_first(node, selector)
    }

    fn root_node(st: &JsBridge, doc_id: usize) -> NodeRef<'_> {
        st.dom.node(doc_id, 0).expect("mangayomi js: root node")
    }

    pub(super) fn get_doc_element(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let kind = arg_str(args, 1)?;
        let (doc_id, root) = parse_root(&html);
        let st = state();
        let mut b = st.borrow_mut();
        let node = match kind.as_str() {
            "body" => select_first_id(&b, root_node(&b, doc_id), "body"),
            "head" => select_first_id(&b, root_node(&b, doc_id), "head"),
            "documentElement" => select_first_id(&b, root_node(&b, doc_id), "html"),
            _ => Some(root),
        };
        Ok(register(&mut b, doc_id, node))
    }

    pub(super) fn get_doc_string(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let kind = arg_str(args, 1)?;
        let (doc_id, _) = parse_root(&html);
        let st = state();
        let b = st.borrow();
        let node = b.dom.node(doc_id, 0).expect("root node");
        Ok(match kind.as_str() {
            "text" => node.text().trim().to_string(),
            _ => node.html().to_string(),
        })
    }

    pub(super) fn get_element_string(args: &Value) -> Result<String> {
        let kind = arg_str(args, 0)?;
        let key = arg_str(args, 1)?.parse::<u32>().unwrap_or(0);
        let st = state();
        let b = st.borrow();
        Ok(element_string(&b, key, &kind))
    }

    pub(super) fn doc_select_first(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let selector = arg_str(args, 1)?;
        let (doc_id, _) = parse_root(&html);
        let st = state();
        let mut b = st.borrow_mut();
        let node = select_first_id(&b, root_node(&b, doc_id), &selector);
        Ok(register(&mut b, doc_id, node))
    }

    pub(super) fn ele_select_first(args: &Value) -> Result<String> {
        let selector = arg_str(args, 0)?;
        let key = arg_str(args, 1)?.parse::<u32>().unwrap_or(0);
        let st = state();
        let mut b = st.borrow_mut();
        let node = node_of(&b, key);
        let doc_id = node
            .map(|_| {
                b.elements
                    .get(&key)
                    .copied()
                    .flatten()
                    .map(|(d, _)| d)
                    .expect("checked")
            })
            .unwrap_or(0);
        let id = node.and_then(|n| select_first_id(&b, n, &selector));
        Ok(register(&mut b, doc_id, id))
    }

    pub(super) fn ele_element_sibling(args: &Value) -> Result<String> {
        let kind = arg_str(args, 0)?;
        let key = arg_str(args, 1)?.parse::<u32>().unwrap_or(0);
        let st = state();
        let mut b = st.borrow_mut();
        let node = node_of(&b, key);
        let doc_id = node
            .map(|_| {
                b.elements
                    .get(&key)
                    .copied()
                    .flatten()
                    .map(|(d, _)| d)
                    .expect("checked")
            })
            .unwrap_or(0);
        let id = node.and_then(|n| {
            match kind.as_str() {
                "nextElementSibling" => n.next_element_sibling(),
                _ => n.prev_element_sibling(),
            }
            .map(|n| n.id)
        });
        Ok(register(&mut b, doc_id, id))
    }

    pub(super) fn ele_attr(args: &Value) -> Result<String> {
        let attr = arg_str(args, 0)?;
        let key = arg_str(args, 1)?.parse::<u32>().unwrap_or(0);
        let st = state();
        let b = st.borrow();
        Ok(match node_of(&b, key) {
            Some(node) => node.attr(&attr).map(|v| v.to_string()).unwrap_or_default(),
            None => String::new(),
        })
    }

    pub(super) fn doc_attr(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let attr = arg_str(args, 1)?;
        let (doc_id, _) = parse_root(&html);
        let st = state();
        let b = st.borrow();
        // The app's `Document.attr` reads the *document node's own*
        // attributes map, which the parser leaves empty (`package:html`
        // attributes live on `documentElement`), so document-level attribute
        // lookups always miss. Verified against `html 0.15.6` with Dart.
        Ok(b.dom
            .node(doc_id, 0)
            .expect("doc attr node")
            .attr(&attr)
            .map(|v| v.to_string())
            .unwrap_or_default())
    }

    /// The app's `Element.hasAttr` always resolves to `false` (it passes
    /// `this.html` instead of `this.key`); kept for parity. Empty string is
    /// falsy in JS, matching the boolean the app returns.
    pub(super) fn ele_has_attr(_args: &Value) -> Result<String> {
        Ok(String::new())
    }

    pub(super) fn doc_has_attr(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let attr = arg_str(args, 1)?;
        let (doc_id, _) = parse_root(&html);
        let st = state();
        let b = st.borrow();
        // Same as `doc_attr`: the document node's own attributes map is
        // empty, so `hasAtr` on the document is always false in the app
        // (verified against `html 0.15.6` with Dart).
        let has = b
            .dom
            .node(doc_id, 0)
            .expect("doc has attr node")
            .has_attr(&attr);
        // Empty string is falsy in JS, matching the boolean the app returns.
        Ok(if has {
            "true".to_string()
        } else {
            String::new()
        })
    }

    pub(super) fn doc_get_elements_by(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let kind = arg_str(args, 1)?;
        let name = arg_str(args, 2)?;
        let (doc_id, _) = parse_root(&html);
        let st = state();
        let mut b = st.borrow_mut();
        let ids = match kind.as_str() {
            "children" => root_node(&b, doc_id)
                .element_children()
                .iter()
                .map(|n| n.id)
                .collect(),
            "getElementsByTagName" => select_ids(&b, root_node(&b, doc_id), &name),
            _ => select_ids(&b, root_node(&b, doc_id), &format!(".{}", name)),
        };
        Ok(register_many(&mut b, doc_id, ids))
    }

    pub(super) fn ele_get_elements_by(args: &Value) -> Result<String> {
        let kind = arg_str(args, 0)?;
        let name = arg_str(args, 1)?;
        let key = arg_str(args, 2)?.parse::<u32>().unwrap_or(0);
        let st = state();
        let mut b = st.borrow_mut();
        let node = node_of(&b, key);
        let doc_id = node
            .map(|_| {
                b.elements
                    .get(&key)
                    .copied()
                    .flatten()
                    .map(|(d, _)| d)
                    .expect("checked")
            })
            .unwrap_or(0);
        let ids = node
            .map(|n| match kind.as_str() {
                "children" => n.element_children().iter().map(|c| c.id).collect(),
                "getElementsByTagName" => select_ids(&b, n, &name),
                _ => select_ids(&b, n, &format!(".{}", name)),
            })
            .unwrap_or_default();
        Ok(register_many(&mut b, doc_id, ids))
    }

    pub(super) fn doc_get_element_by_id(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let id = arg_str(args, 1)?;
        let (doc_id, _) = parse_root(&html);
        let st = state();
        let mut b = st.borrow_mut();
        let node = select_first_id(&b, root_node(&b, doc_id), &format!("#{}", id));
        Ok(register(&mut b, doc_id, node))
    }

    pub(super) fn doc_select(args: &Value) -> Result<String> {
        let html = arg_str(args, 0)?;
        let selector = arg_str(args, 1)?;
        let (doc_id, _) = parse_root(&html);
        let st = state();
        let mut b = st.borrow_mut();
        let ids = select_ids(&b, root_node(&b, doc_id), &selector);
        Ok(register_many(&mut b, doc_id, ids))
    }

    pub(super) fn ele_select(args: &Value) -> Result<String> {
        let selector = arg_str(args, 0)?;
        let key = arg_str(args, 1)?.parse::<u32>().unwrap_or(0);
        let st = state();
        let mut b = st.borrow_mut();
        let node = node_of(&b, key);
        let doc_id = node
            .map(|_| {
                b.elements
                    .get(&key)
                    .copied()
                    .flatten()
                    .map(|(d, _)| d)
                    .expect("checked")
            })
            .unwrap_or(0);
        let ids = node
            .map(|n| select_ids(&b, n, &selector))
            .unwrap_or_default();
        Ok(register_many(&mut b, doc_id, ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn test_bridge() {
        install(JsBridge {
            client: crate::tls::blocking_client_builder()
                .build()
                .expect("blocking client"),
            dom: MangaYomiDom::new(),
            elements: HashMap::new(),
            next_key: 0,
            prefs: Arc::new(Mutex::new(HashMap::new())),
        });
    }

    fn call(name: &str, args: &str) -> Result<String> {
        dispatch(name, args)
    }

    /// Serialises the handler arguments as a JSON array (escaping the HTML
    /// fixtures, which contain quotes).
    fn js(args: &[Value]) -> String {
        serde_json::to_string(args).unwrap()
    }

    const HTML: &str = r#"<!DOCTYPE html><html lang="en"><head><title>t</title></head>
<body><div class="item"><a class="title" href="/manga/1">One</a><img src="/img/1.jpg"></div>
<div class="item"><a class="title" href="/manga/2">Two</a><img src="/img/2.jpg"></div>
<ul id="list"><li>a</li><li>b</li><li>c</li></ul></body></html>"#;

    #[test]
    fn log_coerces_values_and_returns_null() {
        test_bridge();
        assert_eq!(call("log", r#"["hello world"]"#).unwrap(), "null");
        assert_eq!(call("log", r#"[42]"#).unwrap(), "null");
        assert_eq!(call("log", r#"[]"#).unwrap(), "null");
        assert_eq!(call("log", r#"["a", null]"#).unwrap(), "null");
    }

    #[test]
    fn unhandled_message_is_an_error() {
        test_bridge();
        assert!(call("does_not_exist", "[]")
            .unwrap_err()
            .to_string()
            .contains("unhandled message"));
    }

    #[test]
    fn non_array_args_are_tolerated() {
        test_bridge();
        assert_eq!(call("log", "null").unwrap(), "null");
        assert_eq!(call("log", r#"{"k":1}"#).unwrap(), "null");
    }

    #[test]
    fn preference_get_missing_key_is_empty() {
        test_bridge();
        assert_eq!(call("get", r#"["site"]"#).unwrap(), "");
    }

    #[test]
    fn preference_set_and_read_back() {
        test_bridge();
        assert_eq!(
            call("setString", r#"["site","https://example.com"]"#).unwrap(),
            "null"
        );
        assert_eq!(
            call("getString", r#"["site","fallback"]"#).unwrap(),
            "https://example.com"
        );
        assert_eq!(call("get", r#"["site"]"#).unwrap(), "https://example.com");
    }

    #[test]
    fn preference_get_string_falls_back_to_default() {
        test_bridge();
        assert_eq!(
            call("getString", r#"["missing","fallback"]"#).unwrap(),
            "fallback"
        );
        assert_eq!(call("getString", r#"["missing", 7]"#).unwrap(), "7");
    }

    #[test]
    fn preference_get_overwrites_install_state() {
        test_bridge();
        let prefs = state().borrow().prefs.clone();
        prefs.lock().unwrap().insert(
            "site".to_string(),
            SourceSettingValue::String("https://installed".to_string()),
        );
        assert_eq!(call("get", r#"["site"]"#).unwrap(), "https://installed");
        assert_eq!(
            call("getString", r#"["site","fb"]"#).unwrap(),
            "https://installed"
        );
    }

    #[test]
    fn crypto_roundtrips_through_dispatch() {
        test_bridge();
        let encrypted = call("encryptAESCryptoJS", r#"["hello world","secret"]"#).unwrap();
        assert!(encrypted.starts_with("U2FsdGVkX1"));
        let decrypted = call(
            "decryptAESCryptoJS",
            &format!(r#"["{encrypted}","secret"]"#),
        )
        .unwrap();
        assert_eq!(decrypted, "hello world");

        let key = "0123456789abcdef0123456789abcdef";
        let iv = "0123456789abcdef";
        let ct = call(
            "cryptoHandler",
            &format!(r#"["payload","{iv}","{key}",true]"#),
        )
        .unwrap();
        assert_ne!(ct, "payload");
        let pt = call(
            "cryptoHandler",
            &format!(r#"["{ct}","{iv}","{key}",false]"#),
        )
        .unwrap();
        assert_eq!(pt, "payload");
        // encrypt defaults to `encrypt=true` when the flag is omitted.
        let ct_default = call("cryptoHandler", &format!(r#"["payload2","{iv}","{key}"]"#)).unwrap();
        let pt_default = call(
            "cryptoHandler",
            &format!(r#"["{ct_default}","{iv}","{key}",false]"#),
        )
        .unwrap();
        assert_eq!(pt_default, "payload2");
    }

    #[test]
    fn deobfuscate_and_unpack_through_dispatch() {
        test_bridge();
        assert_eq!(
            call("deobfuscateJsPassword", r#"["[!+[]+!+[]]"]"#).unwrap(),
            "2"
        );
        let packed = r#"eval(function(p,a,c,k,e,r){e=String;while(c--)r[c]=k[c]||c;k=[function(e){return r[e]}];e=function(){return'\\w+'};c=1};while(c--)if(k[c])p=p.replace(new RegExp('\\b'+e(c)+'\\b','g'),k[c]);return p;}('1 0=2.3();',4,4,'a|var|document|createElement'.split('|'),0,{}))"#;
        for name in ["unpackJs", "unpackJsAndCombine"] {
            let out = call(name, &format!(r#"["{packed}"]"#)).unwrap();
            assert_eq!(out.trim(), "var a=document.createElement();");
        }
        assert_eq!(call("unpackJs", r#"["not packed at all"]"#).unwrap(), "");
    }

    #[test]
    fn app_only_messages_are_stubbed() {
        test_bridge();
        for name in ["parseDates", "parseEpub", "parseEpubChapter"] {
            assert_eq!(call(name, r#"[[],0,[]]"#).unwrap(), "");
        }
        assert_eq!(
            call("evaluateJavascriptViaWebview", r#"["var x = 1;"]"#).unwrap(),
            "false"
        );
        assert_eq!(call("xpath", r#"["//a", 1]"#).unwrap(), "[]");
        assert_eq!(call("xpathFirst", r#"["//a", 5]"#).unwrap(), "");
    }

    #[test]
    fn doc_select_first_and_element_access() {
        test_bridge();
        // First registered key starts at 1.
        let key = call("doc_select_first", &js(&[json!(HTML), json!("a.title")])).unwrap();
        assert_eq!(key, "1");
        assert_eq!(
            call("get_element_string", &js(&[json!("text"), json!("1")])).unwrap(),
            "One"
        );
        assert_eq!(
            call("get_element_string", &js(&[json!("outerHtml"), json!("1")])).unwrap(),
            r#"<a class="title" href="/manga/1">One</a>"#
        );
        assert_eq!(
            call("ele_attr", &js(&[json!("href"), json!("1")])).unwrap(),
            "/manga/1"
        );
        assert_eq!(
            call("ele_attr", &js(&[json!("class"), json!("1")])).unwrap(),
            "title"
        );

        // Descendant select from the element and its sub-attributes.
        // The img is a sibling of the anchor; select the .item wrapper first.
        let item = call("doc_select_first", &js(&[json!(HTML), json!("div.item")])).unwrap();
        let img_key = call("ele_select", &js(&[json!("img"), json!(item)])).unwrap();
        let img_key: Value = serde_json::from_str(&img_key).unwrap();
        let img_key = img_key[0].as_str().unwrap().to_string();
        assert_eq!(
            call("ele_attr", &js(&[json!("src"), json!(img_key)])).unwrap(),
            "/img/1.jpg"
        );

        // Unmatched attribute is empty, not an error.
        assert_eq!(
            call("ele_attr", &js(&[json!("unknown"), json!("1")])).unwrap(),
            ""
        );
    }

    #[test]
    fn select_first_miss_registers_null_slot() {
        test_bridge();
        let key = call("doc_select_first", &js(&[json!(HTML), json!("p.missing")])).unwrap();
        assert_eq!(
            call("get_element_string", &js(&[json!("text"), json!(key)])).unwrap(),
            ""
        );
        assert_eq!(
            call("ele_attr", &js(&[json!("href"), json!(key)])).unwrap(),
            ""
        );
        let list = call("ele_select", &js(&[json!("a"), json!(key)])).unwrap();
        assert_eq!(list, "[]");
    }

    #[test]
    fn doc_string_and_attr_handlers() {
        test_bridge();
        assert!(!call("get_doc_string", &js(&[json!(HTML), json!("text")]))
            .unwrap()
            .is_empty());
        assert!(call("get_doc_string", &js(&[json!(HTML), json!("other")]))
            .unwrap()
            .contains("html"));
        // Document-level attribute lookups always miss: the app reads the
        // document node's own (empty) attributes map, not the `<html>`
        // element's (verified against package:html 0.15.6 with Dart).
        assert_eq!(
            call("doc_attr", &js(&[json!(HTML), json!("lang")])).unwrap(),
            ""
        );
        assert_eq!(
            call("doc_attr", &js(&[json!(HTML), json!("missing")])).unwrap(),
            ""
        );
        assert_eq!(
            call("doc_has_attr", &js(&[json!(HTML), json!("lang")])).unwrap(),
            ""
        );
        assert_eq!(
            call("doc_has_attr", &js(&[json!(HTML), json!("missing")])).unwrap(),
            ""
        );
        // Element.hasAttr is always falsy on this backend.
        assert_eq!(
            call("ele_has_attr", &js(&[json!("class"), json!("1")])).unwrap(),
            ""
        );
    }

    #[test]
    fn get_doc_element_and_document_children() {
        test_bridge();
        let key = call("get_doc_element", &js(&[json!(HTML), json!("unknown")])).unwrap();
        assert_eq!(key, "1");
        // Document children are the document element; its children are
        // head + body.
        let children = call(
            "doc_get_elements_by",
            &js(&[json!(HTML), json!("children"), json!("")]),
        )
        .unwrap();
        assert_eq!(children, r#"["2"]"#);
        assert_eq!(
            call("get_element_string", &js(&[json!("localName"), json!("2")])).unwrap(),
            "html"
        );
        let html_kids = call(
            "ele_get_elements_by",
            &js(&[json!("children"), json!(""), json!("2")]),
        )
        .unwrap();
        assert_eq!(html_kids, r#"["3","4"]"#);
        assert_eq!(
            call("get_element_string", &js(&[json!("localName"), json!("4")])).unwrap(),
            "body"
        );
    }

    #[test]
    fn get_elements_by_tag_and_class() {
        test_bridge();
        let by_tag = call(
            "doc_get_elements_by",
            &js(&[json!(HTML), json!("getElementsByTagName"), json!("div")]),
        )
        .unwrap();
        assert_eq!(by_tag, r#"["1","2"]"#);
        let by_class = call(
            "doc_get_elements_by",
            &js(&[json!(HTML), json!("getElementsByClassName"), json!("item")]),
        )
        .unwrap();
        assert_eq!(by_class, r#"["3","4"]"#);
        // Same selectors scoped to a registered element.
        let body = call("get_doc_element", &js(&[json!(HTML), json!("body")])).unwrap();
        assert_eq!(body, "5");
        let scoped = call(
            "ele_get_elements_by",
            &js(&[json!("getElementsByTagName"), json!("div"), json!(body)]),
        )
        .unwrap();
        assert_eq!(scoped, r#"["6","7"]"#);
        let li = call("doc_select", &js(&[json!(HTML), json!("li")])).unwrap();
        let li: Vec<String> = serde_json::from_str(&li).unwrap();
        assert_eq!(li, vec!["8".to_string(), "9".to_string(), "10".to_string()]);
        assert_eq!(li.len(), 3);
        // Children of the <ul> (registered through get_element_by_id) are the
        // same three <li>s, re-registered under new keys.
        let ul = call("doc_get_element_by_id", &js(&[json!(HTML), json!("list")])).unwrap();
        let kids = call(
            "ele_get_elements_by",
            &js(&[json!("children"), json!(""), json!(ul)]),
        )
        .unwrap();
        let kids: Vec<String> = serde_json::from_str(&kids).unwrap();
        assert_eq!(kids.len(), 3);
        assert_eq!(
            call(
                "get_element_string",
                &js(&[json!("text"), json!(kids[1].clone())])
            )
            .unwrap(),
            "b"
        );
    }

    #[test]
    fn sibling_navigation() {
        test_bridge();
        let keys = call(
            "doc_get_elements_by",
            &js(&[json!(HTML), json!("getElementsByTagName"), json!("li")]),
        )
        .unwrap();
        let keys: Vec<String> = serde_json::from_str(&keys).unwrap();
        assert_eq!(
            call(
                "get_element_string",
                &js(&[json!("text"), json!(keys[1].clone())])
            )
            .unwrap(),
            "b"
        );
        // Siblings are re-registered under new keys; compare text instead.
        let next = call(
            "ele_element_sibling",
            &js(&[json!("nextElementSibling"), json!(keys[0].clone())]),
        )
        .unwrap();
        assert_eq!(
            call("get_element_string", &js(&[json!("text"), json!(next)])).unwrap(),
            "b"
        );
        let prev = call(
            "ele_element_sibling",
            &js(&[json!("previousElementSibling"), json!(keys[1].clone())]),
        )
        .unwrap();
        assert_eq!(
            call("get_element_string", &js(&[json!("text"), json!(prev)])).unwrap(),
            "a"
        );
        // Boundary siblings are unset -> null slot.
        let none = call(
            "ele_element_sibling",
            &js(&[json!("previousElementSibling"), json!(keys[0].clone())]),
        )
        .unwrap();
        assert_eq!(
            call("get_element_string", &js(&[json!("text"), json!(none)])).unwrap(),
            ""
        );
    }

    #[test]
    fn get_src_and_get_data_src_fall_back_to_regex() {
        test_bridge();
        // An element without a `src` attribute falls back to the first
        // `src="..."` occurrence in its outer HTML.
        let html = r#"<div id="c"><img src="/cover.jpg"></div>"#;
        let key = call("doc_get_element_by_id", &js(&[json!(html), json!("c")])).unwrap();
        assert_eq!(
            call("get_element_string", &js(&[json!("getSrc"), json!(key)])).unwrap(),
            "/cover.jpg"
        );
        let html2 = r#"<img data-src="/art/lazy.jpg">"#;
        let key2 = call("doc_select_first", &js(&[json!(html2), json!("img")])).unwrap();
        assert_eq!(
            call(
                "get_element_string",
                &js(&[json!("getDataSrc"), json!(key2)])
            )
            .unwrap(),
            "/art/lazy.jpg"
        );
        assert_eq!(
            call(
                "get_element_string",
                &js(&[json!("namespaceUri"), json!(key2)])
            )
            .unwrap(),
            ""
        );
        // No matching src/href -> empty.
        assert_eq!(
            call("get_element_string", &js(&[json!("getHref"), json!(key2)])).unwrap(),
            ""
        );
    }

    /// Reads one HTTP request (head + content-length body) off the stream.
    fn read_request(stream: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 2048];
        let mut header_end = 0;
        for _ in 0..64 {
            let n = stream.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            let before = buf.len();
            buf.extend_from_slice(&tmp[..n]);
            if let Some(p) = buf[before..].windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = before + p + 4;
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf[..header_end.min(buf.len())]);
        let len: usize = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse().ok())
                    .flatten()
            })
            .unwrap_or(0);
        while buf.len() < header_end + len {
            let n = stream.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Serves each incoming connection with `handler(entire_request)` and
    /// responds from the returned `(status, extra_headers, body)` triple.
    fn spawn_server(
        mut handler: impl FnMut(&str) -> (u16, String, String) + Send + 'static,
    ) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let request = read_request(&mut stream);
                let (status, extra_headers, body) = handler(&request);
                let head = format!(
                    "HTTP/1.1 {status} reason\r\n{extra_headers}Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        });
        addr
    }

    fn json_field(response: &str, field: &str) -> Value {
        let value: Value = serde_json::from_str(response).expect("response json");
        value
            .get(field)
            .unwrap_or_else(|| panic!("missing field {field} in {value}"))
            .clone()
    }

    #[test]
    fn http_get_returns_full_response_json() {
        test_bridge();
        let addr = spawn_server(|_req| (200, String::new(), "hello body".to_string()));
        let resp = call(
            "http_get",
            &format!(r#"["", null, "http://{addr}/page", {{"User-Agent": "test-agent"}}]"#),
        )
        .unwrap();
        assert_eq!(json_field(&resp, "statusCode"), json!(200));
        assert_eq!(json_field(&resp, "body"), json!("hello body"));
        assert_eq!(json_field(&resp, "isRedirect"), json!(false));
        assert!(json_field(&resp, "headers")["content-type"]
            .as_str()
            .unwrap()
            .contains("text/plain"));
        assert_eq!(json_field(&resp, "request")["method"], json!("GET"));
        assert_eq!(
            json_field(&resp, "request")["url"],
            json!(format!("http://{addr}/page"))
        );
        assert_eq!(
            json_field(&resp, "request")["headers"]["User-Agent"],
            json!("test-agent")
        );
    }

    #[test]
    fn http_head_returns_empty_body() {
        test_bridge();
        let addr = spawn_server(|_req| (204, String::new(), "unused".to_string()));
        let resp = call("http_head", &format!(r#"["", null, "http://{addr}/h"]"#)).unwrap();
        assert_eq!(json_field(&resp, "statusCode"), json!(204));
        assert_eq!(json_field(&resp, "body"), json!(""));
    }

    #[test]
    fn http_post_json_body_is_json_encoded() {
        test_bridge();
        let addr = spawn_server(|req| {
            let body = req.split("\r\n\r\n").nth(1).unwrap_or("").trim();
            (200, String::new(), format!("received:{body}"))
        });
        let resp = call(
            "http_post",
            &format!(
                r#"["", null, "http://{addr}/p", {{"Content-Type": "application/json"}}, ["a","b"]]"#
            ),
        )
        .unwrap();
        assert_eq!(json_field(&resp, "body"), json!("received:[\"a\",\"b\"]"));
    }

    #[test]
    fn http_string_body_is_sent_verbatim() {
        test_bridge();
        let addr = spawn_server(|req| {
            let body = req.split("\r\n\r\n").nth(1).unwrap_or("").trim();
            (200, String::new(), format!("received:{}", body))
        });
        let resp = call(
            "http_post",
            &format!(r#"["", null, "http://{addr}/p", {{}}, "abc"]"#),
        )
        .unwrap();
        assert_eq!(json_field(&resp, "body"), json!("received:abc"));
    }

    #[test]
    fn http_put_and_delete_map_to_post_transport() {
        test_bridge();
        let addr = spawn_server(|_req| (200, String::new(), String::new()));
        let resp = call("http_put", &format!(r#"["", null, "http://{addr}/u"]"#)).unwrap();
        assert_eq!(json_field(&resp, "request")["method"], json!("PUT"));
        let resp = call("http_delete", &format!(r#"["", null, "http://{addr}/d"]"#)).unwrap();
        assert_eq!(json_field(&resp, "request")["method"], json!("DELETE"));
        let resp = call("http_patch", &format!(r#"["", null, "http://{addr}/pa"]"#)).unwrap();
        assert_eq!(json_field(&resp, "request")["method"], json!("PATCH"));
    }

    #[test]
    fn http_missing_url_is_an_error() {
        test_bridge();
        assert!(call("http_get", r#"["", null, ""]"#)
            .unwrap_err()
            .to_string()
            .contains("missing request url"));
    }

    #[test]
    fn http_follows_redirects_to_final_url() {
        test_bridge();
        let addr = spawn_server(|req| {
            let first_line = req.lines().next().unwrap_or("").to_string();
            if first_line.contains("/start") {
                (302, "Location: /final\r\n".to_string(), String::new())
            } else {
                (200, String::new(), "done".to_string())
            }
        });
        let resp = call("http_get", &format!(r#"["", null, "http://{addr}/start"]"#)).unwrap();
        assert_eq!(json_field(&resp, "body"), json!("done"));
        assert_eq!(json_field(&resp, "statusCode"), json!(200));
        assert_eq!(
            json_field(&resp, "request")["url"],
            json!(format!("http://{addr}/final"))
        );
    }
}
