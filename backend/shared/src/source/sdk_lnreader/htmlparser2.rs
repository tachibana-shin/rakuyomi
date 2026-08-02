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
//! before `.end()`. `onattribute`/`onend` aren't exercised by that fixture
//! but are implemented anyway for fidelity with the wider ~133-source corpus.

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

/// Reads `handlers[name]` and returns it as a callable [`JsObject`], or
/// `None` if the property is missing/not a function — plugin code routinely
/// only supplies a subset of the 5 handlers.
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

/// Walks `node` and its descendants in document order, dispatching
/// `onopentag`/`onattribute`/`ontext`/`onclosetag` for elements and text
/// nodes. Comments/doctype/processing-instruction nodes are skipped (real
/// htmlparser2 has `oncomment`/etc. too, but no fixture needs them).
fn walk(
    node: NodeRef<'_>,
    onopentag: &Option<JsObject>,
    onattribute: &Option<JsObject>,
    ontext: &Option<JsObject>,
    onclosetag: &Option<JsObject>,
    context: &mut Context,
) -> JsResult<()> {
    if node.is_element() {
        let name = node.node_name().unwrap_or_default().to_string();
        // Only fetch/build attribute data when a handler actually wants it
        // -- most plugin `Parser` calls only supply a subset of the 5
        // handlers, and building a JS object per element (`ObjectInitializer`)
        // for a handler nobody registered would be pure waste on documents
        // with thousands of elements.
        if onattribute.is_some() || onopentag.is_some() {
            let attrs = node.attrs();

            for attr in &attrs {
                call_handler(
                    onattribute,
                    &[
                        JsValue::from(js_string!(attr.name.local.as_ref())),
                        JsValue::from(js_string!(attr.value.as_ref())),
                    ],
                    context,
                )?;
            }

            if onopentag.is_some() {
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
                    onopentag,
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
            walk(c, onopentag, onattribute, ontext, onclosetag, context)?;
            child = next;
        }

        call_handler(
            onclosetag,
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
        call_handler(ontext, &[JsValue::from(js_string!(text.as_str()))], context)?;
    } else {
        // Document/Fragment/Comment/Doctype/ProcessingInstruction: not a tag
        // or text run itself, but a Fragment/Document root can still have
        // element/text children (that's the normal case for a whole parsed
        // document) — recurse into children without emitting an event for
        // the container node itself.
        let mut child = node.first_child();
        while let Some(c) = child {
            let next = c.next_sibling();
            walk(c, onopentag, onattribute, ontext, onclosetag, context)?;
            child = next;
        }
    }
    Ok(())
}

/// `__native_htmlparser2_parse(html, handlers) -> undefined`. Parses `html`
/// once via `Document::fragment` (no synthetic `<html><head><body>`
/// wrapping, matching real htmlparser2's raw-stream semantics — same choice
/// already documented for `cheerio::native_clone`'s use of `to_fragment()`),
/// walks the resulting tree, and calls `handlers.onopentag/onattribute/
/// ontext/onclosetag` along the way, then `handlers.onend()` once done.
fn native_htmlparser2_parse(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let html = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    let handlers = args
        .get_or_undefined(1)
        .as_object()
        .ok_or_else(|| js_error("htmlparser2 Parser: handlers must be an object"))?
        .clone();

    let onopentag = get_handler(&handlers, "onopentag", context)?;
    let onattribute = get_handler(&handlers, "onattribute", context)?;
    let ontext = get_handler(&handlers, "ontext", context)?;
    let onclosetag = get_handler(&handlers, "onclosetag", context)?;
    let onend = get_handler(&handlers, "onend", context)?;

    let doc = Document::fragment(html);
    walk(
        doc.root(),
        &onopentag,
        &onattribute,
        &ontext,
        &onclosetag,
        context,
    )?;
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
