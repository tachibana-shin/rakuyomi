//! Native cheerio-compatible DOM/selector primitives exposed to the LNReader
//! plugin's JS runtime, backed by `dom_query` (the same crate/version already
//! used by `wasm_imports/html.rs` for WASM sources).
//!
//! Ported from the validated standalone PoC (`docs/lnreader/poc-reference-main.rs`)
//! that proved this approach against real `boa_engine 0.21`/`dom_query 0.28`.
//! The JS never sees a DOM object directly, only integer handles into the
//! [`Store`] below, which it passes back into these native functions. A JS
//! prelude ([`super::js_runtime::CHEERIO_PRELUDE`]) rebuilds cheerio's
//! chainable `$('sel').find().text()` API on top of these handles.

use std::collections::HashMap;

use boa_engine::{
    js_string,
    native_function::NativeFunction,
    object::{builtins::JsArray, FunctionObjectBuilder},
    Context, JsArgs, JsError, JsNativeError, JsResult, JsValue,
};
use boa_gc::{Finalize, Gc, GcRefCell, Trace};
use dom_query::{Document, Matcher, NodeRef, Selection};

pub(super) type SharedStore = Gc<GcRefCell<Store>>;

/// Table of parsed documents and live selections, indexed by integer handle.
/// The JS side never manipulates DOM objects directly, only these handles
/// (cast to `usize`).
///
/// Documents/matchers are owned (`Box<Document>`/`Box<Matcher>`), not
/// leaked: an earlier version of this store used `Box::leak` to get the
/// `'static` references `Selection<'static>` needs, on the assumption that
/// everything would be freed together "when the source is unloaded" — in
/// practice a source is never unloaded for the process's lifetime, so nothing
/// was ever freed. Tested against a real source with a large paginated
/// listing (~1.8 MB library page, thousands of entries): the accumulated
/// leak from that single call was enough to crash the process on the very
/// next operation (a native stack overflow deep inside `boa_engine`'s own
/// Promise job processing, `SIGSEGV`, confirmed via debugger backtrace —
/// not something catchable as an ordinary JS exception). [`Store::clear`]
/// is what actually fixes this: called after every top-level `Source`
/// operation (`js_runtime::JsRuntime::call_plugin_method`), since nothing
/// loaded during one operation is needed by the next.
///
/// `#[unsafe_ignore_trace]`: none of these fields hold a `JsValue`, so there
/// is nothing for boa's GC to trace here even though this type is captured
/// by a `NativeFunction` (which requires `Trace`/`Finalize`).
#[derive(Trace, Finalize, Default)]
pub(super) struct Store {
    // clippy's `vec_box` suggestion (`Vec<Document>` instead of
    // `Vec<Box<Document>>`) would be a real soundness bug here, not just a
    // style nit: `doc()`'s `&'static` cast is only sound because growing
    // this `Vec` moves `Box` pointers, never the heap allocations they
    // point to (see that method's SAFETY comment) -- a bare `Vec<Document>`
    // relocates the `Document` values themselves on reallocation, which
    // would dangle any `&'static Document` already handed out.
    #[allow(clippy::vec_box)]
    #[unsafe_ignore_trace]
    docs: Vec<Box<Document>>,
    #[unsafe_ignore_trace]
    sels: Vec<Selection<'static>>,
    #[unsafe_ignore_trace]
    matcher_cache: HashMap<String, Box<Matcher>>,
}

impl Store {
    fn push_doc(&mut self, doc: Document) -> usize {
        self.docs.push(Box::new(doc));
        self.docs.len() - 1
    }

    fn push_sel(&mut self, sel: Selection<'static>) -> usize {
        self.sels.push(sel);
        self.sels.len() - 1
    }

    /// Safe handle resolution: an out-of-bounds handle is a real bug (bad
    /// caller), so it's a catchable JS exception via [`js_error`], not a
    /// silent default.
    fn sel(&self, id: usize) -> Result<&Selection<'static>, String> {
        self.sels
            .get(id)
            .ok_or_else(|| format!("invalid selection handle: {id}"))
    }

    fn sel_mut(&mut self, id: usize) -> Result<&mut Selection<'static>, String> {
        self.sels
            .get_mut(id)
            .ok_or_else(|| format!("invalid selection handle: {id}"))
    }

    fn doc(&self, id: usize) -> Result<&'static Document, String> {
        let boxed = self
            .docs
            .get(id)
            .ok_or_else(|| format!("invalid document handle: {id}"))?;
        // SAFETY: sound because `Store::clear()` drops every `Selection`
        // derived from this `Document` in the same call as the `Document`
        // itself (never independently — see `clear()`), and no reference
        // into `Store` ever escapes it (JS only ever holds opaque integer
        // handles, never a raw pointer). Moving/growing `self.docs` (a
        // `Vec<Box<Document>>`) only moves the `Box` pointer, never the
        // heap allocation it points to, so this reference stays valid
        // across further `push_doc` calls too.
        Ok(unsafe { &*(boxed.as_ref() as *const Document) })
    }

    /// Compiles a CSS selector once (`Matcher::new`, which also validates
    /// the syntax) and caches it — repeated calls to the same selector (very
    /// common: a source looping over `.novel-item` many times) no longer
    /// re-parse anything. The cache itself is cleared by [`Store::clear`]
    /// along with everything else, so this only helps within one top-level
    /// operation, not across operations — an acceptable tradeoff for
    /// actually freeing memory (see the [`Store`] doc comment).
    fn compile_matcher(&mut self, selector: &str) -> Result<&'static Matcher, String> {
        if let Some(m) = self.matcher_cache.get(selector) {
            // SAFETY: see `doc()`.
            return Ok(unsafe { &*(m.as_ref() as *const Matcher) });
        }
        let matcher = Matcher::new(selector)
            .map_err(|e| format!("invalid CSS selector [{selector}]: {e:?}"))?;
        let boxed = Box::new(matcher);
        // SAFETY: see `doc()`.
        let static_ref: &'static Matcher = unsafe { &*(boxed.as_ref() as *const Matcher) };
        self.matcher_cache.insert(selector.to_string(), boxed);
        Ok(static_ref)
    }

    /// Frees every document/selection/cached matcher held by this store.
    /// Called after each top-level `Source` operation completes (see
    /// `js_runtime::JsRuntime::call_plugin_method`) — see the [`Store`] doc
    /// comment for why this replaced the original `Box::leak`-based design.
    /// `sels` is cleared before `docs`/`matcher_cache` on principle (drop
    /// the borrower before the borrowed), though it isn't load-bearing for
    /// soundness here: nothing is meant to survive across a `clear()` call
    /// in the first place (see the safety comments on
    /// `doc()`/`compile_matcher()`).
    pub(super) fn clear(&mut self) {
        self.sels.clear();
        self.docs.clear();
        self.matcher_cache.clear();
    }
}

/// Converts a Rust error message into a real catchable JS exception, rather
/// than letting the underlying panic (e.g. `dom_query`'s internal
/// `Selection::filter` on an invalid selector) take down the whole engine.
fn js_error(message: impl Into<String>) -> JsError {
    JsNativeError::typ().with_message(message.into()).into()
}

/// Normalizes `:contains(text)`/`:icontains(text)` into `:contains("text")`
/// before handing it to the selector engine.
///
/// Two independent things happen here for `:contains`: the selector engine
/// requires quotes, but unquoted `:contains(text)` is common in
/// cheerio-based code (LNReader sources included) -- so unquoted text gets
/// wrapped. `:icontains(text)` (real cheerio/`css-select`'s
/// case-INsensitive `:contains` variant) is additionally rewritten down to
/// plain `:contains(...)`, the only one of the two `dom_query`'s matcher
/// actually implements (`matcher.rs` matches the pseudo-class name only
/// against the literal string "contains", not "icontains" -- two genuinely
/// different pseudo-class names, not a case-of-the-same-name difference).
/// Found via `FWK.US`'s `parseNovel`
/// (`#lcp_instance_0 +:icontains('complete')`), which previously threw
/// `TypeError: invalid CSS selector ... UnsupportedPseudoClassOrElement`.
/// This trades away case-insensitivity (a `:contains("Complete")` selector
/// will no longer match literal "COMPLETE" text) in exchange for not
/// crashing at all -- the pragmatic choice given only this one corpus
/// source (1/261) uses `:icontains`, and true case-insensitive text
/// matching isn't a knob `dom_query`'s vendored matcher exposes.
fn normalize_contains(selector: &str) -> String {
    let selector = selector.replace(":icontains(", ":contains(");
    let selector = selector.as_str();
    const NEEDLE: &str = ":contains(";
    let mut out = String::with_capacity(selector.len() + 8);
    let bytes = selector.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if selector[i..].starts_with(NEEDLE) {
            out.push_str(NEEDLE);
            i += NEEDLE.len();
            let mut depth = 1u32;
            let start = i;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            let inner = selector[start..i].trim();
            if inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2 {
                out.push_str(inner);
            } else {
                out.push('"');
                out.push_str(inner);
                out.push('"');
            }
            if i < bytes.len() {
                out.push(')'); // consume the closing parenthesis
                i += 1;
            }
            continue;
        }
        out.push(selector[i..].chars().next().unwrap());
        i += selector[i..].chars().next().unwrap().len_utf8();
    }
    out
}

#[cfg(test)]
mod contains_tests {
    use super::normalize_contains;

    #[test]
    fn leaves_selector_without_contains_untouched() {
        let sel = "div.content > a[href]";
        assert_eq!(normalize_contains(sel), sel);
    }

    #[test]
    fn quotes_unquoted_contains() {
        assert_eq!(
            normalize_contains(":contains(hello)"),
            ":contains(\"hello\")"
        );
    }

    #[test]
    fn leaves_already_quoted_contains_untouched() {
        let sel = ":contains(\"hello world\")";
        assert_eq!(normalize_contains(sel), sel);
    }

    #[test]
    fn handles_multiple_contains_in_one_selector() {
        assert_eq!(
            normalize_contains("div:contains(a) span:contains(b)"),
            "div:contains(\"a\") span:contains(\"b\")"
        );
    }

    #[test]
    fn trims_inner_whitespace() {
        assert_eq!(
            normalize_contains(":contains(   hello   )"),
            ":contains(\"hello\")"
        );
    }
}

fn arg_string(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<String> {
    Ok(args
        .get_or_undefined(index)
        .to_string(context)?
        .to_std_string_escaped())
}

fn arg_usize(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<usize> {
    Ok(args.get_or_undefined(index).to_number(context)? as usize)
}

/// `__native_load(html) -> doc_id`. Parses HTML once (html5ever, via
/// `dom_query::Document::from`) and stores the document. Equivalent to
/// `cheerio.load(html)`, but only returns the document id — not a selection
/// yet.
fn native_load(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let html = arg_string(args, 0, context)?;
    let doc = Document::from(html.as_str());
    let id = store.borrow_mut().push_doc(doc);
    Ok(JsValue::from(id as f64))
}

/// `__native_select_root(doc_id, selector) -> sel_id`. The `$('selector')`
/// entry point: searches from the document root.
fn native_select_root(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let doc_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();

    let doc = s.doc(doc_id).map_err(js_error)?;
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;
    let sel = doc.select_matcher(matcher);
    let id = s.push_sel(sel);
    Ok(JsValue::from(id as f64))
}

/// `__native_find(sel_id, selector) -> sel_id`. Equivalent to cheerio's
/// `.find()`: searches among the DESCENDANTS of the current selection, not
/// from the document root.
fn native_find(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();

    let matcher = s.compile_matcher(&selector).map_err(js_error)?;
    let sub = s.sel(sel_id).map_err(js_error)?.select_matcher(matcher);
    let id = s.push_sel(sub);
    Ok(JsValue::from(id as f64))
}

/// `__native_text(sel_id) -> string`
fn native_text(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    let text = s.sel(sel_id).map_err(js_error)?.text();
    Ok(JsValue::from(js_string!(text.as_ref())))
}

/// `__native_outer_html(sel_id) -> string`. `dom_query`'s `.html()` renders
/// the OUTER html (including the element's own tag) — useful for a real
/// outer-HTML need (cheerio's `.prop('outerHTML')`), but not for cheerio's
/// standard `.html()` (see [`native_inner_html`]).
fn native_outer_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    let html = s.sel(sel_id).map_err(js_error)?.html();
    Ok(JsValue::from(js_string!(html.as_ref())))
}

/// `__native_inner_html(sel_id) -> string`. cheerio's `.html()` renders the
/// INNER html (element content, without its own tag) — `dom_query 0.28` has
/// a real `Selection::inner_html()` for exactly this.
fn native_inner_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    let html = s.sel(sel_id).map_err(js_error)?.inner_html();
    Ok(JsValue::from(js_string!(html.as_ref())))
}

/// `__native_attr(sel_id, name) -> string | null`. An invalid handle stays a
/// real error (`Result` via `sel()`), but a missing attribute on a valid
/// element is a plain `null` on the JS side (legitimate `Option`, matching
/// real cheerio's behavior).
fn native_attr(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let name = arg_string(args, 1, context)?;
    let s = store.borrow();
    match s.sel(sel_id).map_err(js_error)?.attr(&name) {
        Some(v) => Ok(JsValue::from(js_string!(v.as_ref()))),
        None => Ok(JsValue::null()),
    }
}

/// `__native_attribs(sel_id) -> JSON string of {name: value}`. Backs the
/// `.attribs` property real cheerio exposes directly on a raw DOM node
/// (distinct from `.attr(name)`, one attribute at a time) -- found needed
/// via `.attribs.class`/`.attribs.href`/`.attribs.title` (89/261 real corpus
/// sources, by far the widest-reaching gap found this pass) because real
/// cheerio's `.each()`/`.map()`/`.filter()` callbacks hand back the raw
/// element as their second argument, and plugin code very commonly reads
/// straight off it instead of re-wrapping with `$(el)` first. Returned as a
/// JSON string and `JSON.parse`'d on the JS side, the same boundary
/// convention `fetch()`'s headers already use in this module, rather than
/// building a `JsObject` field-by-field from Rust.
fn native_attribs(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    let attrs = s.sel(sel_id).map_err(js_error)?.attrs();
    let map: HashMap<String, String> = attrs
        .into_iter()
        .map(|a| (a.name.local.to_string(), a.value.to_string()))
        .collect();
    let json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string());
    Ok(JsValue::from(js_string!(json.as_str())))
}

/// `__native_set_attr(sel_id, name, value) -> undefined`
fn native_set_attr(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let name = arg_string(args, 1, context)?;
    let value = arg_string(args, 2, context)?;
    let mut s = store.borrow_mut();
    s.sel_mut(sel_id).map_err(js_error)?.set_attr(&name, &value);
    Ok(JsValue::undefined())
}

/// `__native_node_type(sel_id) -> "text" | "tag" | "comment" | "other"`.
/// Needed for `.contents()`: cheerio distinguishes text nodes from tags.
fn native_node_type(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    let ty = match s.sel(sel_id).map_err(js_error)?.nodes().first() {
        Some(node) => {
            if node.query(|n| n.is_text()).unwrap_or(false) {
                "text"
            } else if node.query(|n| n.is_comment()).unwrap_or(false) {
                "comment"
            } else if node.query(|n| n.is_element()).unwrap_or(false) {
                "tag"
            } else {
                "other"
            }
        }
        None => "other",
    };
    Ok(JsValue::from(js_string!(ty)))
}

/// `__native_tag_name(sel_id) -> uppercase tag name, "" if not an element`.
/// Backs `.prop("tagName")` -- real cheerio/browser DOM's `.prop("tagName")`
/// returns the UPPERCASE tag name, distinct from `.attr()` (which has no
/// concept of "tagName", an intrinsic property rather than a markup
/// attribute). Found needed via `skythewood.js`'s `i.prop("tagName")` on an
/// element from a `.find("*").each()` callback, used to pick out `<img>`
/// nodes by name -- `.prop` didn't exist on `CheerioSelection` at all before
/// this ("not a callable function" calling the missing method).
fn native_tag_name(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    let name = match s.sel(sel_id).map_err(js_error)?.nodes().first() {
        Some(node) => node
            .query(|n| n.as_element().map(|e| e.node_name().to_string()))
            .flatten()
            .unwrap_or_default(),
        None => String::new(),
    };
    Ok(JsValue::from(js_string!(name.to_uppercase().as_str())))
}

/// `__native_contents(sel_id) -> JsArray` of handles (one 1-element
/// selection per handle).
///
/// `Selection`'s high-level navigation (`children`/`next_sibling`) only
/// exposes ELEMENTS. But at the lower `NodeRef` level,
/// `first_child()`/`next_sibling()` don't filter anything — they include
/// text and comment nodes too. Walking at that level lets us enumerate every
/// child of the first matched node, then rebuild a 1-element `Selection` per
/// child via `Selection::from(node)`.
fn native_contents(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let mut s = store.borrow_mut();
    let first_child = s
        .sel(sel_id)
        .map_err(js_error)?
        .nodes()
        .first()
        .and_then(|node| node.first_child());

    let mut handles: Vec<JsValue> = Vec::new();
    let mut current = first_child;
    while let Some(node) = current {
        let next = node.next_sibling();
        let one = Selection::from(node);
        let id = s.push_sel(one);
        handles.push(JsValue::from(id as f64));
        current = next;
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// `__native_first(sel_id) -> sel_id`
fn native_first(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let mut s = store.borrow_mut();
    let first = s.sel(sel_id).map_err(js_error)?.first();
    let id = s.push_sel(first);
    Ok(JsValue::from(id as f64))
}

/// `__native_each_count(sel_id) -> number of matched elements`
fn native_each_count(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    Ok(JsValue::from(
        s.sel(sel_id).map_err(js_error)?.length() as f64
    ))
}

/// `__native_each_at(sel_id, index) -> sel_id` (1-element selection)
fn native_each_at(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let index = arg_usize(args, 1, context)?;
    let mut s = store.borrow_mut();
    let one = match s.sel(sel_id).map_err(js_error)?.get(index).cloned() {
        Some(node) => Selection::from(node),
        None => Selection::default(),
    };
    let id = s.push_sel(one);
    Ok(JsValue::from(id as f64))
}

/// `__native_all_handles(sel_id) -> JsArray` of handles (one per matched
/// element). The whole loop runs on the Rust side and returns a single
/// `JsArray` — one JS<->Rust boundary crossing for the whole list, no matter
/// its size. Backs `.each()`/`.map()`/`.toArray()`/`.slice()` on the JS side.
fn native_all_handles(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let mut s = store.borrow_mut();
    let n = s.sel(sel_id).map_err(js_error)?.length();
    let mut handles: Vec<JsValue> = Vec::with_capacity(n);
    for i in 0..n {
        let one = match s.sel(sel_id).map_err(js_error)?.get(i).cloned() {
            Some(node) => Selection::from(node),
            None => Selection::default(),
        };
        let id = s.push_sel(one);
        handles.push(JsValue::from(id as f64));
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// Small macro to avoid repeating the same "sel_id -> new derived selection"
/// pattern 5 times (parent/children/next_sibling/prev_sibling/last).
macro_rules! nav_fn {
    ($fn_name:ident, $method:ident) => {
        fn $fn_name(
            store: &SharedStore,
            args: &[JsValue],
            context: &mut Context,
        ) -> JsResult<JsValue> {
            let sel_id = arg_usize(args, 0, context)?;
            let mut s = store.borrow_mut();
            let derived = s.sel(sel_id).map_err(js_error)?.$method();
            let id = s.push_sel(derived);
            Ok(JsValue::from(id as f64))
        }
    };
}

// `__native_parent` / `__native_children` / `__native_next_sibling` /
// `__native_prev_sibling` / `__native_last` (sel_id) -> sel_id
nav_fn!(native_parent, parent);
nav_fn!(native_children, children);
nav_fn!(native_next_sibling, next_sibling);
nav_fn!(native_prev_sibling, prev_sibling);
nav_fn!(native_last, last);

/// `__native_remove(sel_id) -> undefined` (in-place mutation, detaches the
/// nodes)
fn native_remove(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let mut s = store.borrow_mut();
    s.sel_mut(sel_id).map_err(js_error)?.remove();
    Ok(JsValue::undefined())
}

/// Same idea as `nav_fn!` above, for the "single string-arg mutation,
/// returns undefined" pattern (add_class/remove_class/remove_attr all have
/// the exact same body, only the `dom_query` method name changes).
macro_rules! mut_str_fn {
    ($fn_name:ident, $method:ident) => {
        fn $fn_name(
            store: &SharedStore,
            args: &[JsValue],
            context: &mut Context,
        ) -> JsResult<JsValue> {
            let sel_id = arg_usize(args, 0, context)?;
            let value = arg_string(args, 1, context)?;
            let mut s = store.borrow_mut();
            s.sel_mut(sel_id).map_err(js_error)?.$method(&value);
            Ok(JsValue::undefined())
        }
    };
}
mut_str_fn!(native_add_class, add_class);
mut_str_fn!(native_remove_class, remove_class);
mut_str_fn!(native_remove_attr, remove_attr);

/// `__native_has_class(sel_id, class) -> bool`
fn native_has_class(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let class = arg_string(args, 1, context)?;
    let s = store.borrow();
    Ok(JsValue::from(
        s.sel(sel_id).map_err(js_error)?.has_class(&class),
    ))
}

/// `__native_exists(sel_id) -> bool`
fn native_exists(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let s = store.borrow();
    Ok(JsValue::from(s.sel(sel_id).map_err(js_error)?.exists()))
}

/// `__native_is(sel_id, selector) -> bool`
fn native_is(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let selector = arg_string(args, 1, context)?;
    let s = store.borrow();
    Ok(JsValue::from(
        s.sel(sel_id).map_err(js_error)?.is(&selector),
    ))
}

/// `__native_append_html(sel_id, html) -> undefined`
fn native_append_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let html = arg_string(args, 1, context)?;
    let mut s = store.borrow_mut();
    s.sel_mut(sel_id).map_err(js_error)?.append_html(html);
    Ok(JsValue::undefined())
}

/// `__native_set_html(sel_id, html) -> undefined`
fn native_set_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let html = arg_string(args, 1, context)?;
    let mut s = store.borrow_mut();
    s.sel_mut(sel_id).map_err(js_error)?.set_html(html);
    Ok(JsValue::undefined())
}

/// `__native_wrap_html(sel_id, html) -> undefined`. Direct equivalent of
/// cheerio's `.wrap(html)`: `dom_query 0.28` has exactly that,
/// `NodeRef::wrap_html<T>(&self, html: T)`.
fn native_wrap_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let html = arg_string(args, 1, context)?;
    let s = store.borrow();
    if let Some(node) = s.sel(sel_id).map_err(js_error)?.nodes().first() {
        node.wrap_html(html);
    }
    Ok(JsValue::undefined())
}

/// `__native_clone(sel_id) -> sel_id` (new selection, INDEPENDENT nodes).
/// Equivalent of cheerio's `.clone()`: `dom_query 0.28` has
/// `NodeRef::to_fragment()`, which serializes the node and its children into
/// a brand-new `Document` (not a mere reference clone like `Selection`'s
/// derived `Clone`, which still points at the SAME shared nodes). The new
/// document is stored in the registry (like `native_load`), then its root is
/// selected.
///
/// `to_fragment()` builds `<html><copied node directly>` — NOT
/// `<html><body><copied node></body></html>` like `cheerio.load()` does
/// (which goes through a real full-document `html5ever` parse). A `<body>`
/// is created by `to_fragment()` but never attached to the tree (an orphan
/// node). Selecting `'*'` would therefore match `<html>` itself first — the
/// direct children of `<html>` need to be targeted instead of `<body>`'s.
fn native_clone(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let mut s = store.borrow_mut();
    let fragment_doc = match s.sel(sel_id).map_err(js_error)?.nodes().first() {
        Some(node) => node.to_fragment(),
        None => {
            let id = s.push_sel(Selection::default());
            return Ok(JsValue::from(id as f64));
        }
    };
    let doc_id = s.push_doc(fragment_doc);
    let doc = s.doc(doc_id).map_err(js_error)?;
    // 'html > *', not just '*': `to_fragment()` places the copied node as a
    // DIRECT child of `<html>`, not under a `<body>` (see note above).
    let matcher = s.compile_matcher("html > *").map_err(js_error)?;
    let sel = doc.select_matcher(matcher).first();
    let id = s.push_sel(sel);
    Ok(JsValue::from(id as f64))
}

/// `__native_filter(sel_id, selector) -> sel_id` (a real multi-element
/// `Selection`, not a JS array). Equivalent of cheerio's `.filter(selector)`:
/// `dom_query 0.28` has `Selection::filter(&self, sel: &str) -> Self`.
///
/// `dom_query` has no native `.not()` (only `filter`/`add` exist) — `.not()`
/// and the function form of `.filter()` stay composed on the JS side.
///
/// `Selection::filter()` internally does
/// `Matcher::new(sel).expect("Invalid CSS selector")`, which PANICS (takes
/// down the whole JS engine, not a catchable exception) on an invalid
/// selector, unlike `find()`/`select_root()` which go through
/// `compile_matcher()`/`js_error()`. To avoid that, this function validates
/// the selector FIRST via our own `compile_matcher` (already used/tested
/// elsewhere), and only calls `.filter()` once parsing is confirmed to
/// succeed — the internal `.expect()` can then never panic, since we've
/// already proven the same selector parses cleanly.
fn native_filter(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let selector = arg_string(args, 1, context)?;
    let mut s = store.borrow_mut();
    s.compile_matcher(&selector).map_err(js_error)?;
    let filtered = s.sel(sel_id).map_err(js_error)?.filter(&selector);
    let id = s.push_sel(filtered);
    Ok(JsValue::from(id as f64))
}

/// `__native_not(sel_id, selector) -> JsArray` of handles for elements of
/// the selection that do NOT match `selector`. Inverse of [`native_filter`],
/// same "one native call for the whole selection" shape -- found during a
/// perf pass over this file's `__native_*` surface that the JS `.not()`
/// wrapper was instead calling `__native_is` once per element (an N-calls
/// pattern this file otherwise avoids everywhere else: `native_has`/
/// `native_siblings`/`native_next_until`/`native_filter` all already do
/// their per-element test in this same single-call, Rust-side-loop shape).
fn native_not(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;

    let n = s.sel(sel_id).map_err(js_error)?.length();
    let mut handles: Vec<JsValue> = Vec::new();
    for i in 0..n {
        let node = s.sel(sel_id).map_err(js_error)?.get(i).cloned();
        if let Some(node) = node {
            let one = Selection::from(node);
            if !one.is_matcher(matcher) {
                let id = s.push_sel(one);
                handles.push(JsValue::from(id as f64));
            }
        }
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// `__native_before_html(sel_id, html) -> undefined`. Equivalent of
/// cheerio's `.before(html)`. Unlike a composed
/// `outerHtml()`+`replaceWith()` implementation (which destroys the live
/// node reference, breaking `.before(x).after(y)` chained on the same
/// instance), `before_html()` doesn't replace the node, just inserts a
/// sibling — the reference stays valid.
fn native_before_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let html = arg_string(args, 1, context)?;
    let s = store.borrow();
    s.sel(sel_id).map_err(js_error)?.before_html(html);
    Ok(JsValue::undefined())
}

/// `__native_after_html(sel_id, html) -> undefined`. Same as
/// [`native_before_html`], for `.after()`.
fn native_after_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let html = arg_string(args, 1, context)?;
    let s = store.borrow();
    s.sel(sel_id).map_err(js_error)?.after_html(html);
    Ok(JsValue::undefined())
}

/// `__native_set_text(sel_id, text) -> undefined`. Equivalent of cheerio's
/// `.text(value)`: `dom_query 0.28` has `Selection::set_text(&self, text:
/// &str)`.
fn native_set_text(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let text = arg_string(args, 1, context)?;
    let s = store.borrow();
    s.sel(sel_id).map_err(js_error)?.set_text(&text);
    Ok(JsValue::undefined())
}

/// `__native_has(sel_id, selector) -> JsArray` of handles. For each element
/// of the starting selection, tests whether it has a descendant matching the
/// selector — the whole loop runs on the Rust side, a single boundary
/// crossing regardless of the selection's size.
fn native_has(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;

    let n = s.sel(sel_id).map_err(js_error)?.length();
    let mut handles: Vec<JsValue> = Vec::new();
    for i in 0..n {
        let node = s.sel(sel_id).map_err(js_error)?.get(i).cloned();
        if let Some(node) = node {
            let one = Selection::from(node);
            let descendants = one.select_matcher(matcher);
            if descendants.exists() {
                let id = s.push_sel(one);
                handles.push(JsValue::from(id as f64));
            }
        }
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// `__native_siblings(sel_id, selector_or_null) -> JsArray` of handles.
/// Identity comparison (`NodeId` derives `PartialEq`) happens directly in
/// Rust, one traversal for the whole set instead of a per-candidate boundary
/// crossing. The optional selector is tested in the SAME Rust-side loop
/// (`is_matcher`, no separate `Matcher::new` per candidate — the matcher is
/// compiled once via `compile_matcher`) rather than composed on top in JS —
/// same one-native-call shape as `native_has`/`native_not`. Previously the
/// selector form built a `CheerioSelection` (and paid its constructor's
/// `native_each_count`) for every raw sibling before filtering any of them
/// out; not currently exercised by any real corpus source (§1.2.4/§1.2.3),
/// but cheap to fold in using a technique already proven elsewhere in this
/// file, so there was no reason to leave the old N-wasted-constructions
/// shape in place once this file was already being re-reviewed.
fn native_siblings(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let selector_arg = args.get_or_undefined(1);
    let mut s = store.borrow_mut();

    let matcher = if selector_arg.is_undefined() || selector_arg.is_null() {
        None
    } else {
        let raw_selector = selector_arg.to_string(context)?.to_std_string_escaped();
        let selector = normalize_contains(&raw_selector);
        Some(s.compile_matcher(&selector).map_err(js_error)?)
    };

    let self_id = s
        .sel(sel_id)
        .map_err(js_error)?
        .nodes()
        .first()
        .map(|n| n.id);
    let children = s.sel(sel_id).map_err(js_error)?.parent().children();

    let mut handles: Vec<JsValue> = Vec::new();
    for i in 0..children.length() {
        if let Some(node) = children.get(i).cloned() {
            if Some(node.id) != self_id {
                let one = Selection::from(node);
                if matcher.is_some_and(|m| !one.is_matcher(m)) {
                    continue;
                }
                let id = s.push_sel(one);
                handles.push(JsValue::from(id as f64));
            }
        }
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// `__native_children_filtered(sel_id, selector) -> JsArray` of handles.
/// `dom_query`'s `children()` takes no selector, so the previous
/// implementation composed `.children()` + `.toArray()` + a JS-side
/// `.is(selector)` per child — 3+2N native calls (N = ALL children,
/// including non-matches) for what real cheerio's `.children(selector)`
/// needs. This does the same per-child test as `native_has`/`native_not`/
/// `native_siblings` above: one native call, one Rust-side loop, only
/// matching children ever get a `CheerioSelection` handle at all — 1+M (M =
/// matched count, M <= N).
fn native_children_filtered(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;

    let children = s.sel(sel_id).map_err(js_error)?.children();
    let mut handles: Vec<JsValue> = Vec::new();
    for i in 0..children.length() {
        if let Some(node) = children.get(i).cloned() {
            let one = Selection::from(node);
            if one.is_matcher(matcher) {
                let id = s.push_sel(one);
                handles.push(JsValue::from(id as f64));
            }
        }
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// `__native_remove_filtered(sel_id, selector) -> undefined`. Real cheerio's
/// `.remove(selector)` filters the current set by selector, THEN removes
/// only the matches. The previous implementation composed this on the JS
/// side as `.filter(selector).each(el => __native_remove(el.__id))` — 3+2M
/// native calls (M = matched-and-removed count) for what `dom_query`'s own
/// `Selection::filter()` + `Selection::remove()` already do as two whole-
/// selection Rust calls, callable back-to-back inside a SINGLE native
/// function without ever handing intermediate handles back to JS. 1 native
/// call total, regardless of M.
fn native_remove_filtered(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let selector = arg_string(args, 1, context)?;
    let mut s = store.borrow_mut();
    s.compile_matcher(&selector).map_err(js_error)?;
    let filtered = s.sel(sel_id).map_err(js_error)?.filter(&selector);
    filtered.remove();
    Ok(JsValue::undefined())
}

/// Shared body for `__native_next_sibling_filtered`/
/// `__native_prev_sibling_filtered`: real cheerio's `.next(selector)`/
/// `.prev(selector)` test ONLY the immediate sibling (never searching
/// further if it doesn't match — see the JS prelude's own comment on this),
/// so the whole thing — get the sibling, check it exists, test it against
/// the selector — collapses into one native call instead of the previous
/// `native_{next,prev}_sibling` + ctor + `.exists()` + `.is()` chain (up to
/// 4 calls for 1).
fn sibling_filtered(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
    direction: fn(&Selection<'static>) -> Selection<'static>,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;

    let sibling = direction(s.sel(sel_id).map_err(js_error)?);
    let result = if sibling.exists() && sibling.is_matcher(matcher) {
        sibling
    } else {
        Selection::default()
    };
    let id = s.push_sel(result);
    Ok(JsValue::from(id as f64))
}

/// `__native_next_sibling_filtered(sel_id, selector) -> sel_id`
fn native_next_sibling_filtered(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    sibling_filtered(store, args, context, Selection::next_sibling)
}

/// `__native_prev_sibling_filtered(sel_id, selector) -> sel_id`
fn native_prev_sibling_filtered(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    sibling_filtered(store, args, context, Selection::prev_sibling)
}

/// `__native_load_and_select_root(html, selector) -> [doc_id, sel_id]`.
/// `cheerio.load(html)` and `$(htmlString)` (the detached-fragment call
/// form) both immediately follow `__native_load` with one
/// `__native_select_root` call on the document they just created — a fixed,
/// always-back-to-back pair, unlike `$(selector)` on an ALREADY-loaded
/// document (which reuses a `doc_id` across arbitrarily many later calls,
/// and still needs the two native functions kept separate for that case).
/// Folding just the "load, then select" pair into one native call removes a
/// JS<->Rust round trip that only ever carried the freshly-minted `doc_id`
/// argument straight back into the very next call — 2 native calls instead
/// of `native_load` + `native_select_root`'s `3` before ctor overhead.
/// Returns both ids (not just the selection's) because `cheerio.load()`'s
/// returned `$(selectorOrElement)` closure still needs `doc_id` as an
/// upvalue for its OWN later, independent `__native_select_root` calls.
fn native_load_and_select_root(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let html = arg_string(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();

    let doc = Document::from(html.as_str());
    let doc_id = s.push_doc(doc);
    let doc_ref = s.doc(doc_id).map_err(js_error)?;
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;
    let sel = doc_ref.select_matcher(matcher);
    let sel_id = s.push_sel(sel);
    drop(s);

    let array = JsArray::from_iter(
        [JsValue::from(doc_id as f64), JsValue::from(sel_id as f64)],
        context,
    );
    Ok(JsValue::from(array))
}

/// `__native_select_and_outer_html(doc_id, selector) -> string`. Backs
/// `$.html()` with no argument — real cheerio serializes the whole document.
/// The previous implementation built a full `CheerioSelection` (paying its
/// constructor's `native_each_count` call) purely to immediately call
/// `.outerHtml()` on it and throw the wrapper away — this does the select +
/// serialize in one native call, 1 instead of 3 (`native_select_root` + ctor
/// + `native_outer_html`), since nothing here is ever chained further.
fn native_select_and_outer_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let doc_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();

    let doc = s.doc(doc_id).map_err(js_error)?;
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;
    let sel = doc.select_matcher(matcher);
    let html = sel.html();
    Ok(JsValue::from(js_string!(html.as_ref())))
}

/// `__native_closest(sel_id, selector) -> sel_id`. Walks up ancestors via
/// `NodeRef::parent()` (unfiltered, same primitive used for `.contents()`),
/// testing each one with `Selection::is_matcher()` — no re-parsing of the
/// selector, no boundary crossing per level.
fn native_closest(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;

    let start = s.sel(sel_id).map_err(js_error)?.nodes().first().cloned();
    let mut current = start;
    let mut found: Option<Selection<'static>> = None;
    for _ in 0..200 {
        // Guard against infinite loops, same limit as a composed JS version.
        let Some(node) = current else { break };
        let one = Selection::from(node);
        if one.is_matcher(matcher) {
            found = Some(one);
            break;
        }
        current = node.parent();
    }
    let id = s.push_sel(found.unwrap_or_default());
    Ok(JsValue::from(id as f64))
}

/// `__native_next_until(sel_id, selector) -> JsArray` of handles. Collects
/// following element siblings until one matches the selector or there are no
/// more. Uses `next_element_sibling()` (not the raw, unfiltered
/// `next_sibling()`) since cheerio's `.nextUntil()` operates at the element
/// level, ignoring raw text nodes between tags.
fn native_next_until(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let raw_selector = arg_string(args, 1, context)?;
    let selector = normalize_contains(&raw_selector);
    let mut s = store.borrow_mut();
    let matcher = s.compile_matcher(&selector).map_err(js_error)?;

    let start = s
        .sel(sel_id)
        .map_err(js_error)?
        .nodes()
        .first()
        .and_then(|n| n.next_element_sibling());
    let mut current = start;
    let mut handles: Vec<JsValue> = Vec::new();
    for _ in 0..500 {
        // Guard against infinite loops, same limit as a composed JS version.
        let Some(node) = current else { break };
        let one = Selection::from(node);
        if one.is_matcher(matcher) {
            break;
        }
        let next = node.next_element_sibling();
        let id = s.push_sel(one);
        handles.push(JsValue::from(id as f64));
        current = next;
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// Reads an optional selector argument (`undefined`/`null` means "no
/// filter"), already `:icontains`-normalized and compiled, shared by
/// [`native_parents`]/[`native_next_all`]/[`native_prev_all`] below — same
/// "filter in the same Rust-side loop that walks the candidates" shape
/// already used by `native_children_filtered`/`native_siblings`.
fn optional_matcher(
    s: &mut Store,
    args: &[JsValue],
    index: usize,
    context: &mut Context,
) -> JsResult<Option<&'static Matcher>> {
    let arg = args.get_or_undefined(index);
    if arg.is_undefined() || arg.is_null() {
        return Ok(None);
    }
    let raw_selector = arg.to_string(context)?.to_std_string_escaped();
    let selector = normalize_contains(&raw_selector);
    Ok(Some(s.compile_matcher(&selector).map_err(js_error)?))
}

/// `__native_add(sel_id, selector) -> sel_id`. Direct mapping onto
/// `Selection::add()`, which already searches from the document root the
/// same way real cheerio's `.add(selector)` does — see
/// `docs/lnreader/REFERENCE.md` §1.2.10 for the `dom_query`-source citation
/// and the one inherited edge case (adding from an empty selection).
fn native_add(store: &SharedStore, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let selector = arg_string(args, 1, context)?;
    let mut s = store.borrow_mut();
    s.compile_matcher(&selector).map_err(js_error)?;
    let added = s.sel(sel_id).map_err(js_error)?.add(&selector);
    let id = s.push_sel(added);
    Ok(JsValue::from(id as f64))
}

/// `__native_parents(sel_id, selector_or_null) -> JsArray` of handles,
/// farthest ancestor first (real cheerio's own documented order). Walks via
/// repeated `NodeRef::parent()` (same technique as `native_closest`) rather
/// than `dom_query`'s own `ancestors()`, stopping at the last real element
/// so the `Document`/`Fragment` root itself is never included — see
/// `docs/lnreader/REFERENCE.md` §1.2.10 for the regression this guards
/// against.
fn native_parents(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let mut s = store.borrow_mut();
    let matcher = optional_matcher(&mut s, args, 1, context)?;

    let start = s.sel(sel_id).map_err(js_error)?.nodes().first().cloned();
    let mut current = start.and_then(|n| n.parent());
    let mut ancestors: Vec<NodeRef<'static>> = Vec::new();
    for _ in 0..200 {
        // Same guard limit as native_closest.
        let Some(node) = current else { break };
        if !node.is_element() {
            break;
        }
        ancestors.push(node);
        current = node.parent();
    }
    ancestors.reverse();

    let mut handles: Vec<JsValue> = Vec::new();
    for node in ancestors {
        let one = Selection::from(node);
        if matcher.is_some_and(|m| !one.is_matcher(m)) {
            continue;
        }
        let id = s.push_sel(one);
        handles.push(JsValue::from(id as f64));
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// Shared body for `__native_next_all`/`__native_prev_all`: walks
/// element-level siblings in one direction, collecting every one (a
/// selector filters the walked set, unlike `.nextUntil()`'s early stop) —
/// see `docs/lnreader/REFERENCE.md` §1.2.10 for the real-cheerio-source
/// citation on ordering (nearest-first, not reversed).
fn sibling_walk_filtered(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
    direction: fn(&NodeRef<'static>) -> Option<NodeRef<'static>>,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let mut s = store.borrow_mut();
    let matcher = optional_matcher(&mut s, args, 1, context)?;

    let start = s
        .sel(sel_id)
        .map_err(js_error)?
        .nodes()
        .first()
        .and_then(direction);
    let mut current = start;
    let mut handles: Vec<JsValue> = Vec::new();
    for _ in 0..500 {
        // Same guard limit as native_next_until.
        let Some(node) = current else { break };
        let one = Selection::from(node);
        if matcher.is_none_or(|m| one.is_matcher(m)) {
            let id = s.push_sel(one);
            handles.push(JsValue::from(id as f64));
        }
        current = direction(&node);
    }
    drop(s);
    let array = JsArray::from_iter(handles, context);
    Ok(JsValue::from(array))
}

/// `__native_next_all(sel_id, selector_or_null) -> JsArray`
fn native_next_all(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    sibling_walk_filtered(store, args, context, NodeRef::next_element_sibling)
}

/// `__native_prev_all(sel_id, selector_or_null) -> JsArray`
fn native_prev_all(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    sibling_walk_filtered(store, args, context, NodeRef::prev_element_sibling)
}

/// `__native_replace_with_html(sel_id, html) -> undefined`
fn native_replace_with_html(
    store: &SharedStore,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let sel_id = arg_usize(args, 0, context)?;
    let html = arg_string(args, 1, context)?;
    let mut s = store.borrow_mut();
    s.sel_mut(sel_id).map_err(js_error)?.replace_with_html(html);
    Ok(JsValue::undefined())
}

/// Registers a Rust closure as a global JS function `name`, capturing a
/// clone of `store`.
fn register_native(
    context: &mut Context,
    name: &str,
    length: usize,
    store: SharedStore,
    f: impl Fn(&SharedStore, &[JsValue], &mut Context) -> JsResult<JsValue> + Copy + 'static,
) {
    let native = NativeFunction::from_copy_closure_with_captures(
        move |_this, args, store, context| f(store, args, context),
        store,
    );
    let func = FunctionObjectBuilder::new(context.realm(), native)
        .name(name)
        .length(length)
        .build();
    context
        .global_object()
        .set(js_string!(name), func, false, context)
        .expect("registering a native function should not fail");
}

/// Registers every `__native_*` primitive as a global function in `context`,
/// returning the backing [`Store`] handle. [`super::js_runtime`] evaluates
/// `CHEERIO_PRELUDE` on top of this to rebuild cheerio's chainable API.
pub(super) fn register(context: &mut Context) -> SharedStore {
    let store: SharedStore = Gc::new(GcRefCell::new(Store::default()));

    register_native(context, "__native_load", 1, store.clone(), native_load);
    register_native(
        context,
        "__native_select_root",
        2,
        store.clone(),
        native_select_root,
    );
    register_native(context, "__native_find", 2, store.clone(), native_find);
    register_native(context, "__native_text", 1, store.clone(), native_text);
    register_native(
        context,
        "__native_inner_html",
        1,
        store.clone(),
        native_inner_html,
    );
    register_native(
        context,
        "__native_outer_html",
        1,
        store.clone(),
        native_outer_html,
    );
    register_native(context, "__native_attr", 2, store.clone(), native_attr);
    register_native(
        context,
        "__native_attribs",
        1,
        store.clone(),
        native_attribs,
    );
    register_native(
        context,
        "__native_set_attr",
        3,
        store.clone(),
        native_set_attr,
    );
    register_native(context, "__native_first", 1, store.clone(), native_first);
    register_native(
        context,
        "__native_node_type",
        1,
        store.clone(),
        native_node_type,
    );
    register_native(
        context,
        "__native_tag_name",
        1,
        store.clone(),
        native_tag_name,
    );
    register_native(
        context,
        "__native_contents",
        1,
        store.clone(),
        native_contents,
    );
    register_native(
        context,
        "__native_each_count",
        1,
        store.clone(),
        native_each_count,
    );
    register_native(
        context,
        "__native_each_at",
        2,
        store.clone(),
        native_each_at,
    );
    register_native(
        context,
        "__native_all_handles",
        1,
        store.clone(),
        native_all_handles,
    );
    register_native(context, "__native_parent", 1, store.clone(), native_parent);
    register_native(
        context,
        "__native_children",
        1,
        store.clone(),
        native_children,
    );
    register_native(
        context,
        "__native_next_sibling",
        1,
        store.clone(),
        native_next_sibling,
    );
    register_native(
        context,
        "__native_prev_sibling",
        1,
        store.clone(),
        native_prev_sibling,
    );
    register_native(context, "__native_last", 1, store.clone(), native_last);
    register_native(context, "__native_remove", 1, store.clone(), native_remove);
    register_native(
        context,
        "__native_add_class",
        2,
        store.clone(),
        native_add_class,
    );
    register_native(
        context,
        "__native_remove_class",
        2,
        store.clone(),
        native_remove_class,
    );
    register_native(
        context,
        "__native_has_class",
        2,
        store.clone(),
        native_has_class,
    );
    register_native(
        context,
        "__native_remove_attr",
        2,
        store.clone(),
        native_remove_attr,
    );
    register_native(context, "__native_exists", 1, store.clone(), native_exists);
    register_native(context, "__native_is", 2, store.clone(), native_is);
    register_native(context, "__native_filter", 2, store.clone(), native_filter);
    register_native(context, "__native_not", 2, store.clone(), native_not);
    register_native(
        context,
        "__native_append_html",
        2,
        store.clone(),
        native_append_html,
    );
    register_native(
        context,
        "__native_set_html",
        2,
        store.clone(),
        native_set_html,
    );
    register_native(
        context,
        "__native_replace_with_html",
        2,
        store.clone(),
        native_replace_with_html,
    );
    register_native(
        context,
        "__native_wrap_html",
        2,
        store.clone(),
        native_wrap_html,
    );
    register_native(context, "__native_clone", 1, store.clone(), native_clone);
    register_native(
        context,
        "__native_before_html",
        2,
        store.clone(),
        native_before_html,
    );
    register_native(
        context,
        "__native_after_html",
        2,
        store.clone(),
        native_after_html,
    );
    register_native(
        context,
        "__native_set_text",
        2,
        store.clone(),
        native_set_text,
    );
    register_native(context, "__native_has", 2, store.clone(), native_has);
    register_native(
        context,
        "__native_siblings",
        1,
        store.clone(),
        native_siblings,
    );
    register_native(
        context,
        "__native_closest",
        2,
        store.clone(),
        native_closest,
    );
    register_native(
        context,
        "__native_next_until",
        2,
        store.clone(),
        native_next_until,
    );
    register_native(
        context,
        "__native_children_filtered",
        2,
        store.clone(),
        native_children_filtered,
    );
    register_native(
        context,
        "__native_remove_filtered",
        2,
        store.clone(),
        native_remove_filtered,
    );
    register_native(
        context,
        "__native_next_sibling_filtered",
        2,
        store.clone(),
        native_next_sibling_filtered,
    );
    register_native(
        context,
        "__native_prev_sibling_filtered",
        2,
        store.clone(),
        native_prev_sibling_filtered,
    );
    register_native(
        context,
        "__native_load_and_select_root",
        2,
        store.clone(),
        native_load_and_select_root,
    );
    register_native(
        context,
        "__native_select_and_outer_html",
        2,
        store.clone(),
        native_select_and_outer_html,
    );
    register_native(context, "__native_add", 2, store.clone(), native_add);
    register_native(context, "__native_parents", 2, store.clone(), native_parents);
    register_native(
        context,
        "__native_next_all",
        2,
        store.clone(),
        native_next_all,
    );
    register_native(
        context,
        "__native_prev_all",
        2,
        store.clone(),
        native_prev_all,
    );

    store
}
