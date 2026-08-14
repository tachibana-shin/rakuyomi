//! Native `require('htmlparser2')` binding: a single `dom_query` parse of the
//! whole document, replayed as a sequence of calls into the JS handler
//! object real htmlparser2's `new Parser({onopentag, ontext, onclosetag,
//! ...})` takes. Not a second HTML parser — `dom_query` (already used by
//! [`super::cheerio`]) does the actual parsing; this module only walks the
//! resulting tree and dispatches events, the same "native call reaches back
//! into JS" shape [`super::js_runtime::JsRuntime::call_plugin_method`]
//! already uses for the plugin's own async methods.
//!
//! Confirmed against the real fixtures that need it (`ranobes.js`): plugin
//! code does `new Parser({onopentag, ontext, onclosetag}); parser.write(html);
//! parser.end();` — no streaming, one whole document handed to `.write()`
//! before `.end()`.

use dom_query::{Document, NodeData, NodeRef};

use boa_engine::{
    js_string,
    native_function::NativeFunction,
    object::{FunctionObjectBuilder, ObjectInitializer},
    property::Attribute as PropAttribute,
    Context, JsArgs, JsError, JsNativeError, JsObject, JsResult, JsValue,
};

fn js_error(message: impl Into<String>) -> JsError {
    JsNativeError::typ().with_message(message.into()).into()
}

/// One resolved callback out of the real htmlparser2 `Handler` interface
/// (fb55/htmlparser2's `src/Parser.ts`). `onreset`/`onerror`/
/// `oncdatastart`/`oncdataend` are deliberately not in this set — see
/// [`native_htmlparser2_parse`]'s doc comment for why each one is an
/// architectural mismatch for this shim rather than a missing feature.
#[derive(Default)]
struct Handlers {
    onopentagname: Option<JsObject>,
    onopentag: Option<JsObject>,
    onattribute: Option<JsObject>,
    ontext: Option<JsObject>,
    onclosetag: Option<JsObject>,
    oncomment: Option<JsObject>,
    oncommentend: Option<JsObject>,
    onprocessinginstruction: Option<JsObject>,
}

/// Reads `handlers[name]` and returns it as a callable [`JsObject`], or
/// `None` if the property is missing/not a function — plugin code routinely
/// only supplies a subset of the handlers.
fn get_handler(
    handlers: &JsObject,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<JsObject>> {
    let value = handlers.get(js_string!(name), context)?;
    Ok(value.as_object().filter(|o| o.is_callable()))
}

fn call_handler(
    handler: &Option<JsObject>,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<()> {
    if let Some(f) = handler {
        f.call(&JsValue::undefined(), args, context)?;
    }
    Ok(())
}

/// Walks `node` and its descendants in document order, dispatching the
/// resolved [`Handlers`] for elements, text, comment, and processing-
/// instruction nodes. Doctype nodes are still skipped entirely (no
/// `Handler` callback exists for them in real htmlparser2 either).
///
/// `depth` guards against a native stack overflow (an uncatchable process
/// abort, not a recoverable JS exception) on pathologically/maliciously
/// deeply-nested HTML -- real-world markup never comes close to
/// `MAX_DEPTH`, so this only ever trips on input that would otherwise crash
/// the worker process outright.
fn walk(
    node: NodeRef<'_>,
    handlers: &Handlers,
    context: &mut Context,
    depth: usize,
) -> JsResult<()> {
    const MAX_DEPTH: usize = 500;
    if depth > MAX_DEPTH {
        return Err(js_error(format!(
            "HTML document nesting exceeds the maximum supported depth ({MAX_DEPTH})"
        )));
    }

    if node.is_element() {
        let name = node.node_name().unwrap_or_default().to_string();
        call_handler(
            &handlers.onopentagname,
            &[JsValue::from(js_string!(name.as_str()))],
            context,
        )?;

        // Only fetch/build attribute data when a handler actually wants it
        // -- most plugin `Parser` calls only supply a subset of the
        // handlers, and building a JS object per element (`ObjectInitializer`)
        // for a handler nobody registered would be pure waste on documents
        // with thousands of elements.
        if handlers.onattribute.is_some() || handlers.onopentag.is_some() {
            let attrs = node.attrs();

            for attr in &attrs {
                call_handler(
                    &handlers.onattribute,
                    &[
                        JsValue::from(js_string!(attr.name.local.as_ref())),
                        JsValue::from(js_string!(attr.value.as_ref())),
                    ],
                    context,
                )?;
            }

            if handlers.onopentag.is_some() {
                let mut attribs_builder = ObjectInitializer::new(context);
                for attr in &attrs {
                    attribs_builder.property(
                        js_string!(attr.name.local.as_ref()),
                        js_string!(attr.value.as_ref()),
                        PropAttribute::all(),
                    );
                }
                let attribs_object = attribs_builder.build();
                call_handler(
                    &handlers.onopentag,
                    &[
                        JsValue::from(js_string!(name.as_str())),
                        JsValue::from(attribs_object),
                    ],
                    context,
                )?;
            }
        }

        let mut child = node.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            walk(c, handlers, context, depth + 1)?;
            child = next;
        }

        call_handler(
            &handlers.onclosetag,
            &[JsValue::from(js_string!(name.as_str()))],
            context,
        )?;
    } else if node.is_text() {
        let text = node
            .query(|n| match &n.data {
                NodeData::Text { contents } => Some(contents.to_string()),
                _ => None,
            })
            .flatten()
            .unwrap_or_default();
        call_handler(
            &handlers.ontext,
            &[JsValue::from(js_string!(text.as_str()))],
            context,
        )?;
    } else if node.is_comment() {
        let text = node
            .query(|n| match &n.data {
                NodeData::Comment { contents } => Some(contents.to_string()),
                _ => None,
            })
            .flatten()
            .unwrap_or_default();
        call_handler(
            &handlers.oncomment,
            &[JsValue::from(js_string!(text.as_str()))],
            context,
        )?;
        call_handler(&handlers.oncommentend, &[], context)?;
    } else {
        let pi = node.query(|n| match &n.data {
            NodeData::ProcessingInstruction { target, contents } => {
                Some((target.to_string(), contents.to_string()))
            }
            _ => None,
        });
        if let Some(Some((target, contents))) = pi {
            call_handler(
                &handlers.onprocessinginstruction,
                &[
                    JsValue::from(js_string!(target.as_str())),
                    JsValue::from(js_string!(contents.as_str())),
                ],
                context,
            )?;
            return Ok(());
        }
        // Document/Fragment/Doctype: not a tag, text run, comment, or PI
        // itself, but a Fragment/Document root can still have such
        // children (that's the normal case for a whole parsed document) —
        // recurse into children without emitting an event for the
        // container node itself.
        let mut child = node.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            walk(c, handlers, context, depth + 1)?;
            child = next;
        }
    }
    Ok(())
}

/// `__native_htmlparser2_parse(html, handlers) -> undefined`. Parses `html`
/// once via `Document::fragment` (which does wrap the content in a
/// synthetic `<html>` element, but no `<head>`/`<body>` layer beneath it —
/// see the wrapper-skipping logic below), walks the resulting tree
/// dispatching [`Handlers`], then calls `handlers.onend()` once done.
///
/// Three real htmlparser2 `Handler` callbacks are deliberately NOT
/// implemented, each for an architectural reason rather than being an
/// oversight (checked against `dom_query`'s own `NodeData`/`Document` API,
/// not assumed):
/// - `onerror` — real htmlparser2 reports tokenizer-level malformed-markup
///   errors; `dom_query`'s underlying `html5ever` parser is tolerant by
///   design (HTML5's own error-recovery rules mean malformed markup still
///   parses successfully) and doesn't expose a per-node error channel
///   through `dom_query`'s API for this to hook into.
/// - `onparserinit`/`onreset` — real htmlparser2 fires these around
///   incremental/streaming parse state changes. This shim's parse is
///   always synchronous, one-shot, and either fully succeeds or throws a
///   catchable JS exception — there is no "reset" state to report, and
///   `onparserinit` would have nothing meaningful to pass besides the
///   `Parser` instance itself before any real parsing has happened.
/// - `oncdatastart`/`oncdataend` — `dom_query`'s `NodeData` enum has no
///   CDATA variant at all; HTML5 parsing (unlike XML) treats CDATA-like
///   syntax as a bogus comment, so there is no underlying node kind for
///   these to ever fire from when parsing HTML input.
fn native_htmlparser2_parse(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let html = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let handlers_obj = args
        .get_or_undefined(1)
        .as_object()
        .ok_or_else(|| js_error("htmlparser2 Parser: handlers must be an object"))?
        .clone();

    let handlers = Handlers {
        onopentagname: get_handler(&handlers_obj, "onopentagname", context)?,
        onopentag: get_handler(&handlers_obj, "onopentag", context)?,
        onattribute: get_handler(&handlers_obj, "onattribute", context)?,
        ontext: get_handler(&handlers_obj, "ontext", context)?,
        onclosetag: get_handler(&handlers_obj, "onclosetag", context)?,
        oncomment: get_handler(&handlers_obj, "oncomment", context)?,
        oncommentend: get_handler(&handlers_obj, "oncommentend", context)?,
        onprocessinginstruction: get_handler(&handlers_obj, "onprocessinginstruction", context)?,
    };
    let onend = get_handler(&handlers_obj, "onend", context)?;

    let doc = Document::fragment(html);
    // `doc.root()` is a bare Fragment marker (not itself an element, so the
    // existing `walk()` Document/Fragment branch already handles it
    // correctly on its own) whose one and only child is a SYNTHETIC
    // `<html>` wrapper `Document::fragment()` adds around the parsed
    // content — confirmed empirically (this pass's own `onopentagname`
    // regression test caught the alternative: walking `doc.root()`
    // directly dispatched a spurious `onopentagname("html")`/
    // `onclosetag("html")` pair real htmlparser2 never emits for fragment
    // input). Unlike a full `Document::from()` parse, there is no further
    // `<head>`/`<body>` layer underneath — the real content is the
    // `<html>` wrapper's direct children. Skip exactly that one synthetic
    // level, not `walk()`'s own Fragment-root handling (which stays
    // correct and is reused as-is for every level below this one).
    let html_wrapper = doc.root().first_child();
    let mut child = html_wrapper.and_then(|html| html.first_child());
    while let Some(c) = child {
        let next = c.next_sibling();
        walk(c, &handlers, context, 0)?;
        child = next;
    }
    call_handler(&onend, &[], context)?;

    Ok(JsValue::undefined())
}

/// Registers `__native_htmlparser2_parse` as a global function in `context`.
/// [`super::js_runtime::RUNTIME_PRELUDE`] wraps it as `require('htmlparser2')`'s
/// `Parser` class (`.write(chunk)` buffers, `.end()` triggers the actual
/// native parse+replay — matching the "single native parse" design, not a
/// second incremental parser).
pub(super) fn register(context: &mut Context) {
    let native = NativeFunction::from_fn_ptr(native_htmlparser2_parse);
    let func = FunctionObjectBuilder::new(context.realm(), native)
        .name("__native_htmlparser2_parse")
        .length(2)
        .build();
    context
        .global_object()
        .set(
            js_string!("__native_htmlparser2_parse"),
            func,
            false,
            context,
        )
        .expect("registering __native_htmlparser2_parse should not fail");
}
