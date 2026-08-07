//! Boa `Context` setup for LNReader plugins: registers the cheerio shim
//! ([`super::cheerio`]) and `fetch` ([`super::net`]) natives, evaluates the
//! cheerio JS prelude plus a small CommonJS `require()` shim, loads
//! `Payload/main.js`, and drives its `async` methods to completion.
//!
//! This is a dedicated, separate `Context` per loaded source — not the
//! sandboxed one already used by `wasm_imports/next/js.rs` for Aidoku-next
//! sources, which serves an unrelated purpose (small anti-bot/deobfuscation
//! scripts) and shouldn't be coupled to a much larger, more permissive
//! runtime.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _, Result};
use boa_engine::{
    builtins::promise::PromiseState,
    js_string,
    native_function::NativeFunction,
    object::{builtins::JsPromise, FunctionObjectBuilder},
    property::PropertyKey,
    Context, JsArgs, JsValue, Source as JsSource,
};
use boa_gc::{empty_trace, Finalize, Trace};

use crate::settings::SourceSettingValue;
use crate::source::model::{Chapter, Manga};

use super::{cheerio, dayjs, htmlparser2, net};

/// How long a `parseNovel()` result stays available for reuse by a second,
/// immediately-following call for the same novel — see
/// [`JsRuntime::take_cached_novel`]/[`JsRuntime::cache_novel`]. Deliberately
/// short: this exists only to collapse the `get_manga_details` +
/// `get_chapter_list` pair the real UI issues back-to-back when a novel is
/// opened (both call `parseNovel()` independently — it's the one plugin
/// method that returns both metadata and the chapter list, see
/// `worker.rs::parse_and_convert_novel`), not to serve as a general-purpose
/// novel metadata cache. Revisiting the same novel later must still
/// re-fetch normally — the TTL bounds staleness even if only one of the two
/// calls ever happens, and [`JsRuntime::take_cached_novel`] additionally
/// consumes (clears) the entry on a hit, so a *third* call never reuses it
/// either.
const NOVEL_CACHE_TTL: Duration = Duration::from_secs(10);

/// A `parseNovel()` result stashed as plain Rust data, not a `JsValue` —
/// holding a `JsValue`/`Gc<T>` across calls risks a use-after-free once
/// boa's collector runs a cycle it isn't rooted for (see
/// [`JsRuntime::call_plugin_method`]'s doc comment on why the plugin
/// instance itself is re-fetched every call rather than cached for the same
/// reason), so the value is converted to `Manga`/`Vec<Chapter>` immediately
/// after the call and only that is cached.
struct NovelCacheEntry {
    manga_id: String,
    manga: Manga,
    chapters: Vec<Chapter>,
    cached_at: Instant,
}

/// The chainable cheerio-like API (`$('sel').find().text()`), rebuilt in JS
/// on top of the `__native_*` primitives in [`super::cheerio`]. Ported from
/// the validated PoC (`docs/lnreader/poc-reference-main.rs`), translated to
/// English; logic unchanged.
const CHEERIO_PRELUDE: &str = r#"
// Arrays returned by slice/filter/has need to keep cheerio's (index, element)
// convention for .each()/.map(), NOT the native JS array convention
// (element, index) -- otherwise any chained .filter(x).each((i, el) => ...)
// silently breaks (wrong argument order). We wrap the raw array with
// hand-rolled .each()/.map() methods, while keeping it usable as a normal
// array (length, indexing, for...of, etc.).
function toChain(arr) {
    // `.call(el, ...)`, not a plain `callback(...)`, on both of these --
    // real cheerio's `.each()`/`.map()` bind `this` to the current element
    // INSIDE the callback, a real, common, and independent idiom from the
    // `(index, element)` arguments (`.each(function () { var $el = $(this);
    // ... })`, no declared parameters at all). A plain call left `this`
    // as boa's global object inside such a callback; `$(this)` then wrapped
    // THAT (not the element) via `native_select_root`'s selector path,
    // `arg_string` stringified it to the literal text "[object Object]" (JS's
    // own default `Object.prototype.toString` conversion), and searching for
    // that as a CSS selector threw "invalid CSS selector". Found via
    // `novel-lucky.js`'s `parseNovel`, one of the shared `MadaraPlugin`-base
    // sources.
    arr.each = function (callback) {
        for (var i = 0; i < this.length; i++) callback.call(this[i], i, this[i]);
        return this;
    };
    arr.map = function (callback) {
        var out = [];
        for (var i = 0; i < this.length; i++) out.push(callback.call(this[i], i, this[i]));
        return toChain(out);
    };
    // .get(index?) -- without an argument, returns a genuinely plain JS
    // array (mirrors real cheerio converting a collection to a plain
    // array). Must be a fresh `.slice()`, not `this` -- returning `this`
    // left every `.each`/`.map`/`.get`/etc. override from `toChain` still
    // attached, so plugin code chaining a *native*-convention
    // `.map((element, index) => ...)`/`.filter()` onto the result of
    // `.get()` (a very common real pattern: cheerio-convention `.map()`
    // inside the chain, then plain-array methods after `.get()` exits it --
    // found via `kisswood.js`'s `parseChapter`) silently got this module's
    // `(index, element)` cheerio convention instead of the real,
    // spec-correct `(element, index)` one, with no type error at the call
    // site itself (JS doesn't check parameter names) -- only later, as
    // "TypeError: not a callable function" when the swapped-in "element"
    // (actually the index, a number) didn't have whatever string/element
    // method the plugin called on it. `.slice()` is real, native
    // `Array.prototype.slice` (still present -- `toChain` only ever *adds*
    // properties, never removes native ones), so the copy it returns was
    // never itself passed through `toChain` and carries none of the
    // overrides.
    arr.get = function (index) {
        if (index === undefined) return this.slice();
        return this[index];
    };
    // `.toArray()` -- found missing (`TypeError: not a callable function`)
    // via `readfrom.js`'s `parseNovels`, which does the extremely common
    // real cheerio idiom `selection.map(fn).toArray()`: `.map()` above
    // returns a `toChain()`-wrapped plain array (not a `CheerioSelection`,
    // which already has its own `.toArray` from the native side), and until
    // now `toChain()` never added one -- any plugin calling `.toArray()`
    // directly after `.map()`/`.filter()`/`.slice()`/etc. hit this, a
    // pattern confirmed common across the corpus (`grep -c
    // '\.map(...).*\.toArray()'`). Same semantics as `.get()` with no
    // argument: a genuine plain-array copy, real cheerio's own behavior.
    arr.toArray = function () {
        return this.slice();
    };
    // Real-world plugin code calls READ and MUTATION methods directly on the
    // result of a filter/navigation, not just element-by-element via each().
    // Following real cheerio's convention: READ methods act on the FIRST
    // element of the collection (like .text()/.attr() do in real cheerio),
    // MUTATION methods act on ALL elements.
    var readMethods = ['first', 'last', 'text', 'attr', 'html', 'outerHtml', 'is', 'exists', 'data', 'hasClass'];
    readMethods.forEach(function (name) {
        arr[name] = function () {
            if (this.length === 0) {
                // No element: return a sensible "empty" value for the method
                // kind, instead of crashing on an empty array.
                if (name === 'first' || name === 'last') return toChain([]);
                if (name === 'exists') return false;
                if (name === 'is' || name === 'hasClass') return false;
                // `.text()` on an empty selection is '' in real cheerio (it
                // concatenates the text of every matched element -- zero
                // elements concatenate to ''), NEVER null, unlike
                // `.attr()`/`.html()`/`.data()` which have no sensible
                // non-null empty value (there's no element to read from).
                // Found via `chireads.js`: a real, live "no results this
                // page" category page whose placeholder `<li>` has no `<div>`
                // at all, so `.contents().find("div").first().text()` hits
                // exactly this empty-collection path -- the extremely common
                // real cheerio idiom `.text().trim()` then called `.trim()`
                // on the wrongly-returned `null`, throwing "cannot convert
                // 'null' or 'undefined' to object".
                if (name === 'text') return '';
                return null;
            }
            var first = this[0];
            return first[name].apply(first, arguments);
        };
    });
    // .eq(index) -- found missing while adding regression tests for
    // .parents()/.nextAll()/.prevAll() this pass: any toChain()-wrapped
    // array (.siblings()/.contents()/.parents()/.nextAll()/.prevAll()/...)
    // had no .eq() at all, silently falling through to nothing and crashing
    // "not a callable function" on the very next call -- the same failure
    // shape as every other toChain() gap already documented above, just not
    // yet hit by a real corpus source. Mirrors CheerioSelection.prototype.eq
    // (out-of-range, including negative, returns an empty chain -- not real
    // cheerio's own negative-index-from-the-end behavior, kept consistent
    // with the native .eq()'s existing same limitation rather than fixing
    // one without the other).
    arr.eq = function (index) {
        if (index < 0 || index >= this.length) return toChain([]);
        return toChain([this[index]]);
    };
    var mutateMethods = ['remove', 'addClass', 'removeClass', 'removeAttr'];
    mutateMethods.forEach(function (name) {
        arr[name] = function () {
            var args = arguments;
            this.each(function (_i, el) { el[name].apply(el, args); });
            return this;
        };
    });
    // .find(selector) -- a TRAVERSAL method, unlike the read/mutate ones
    // above: real cheerio searches descendants of EVERY element in the
    // collection and unions the results, not just the first. Found missing
    // (`TypeError: Array.prototype.find: predicate is not callable`) via
    // `chireads.js`'s `r(t).contents().find("div")`: `.contents()` returns a
    // `toChain()`-wrapped plain array of `CheerioSelection`s (not a native
    // `CheerioSelection` itself, which already has its own selector-aware
    // `.find` from the native side), and with no override here the call fell
    // through to native `Array.prototype.find`, which treats its argument as
    // a predicate FUNCTION -- the selector string "div" isn't one.
    arr.find = function (selector) {
        var out = [];
        for (var i = 0; i < this.length; i++) {
            var found = this[i].find(selector);
            for (var j = 0; j < found.length; j++) out.push(found[j]);
        }
        return toChain(out);
    };
    // .filter(selectorOrFn) -- without this override, calling .filter() on a
    // toChain()-wrapped array (e.g. the result of .contents()/.siblings())
    // fell through to NATIVE Array.prototype.filter: wrong argument order
    // ((element, index) instead of cheerio's (index, element)), no
    // `this`-binding, AND its result is a fresh plain array carrying none of
    // toChain()'s methods (native .filter() doesn't call back into
    // toChain()). Found via `archiveofourown.js`'s parseChapter, whose
    // `l.contents().filter((e, a) => 3 === a.nodeType).text()` crashed with
    // "TypeError: not a callable function" on that final `.text()` -- not
    // because .text() itself was missing, but because the object it was
    // called on was a bare array that never went through toChain() in the
    // first place. Mirrors CheerioSelection.prototype.filter's two call
    // forms (selector string vs. predicate function).
    arr.filter = function (selectorOrFn) {
        var out = [];
        if (typeof selectorOrFn === 'function') {
            for (var i = 0; i < this.length; i++) {
                if (selectorOrFn.call(this[i], i, this[i])) out.push(this[i]);
            }
        } else {
            for (var j = 0; j < this.length; j++) {
                if (this[j].is(selectorOrFn)) out.push(this[j]);
            }
        }
        return toChain(out);
    };
    return arr;
}

function CheerioSelection(id) {
    this.__id = id;
    Object.defineProperty(this, 'length', {
        get: function () { return __native_each_count(this.__id); },
    });
    // `.attribs` -- real cheerio's `.each()`/`.map()`/`.filter()` callbacks
    // hand back a raw DOM node (has `.attribs`, `.name`, `.type`...), and
    // plugin code very commonly reads `.attribs.class`/`.href`/`.title`
    // straight off that node instead of re-wrapping it with `$(el)` first.
    // `CheerioSelection` isn't that raw node -- it's this module's own
    // wrapper -- so found missing (`TypeError: cannot convert 'null' or
    // 'undefined' to object` on `el.attribs.title`) via 89/261 real corpus
    // sources doing exactly this. A lazy getter (not a field snapshotted at
    // construction) for the same reason `URL`'s `.search`/`.href` above
    // are getters: cheap enough per-access, and avoids a native round trip
    // for selections that never read it.
    Object.defineProperty(this, 'attribs', {
        get: function () { return JSON.parse(__native_attribs(this.__id)); },
    });
    // `.nodeType` -- a NUMBER property (1 = element, 3 = text, 8 = comment,
    // matching real DOM's ELEMENT_NODE/TEXT_NODE/COMMENT_NODE constants),
    // not a method. Found needed (as a bare property, never called) via 74
    // real corpus sources sharing the same `.contents().filter((i, el) =>
    // 3 === el.nodeType)` idiom (a shared base-plugin helper for splitting
    // an element's own loose text from its child tags -- `archiveofourown.js`
    // is one of the 74) -- comparing a NUMBER against this module's old
    // `.nodeType()` METHOD (a function reference) was always false,
    // silently discarding the exact text the plugin was trying to extract.
    // A getter, like `.attribs` above, not a snapshotted field: cheap enough
    // per access and consistent with the rest of this constructor.
    Object.defineProperty(this, 'nodeType', {
        get: function () {
            var t = __native_node_type(this.__id);
            if (t === 'tag') return 1;
            if (t === 'text') return 3;
            if (t === 'comment') return 8;
            return 0;
        },
    });
    // `.name` -- the LOWERCASE tag name, real domhandler/cheerio's raw-node
    // property (`undefined` on non-element nodes: text, comments...),
    // distinct from `.prop("tagName")` above (UPPERCASE, browser-DOM
    // convention) -- both real, both used by different plugins, so both
    // need to coexist rather than picking one. Reuses
    // `__native_tag_name`'s existing uppercase result (`.prop("tagName")`'s
    // primitive) rather than adding a second native call for what's just a
    // casing difference. Found via `novelfire.js`'s `parseChapter`, which
    // walks a `.find(":not(p, h1, ...)")` result checking
    // `s.name.toString().substring(0, 1) === "nf"` to strip the site's own
    // injected anti-scraping tags -- `.name` was `undefined` on every
    // element (property didn't exist at all), crashing
    // `undefined.toString()` with "cannot convert 'null' or 'undefined' to
    // object".
    Object.defineProperty(this, 'name', {
        get: function () {
            var upper = __native_tag_name(this.__id);
            return upper ? upper.toLowerCase() : undefined;
        },
    });
    // Array-like numeric indexing (`selection[0]`) -- real cheerio
    // collections are array-likes, and plugin code commonly reaches into
    // one directly (`.find(sel)[0]`, `.children()[0]`) to get the raw
    // element rather than going through `.eq()`/`.get()`. Found via
    // `yomou.syosetu.js`'s `a.children()[0].attribs.href` (`a.children()`
    // returned a `CheerioSelection` with no "0" property, so the whole
    // expression read `undefined.attribs`). Defined eagerly here (not a
    // Proxy -- this runtime has no need for one elsewhere, and one-off
    // native calls per index at construction time is cheap enough at this
    // corpus's real element-collection sizes) so indexing behaves exactly
    // like `.eq(i)`: a fresh single-element `CheerioSelection`, which
    // already carries its own `.attribs`/`.text()`/etc.
    var indexCount = __native_each_count(id);
    var _wrapAt = function (self, i) {
        Object.defineProperty(self, i, {
            get: function () { return new CheerioSelection(__native_each_at(this.__id, i)); },
            enumerable: true,
        });
    };
    for (var __i = 0; __i < indexCount; __i++) _wrapAt(this, __i);
}
CheerioSelection.prototype.find = function (selector) {
    var result = new CheerioSelection(__native_find(this.__id, selector));
    result.__prev = this;
    return result;
};
// .text(newValue?) -- overloaded getter/setter/setter-function, like real
// cheerio. The function form receives (index, currentText) and returns the
// new text, applied element by element (not the same text everywhere).
CheerioSelection.prototype.text = function (newTextOrFn) {
    if (newTextOrFn === undefined) {
        return __native_text(this.__id);
    }
    if (typeof newTextOrFn === 'function') {
        this.each(function (i, el) {
            var newText = newTextOrFn.call(el, i, el.text());
            __native_set_text(el.__id, newText);
        });
        return this;
    }
    __native_set_text(this.__id, newTextOrFn);
    return this;
};
// .html(newContent?) -- overloaded getter/setter, like real cheerio.
CheerioSelection.prototype.html = function (newHtml) {
    if (newHtml === undefined) {
        return __native_inner_html(this.__id);
    }
    __native_set_html(this.__id, newHtml);
    return this;
};
CheerioSelection.prototype.outerHtml = function () {
    return __native_outer_html(this.__id);
};
// .attr(name, value?) -- overloaded getter/setter. The object form
// .attr({name: value, ...}) sets multiple attributes at once.
CheerioSelection.prototype.attr = function (name, value) {
    if (typeof name === 'object' && name !== null) {
        for (var key in name) {
            if (Object.prototype.hasOwnProperty.call(name, key)) {
                __native_set_attr(this.__id, key, name[key]);
            }
        }
        return this;
    }
    if (value === undefined) {
        return __native_attr(this.__id, name);
    }
    __native_set_attr(this.__id, name, value);
    return this;
};
CheerioSelection.prototype.first = function () {
    return new CheerioSelection(__native_first(this.__id));
};
CheerioSelection.prototype.last = function () {
    return new CheerioSelection(__native_last(this.__id));
};
CheerioSelection.prototype.parent = function () {
    return new CheerioSelection(__native_parent(this.__id));
};
// .children(selector?) -- dom_query's children() takes no selector.
// __native_children_filtered does the per-child selector test in the SAME
// Rust-side loop that walks the children (one native call total, only
// matches ever get a handle) -- same shape as .has()/.not()/.siblings(sel)
// below, replacing an earlier composed version that built a
// CheerioSelection for EVERY child before filtering any of them out
// (native_children + ctor + .toArray() + N x .is()).
CheerioSelection.prototype.children = function (selector) {
    if (!selector) return new CheerioSelection(__native_children(this.__id));
    var handles = __native_children_filtered(this.__id, selector);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
// .next(selector?) -- real cheerio tests ONLY the immediate next sibling,
// never searching further if it doesn't match. __native_next_sibling_filtered
// does the sibling-fetch + existence check + selector test in one native
// call, replacing the earlier native_next_sibling + ctor + .exists() + .is()
// chain (up to 4 calls for 1).
CheerioSelection.prototype.next = function (selector) {
    if (!selector) return new CheerioSelection(__native_next_sibling(this.__id));
    return new CheerioSelection(__native_next_sibling_filtered(this.__id, selector));
};
CheerioSelection.prototype.nextSibling = function () {
    return new CheerioSelection(__native_next_sibling(this.__id));
};
CheerioSelection.prototype.prevSibling = function () {
    return new CheerioSelection(__native_prev_sibling(this.__id));
};
// .prev(selector?) -- mirrors .next(selector) above (same reasoning, same
// "doesn't skip multiple siblings if the immediate one doesn't match"
// semantics, same single-native-call optimization), just the other
// direction. Found genuinely missing (not a deliberate narrowing) via
// `bakainua.js`'s `.prev("div.text-2xl")` call in its own `parseNovel`.
CheerioSelection.prototype.prev = function (selector) {
    if (!selector) return new CheerioSelection(__native_prev_sibling(this.__id));
    return new CheerioSelection(__native_prev_sibling_filtered(this.__id, selector));
};
// .remove(selector?) -- real cheerio filters the CURRENT set by selector
// before removing (elements that don't match stay in the DOM), not just an
// unconditional removal of the whole selection. Found needed via
// `mangatr.js`'s `.children().remove("h3, div")`: without this, every child
// was removed regardless of tag, not just the `h3`/`div` ones the plugin
// asked for. `dom_query` has no combined "remove matching descendants"
// primitive as ONE method, but `Selection::filter()` and `Selection::remove()`
// can run back-to-back inside a single native function without ever handing
// an intermediate handle back to JS -- __native_remove_filtered does exactly
// that, replacing an earlier composed `.filter(selector).each(el =>
// __native_remove(el.__id))` (3+2M calls) with 1.
CheerioSelection.prototype.remove = function (selector) {
    if (selector) {
        __native_remove_filtered(this.__id, selector);
        return this;
    }
    __native_remove(this.__id);
    return this;
};
CheerioSelection.prototype.addClass = function (name) {
    __native_add_class(this.__id, name);
    return this;
};
CheerioSelection.prototype.removeClass = function (name) {
    __native_remove_class(this.__id, name);
    return this;
};
CheerioSelection.prototype.hasClass = function (name) {
    return __native_has_class(this.__id, name);
};
CheerioSelection.prototype.removeAttr = function (name) {
    __native_remove_attr(this.__id, name);
    return this;
};
CheerioSelection.prototype.exists = function () {
    return __native_exists(this.__id);
};
// .prop(name) -- real cheerio/browser DOM intrinsic properties, distinct
// from .attr() (markup attributes). Only "tagName" is implemented (the
// overwhelmingly common real-world use, e.g. `el.prop("tagName") === "IMG"`
// to branch on element kind inside a `.find("*").each()` walk) -- anything
// else falls back to .attr(name), a reasonable approximation for the rest of
// real cheerio's .prop() surface (checked/selected/href/src/... all read as
// plain attributes in the actual HTML this runtime parses).
CheerioSelection.prototype.prop = function (name) {
    if (name === 'tagName') return __native_tag_name(this.__id);
    // "outerHTML" is an intrinsic property (full serialization), not a
    // markup attribute -- falling through to .attr("outerHTML") looked for a
    // literal attribute of that name (never present) and silently returned
    // null. Found via `novelupdates.js`'s `parseChapter`, which builds the
    // whole chapter body via `.map((i, el) => el.prop("outerHTML")).get()
    // .join("")` -- a real, corpus-confirmed content-correctness bug (every
    // paragraph came back `null`, joining into the literal string
    // "nullnullnull..."), not just a missing feature.
    if (name === 'outerHTML') return __native_outer_html(this.__id);
    return this.attr(name);
};
// .contents() -- every child of the first matched element, raw text INCLUDED
// (unlike .children(), which only keeps tags).
CheerioSelection.prototype.contents = function () {
    var handles = __native_contents(this.__id);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
CheerioSelection.prototype.is = function (selector) {
    return __native_is(this.__id, selector);
};
// .data(key) -- real cheerio just reads the underlying data-<key> attribute;
// a pure alias over .attr(), no new native code needed.
CheerioSelection.prototype.data = function (key) {
    return this.attr('data-' + key);
};
// __native_siblings does the optional selector test in the SAME Rust-side
// loop that walks the siblings (one native call total either way) -- only
// matches ever get a CheerioSelection handle, instead of an earlier version
// that built one for every sibling before filtering any of them out.
CheerioSelection.prototype.siblings = function (selector) {
    var handles = __native_siblings(this.__id, selector || null);
    var kids = [];
    for (var i = 0; i < handles.length; i++) {
        kids.push(new CheerioSelection(handles[i]));
    }
    return toChain(kids);
};
// .addBack() -- brings the previous selection (before the last
// .find()/.filter()/etc.) back into the current one. Free once we already
// have __prev (for .end()) and toChain() (for arrays).
CheerioSelection.prototype.addBack = function () {
    var prevArr = this.__prev ? this.__prev.toArray() : [];
    return toChain(prevArr.concat(this.toArray()));
};
// .before(html) / .after(html) -- native (dom_query 0.28's
// before_html()/after_html()). Unlike a composed outerHtml()+replaceWith()
// implementation, these don't replace the node -- just insert a sibling --
// so the live reference stays valid and .before(x).after(y) stays chainable.
CheerioSelection.prototype.before = function (html) {
    __native_before_html(this.__id, html);
    return this;
};
CheerioSelection.prototype.after = function (html) {
    __native_after_html(this.__id, html);
    return this;
};
CheerioSelection.prototype.wrap = function (wrapperHtml) {
    __native_wrap_html(this.__id, wrapperHtml);
    return this;
};
CheerioSelection.prototype.clone = function () {
    return new CheerioSelection(__native_clone(this.__id));
};
CheerioSelection.prototype.nextUntil = function (selector) {
    var handles = __native_next_until(this.__id, selector);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
// .add(selectorOrSelection) -- real cheerio's own document-root-scoped
// union. The selector-string form is native (__native_add, a direct
// dom_query Selection::add() mapping -- see its own doc comment for the one
// dom_query-specific edge case: adding from an empty starting selection
// finds nothing, unlike real cheerio). The CheerioSelection-argument form
// (uniting two already-matched selections) has no equivalent native
// primitive, so it's a plain JS array concat, the same technique
// .addBack() already uses for the same "combine two already-materialized
// selections" shape.
CheerioSelection.prototype.add = function (selectorOrSelection) {
    if (selectorOrSelection instanceof CheerioSelection) {
        return toChain(this.toArray().concat(selectorOrSelection.toArray()));
    }
    return new CheerioSelection(__native_add(this.__id, selectorOrSelection));
};
// .empty() -- removes all children, keeps the element itself. No dedicated
// native primitive needed: real cheerio's .empty() is exactly
// .html('') under a different name, and __native_set_html already exists.
CheerioSelection.prototype.empty = function () {
    return this.html('');
};
// .toString() -- real cheerio's Cheerio instances serialize (via implicit
// coercion in a template literal/string concatenation, or an explicit call)
// to their outer HTML. Without this, JS's default Object.prototype.toString
// silently produces the literal string "[object Object]" instead -- the
// same silent-wrong-data shape already found twice this session
// (toChain().get()'s cheerio-convention-argument mixup, .prop("outerHTML")'s
// null-join bug), not a crash, so worth having even though explicit calls to
// it are rare. Reuses .outerHtml()'s own native primitive directly.
CheerioSelection.prototype.toString = function () {
    return __native_outer_html(this.__id);
};
// .parents(selector?) -- ALL ancestors, farthest-first (real cheerio's own
// documented order: `parents()`'s source walks bottom-up then reverses
// before returning -- verified against cheeriojs/cheerio's actual source,
// not assumed). Distinct from the narrower, already-implemented
// .closest(selector) (nearest matching ancestor only, one result).
CheerioSelection.prototype.parents = function (selector) {
    var handles = __native_parents(this.__id, selector || null);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
// .nextAll(selector?) / .prevAll(selector?) -- every remaining sibling in
// one direction (not just the immediate one, unlike .next(selector)/
// .prev(selector)), in natural walk order (nearest sibling first for BOTH
// directions -- verified against real cheerio's source: neither reverses,
// only .parents() above does). A selector, if given, filters the whole
// walked set rather than stopping the walk early (that's .nextUntil()'s
// job, a different method with different semantics).
CheerioSelection.prototype.nextAll = function (selector) {
    var handles = __native_next_all(this.__id, selector || null);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
CheerioSelection.prototype.prevAll = function (selector) {
    var handles = __native_prev_all(this.__id, selector || null);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
CheerioSelection.prototype.append = function (html) {
    __native_append_html(this.__id, html);
    return this;
};
CheerioSelection.prototype.setHtml = function (html) {
    // Explicit alias of .html(x) (now getter/setter-overloaded above) --
    // kept because some plugin code prefers an explicit mutation name.
    return this.html(html);
};
CheerioSelection.prototype.replaceWith = function (html) {
    __native_replace_with_html(this.__id, html);
    return this;
};
CheerioSelection.prototype.each = function (callback) {
    var handles = __native_all_handles(this.__id);
    for (var i = 0; i < handles.length; i++) {
        // `.call(el, ...)`, not a plain call -- see `toChain`'s `.each`
        // above for why (real cheerio binds `this` to the element too, an
        // idiom independent of the `(index, element)` arguments).
        var el = new CheerioSelection(handles[i]);
        callback.call(el, i, el);
    }
    return this;
};
// .get(index) / .eq(index) -- without an argument, returns the full array
// (like .toArray()); with an index, a single element.
CheerioSelection.prototype.get = function (index) {
    if (index === undefined) {
        return this.toArray();
    }
    return new CheerioSelection(__native_each_at(this.__id, index));
};
CheerioSelection.prototype.eq = function (index) {
    return new CheerioSelection(__native_each_at(this.__id, index));
};
// .map(fn) -- no new native function: cheerio's .map() almost always
// transforms each matched element into a value (text, link...), so a plain
// JS array (rather than a new chainable "cheerio collection") covers real
// usage. Passed through toChain() so the resulting array still supports
// .get() -- needed for the real .map(fn).get() idiom.
CheerioSelection.prototype.map = function (callback) {
    var out = [];
    this.each(function (i, el) { out.push(callback.call(el, i, el)); });
    return toChain(out);
};
// .toArray() -- a genuinely PLAIN array, deliberately NOT toChain()-wrapped
// (unlike .map()/.filter()/etc. above), matching real cheerio: once you
// call .toArray(), you're holding a plain JS Array and any further
// .filter()/.map()/.each() on it uses NATIVE (element, index, array)
// argument order, not cheerio's (index, element) one -- exactly the
// contract toChain()'s OWN `arr.toArray`/`arr.get` already documented and
// relied on `.slice()` (untouched native method) to provide. This
// implementation used to go through `.map()`, which returns `toChain(out)`
// -- WRAPPED -- silently giving `.toArray()`'s result cheerio-convention
// `.filter()`/`.map()` overrides real cheerio never puts there. Harmless
// on its own (before toChain() gained its own `.filter()`, calling
// `.filter()` on the mismatched result still fell through to the correct
// native semantics by accident), but became a real bug the moment
// toChain() started overriding `.filter()` for OTHER reasons (see
// `arr.filter` above): `skythewoodtranslations.js`'s `getDoneProjects`
// does `s(...).toArray().filter((t) => s(t).attr("href"))` -- a
// single-parameter, native-convention callback -- and started receiving
// the numeric INDEX in `t` instead of the element, then feeding that
// number to `s(t)` as a selector, which crashed as
// "invalid CSS selector [0]: ... EmptySelector". Confirmed via
// corpus-wide grep that every real `.toArray().filter(...)`/
// `.toArray().map(...)` call site (3 + 3 corpus sources) uses this same
// single-param, element-first convention -- none expect cheerio's.
CheerioSelection.prototype.toArray = function () {
    var out = [];
    this.each(function (_i, el) { out.push(el); });
    return out;
};
CheerioSelection.prototype.slice = function (start, end) {
    return toChain(this.toArray().slice(start, end));
};
// _filterBy(predicate) -- shared helper for filter/not/has: all three did
// the exact same each()+collect loop, only the test changed.
function _filterBy(selection, predicate) {
    var out = [];
    selection.each(function (i, el) {
        if (predicate.call(el, i, el)) out.push(el);
    });
    return toChain(out);
}
// .filter(selectorOrFn) -- real cheerio accepts both a selector string and a
// (index, element) => bool function, so this shim supports both. The
// function form always stays composed (dom_query has no notion of
// "filter by arbitrary JS callback"); the selector-string form uses the
// native Selection::filter().
CheerioSelection.prototype.filter = function (selectorOrFn) {
    if (typeof selectorOrFn === 'function') {
        return _filterBy(this, selectorOrFn);
    }
    return new CheerioSelection(__native_filter(this.__id, selectorOrFn));
};
// .not(selectorOrFn) -- selector-string form uses the native __native_not
// (one call for the whole selection, mirrors .filter()'s selector-string
// form -- previously called __native_is once per element via _filterBy, a
// perf-pass fix); the function form stays composed, same reasoning as
// .filter()'s function form (an arbitrary JS predicate can't be evaluated
// natively).
CheerioSelection.prototype.not = function (selectorOrFn) {
    if (typeof selectorOrFn === 'function') {
        return _filterBy(this, function (i, el) { return !selectorOrFn(i, el); });
    }
    var handles = __native_not(this.__id, selectorOrFn);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
CheerioSelection.prototype.has = function (selector) {
    var handles = __native_has(this.__id, selector);
    var out = [];
    for (var i = 0; i < handles.length; i++) {
        out.push(new CheerioSelection(handles[i]));
    }
    return toChain(out);
};
CheerioSelection.prototype.closest = function (selector) {
    return new CheerioSelection(__native_closest(this.__id, selector));
};
// .end() -- returns to the previous selection in the chain. Pure JS (no
// native code needed): every method that derives a new CheerioSelection from
// "this" attaches a __prev reference to it.
CheerioSelection.prototype.end = function () {
    return this.__prev || this;
};

function cheerio_load(html) {
    // Real cheerio's `load()` also accepts an already-matched element/node
    // (not just an HTML string) as a way to re-scope a fresh `$` to just
    // that element's subtree -- e.g. `.map((i, el) => cheerio.load(el)(...))`
    // inside an outer `.map()`/`.each()`, using the raw element `.map()`
    // itself hands back rather than `.find()`-ing again from the outer
    // selection. Found via `LeafStudio.js`'s `parseNovelsList`
    // (`(0, r.load)(i)`, `i` being `.map()`'s own element argument): without
    // this, `html` here is a `CheerioSelection` object, and
    // `__native_load` (which expects a real HTML string) coerces it via
    // JS's default `ToString`, producing the literal text "[object Object]"
    // -- valid but useless HTML, silently parsed into an empty-of-real-content
    // document. Every subsequent `$('selector')` inside then matches
    // nothing, and read methods on that empty match (`.text()`) return `''`
    // per this module's own empty-selection convention (see `toChain`) --
    // not a crash, a *silently wrong, always-empty result*, exactly the kind
    // of bug `search_ok` alone can't catch. `.outerHtml()` serializes just
    // that element's subtree back to a string and reloads it as an
    // independent document -- doesn't preserve real cheerio's "same
    // underlying node" mutation-sharing semantics (out of scope for this
    // store's per-document ID model), but makes the overwhelmingly common
    // real-world case -- read-only scoped querying within a matched element
    // -- work correctly.
    if (html instanceof CheerioSelection) {
        html = html.outerHtml();
    }
    // __native_load_and_select_root folds "parse this HTML, then
    // immediately select 'html' from it" into one native call -- that exact
    // pair always happens back-to-back here (unlike `$(selector)` below,
    // which reuses `docId` across arbitrarily many LATER, independent
    // selector calls, so it still needs the two natives kept separate). 2
    // calls total for `root` below instead of the previous native_load +
    // native_select_root + ctor's `native_each_count` (3).
    var loaded = __native_load_and_select_root(html, 'html');
    var docId = loaded[0];
    // `$(el)` -- re-wrapping an already-loaded element (typically the
    // second argument of an `.each((i, el) => ...)` callback) is a real,
    // common cheerio idiom, distinct from `$('selector')`. Found missing
    // while testing against real sources (lnori.ts's getLibraryNovels does
    // `n(e).attr(...)` inside `.each()`, `n` being the loaded `$`).
    var $ = function (selectorOrElement) {
        if (selectorOrElement instanceof CheerioSelection) {
            return selectorOrElement;
        }
        // $(htmlString) -- real cheerio's other well-known call form besides
        // a CSS selector: a string starting with "<" creates a new,
        // DETACHED element/fragment instead of searching the loaded
        // document. Without this, such a string was handed to
        // `__native_select_root` as if it were a selector -- "<img />" isn't
        // valid CSS selector syntax, so this threw "invalid CSS selector"
        // rather than building the element. Found via `komga.js`'s
        // `replaceUrlToImageHref` (`a("<img />").attr({src, width,
        // height})`, then `.replaceWith()` into the document) -- a real
        // crash on a real source, not a hypothetical gap. Parsed the same
        // way `cheerio.load()` itself parses (a real html5ever document,
        // not `to_fragment()`'s orphan-`<body>` case `.clone()` has to
        // special-case) so `'body > *'`, not `'html > *'`, is the right
        // selector here. This throwaway document's `doc_id` is never used
        // again (unlike `docId` above), so the combined native call's own
        // id half is simply discarded (`frag[1]` only).
        if (typeof selectorOrElement === 'string' && /^\s*</.test(selectorOrElement)) {
            var frag = __native_load_and_select_root(selectorOrElement, 'body > *');
            return new CheerioSelection(frag[1]);
        }
        return new CheerioSelection(__native_select_root(docId, selectorOrElement));
    };
    // $.html(el) -- static form found in real plugin code, distinct from
    // $(el).html(). In real cheerio, $.html(el) serializes the passed
    // element (equivalent to el.outerHtml() here). With NO argument (found
    // via `kolnovel.js`, one of 35 corpus sources built on the shared
    // `LightNovelWPPlugin` base class, whose `parseNovels` does
    // `cheerio.load(html).html()` to get the whole document back as a
    // string for manual regexing) real cheerio serializes the whole
    // document -- `$(selectorOrElement)` above can't be reused for that
    // (calling it with `undefined` selects nothing, not root: `arg_string`
    // stringifies a missing arg to the literal text "undefined", which
    // matches zero real elements), so this selects "html" directly, which
    // `dom_query`/html5ever always produces even for fragment input (a
    // `<article>`-only string still parses into a full
    // `<html><head></head><body>...` tree).
    // __native_select_and_outer_html does the select + serialize in ONE
    // native call: the CheerioSelection this used to build via
    // `$('html').outerHtml()` was thrown away immediately after, so its
    // constructor's `native_each_count` call bought nothing -- 1 call
    // instead of 3.
    $.html = function (el) {
        if (el === undefined) return __native_select_and_outer_html(docId, 'html');
        return el.outerHtml();
    };
    // Real cheerio's `$` returned by `load()` isn't just a selector
    // factory -- it IS ITSELF a Cheerio-wrapped selection of the loaded
    // document's root, so calling a selection method DIRECTLY on `$`
    // (`$.text()`, `$.find(sel)`, `$.each(fn)`, no intervening `$('sel')`)
    // is valid, real cheerio usage, equivalent to calling it on `$('html')`.
    // Found via `novelfire.js`'s `getAllChapters`, which does
    // `(0, cheerio.load)(title).text()` to strip markup out of a chapter
    // title fragment -- `.text` (and everything else) didn't exist on our
    // bare `$` FUNCTION at all, crashing with "TypeError: not a callable
    // function". Binds every `CheerioSelection.prototype` method to a
    // `root` selection so both call forms work; deliberately AFTER the
    // `$.html` assignment above so that pre-existing static override (a
    // different real need, `kolnovel.js`) isn't clobbered by this blanket
    // copy -- `root.html` would otherwise overwrite it with a form that
    // doesn't accept `$.html(el)`'s explicit-element argument.
    var root = new CheerioSelection(loaded[1]);
    for (var rootMethod in CheerioSelection.prototype) {
        if (rootMethod === 'html') continue;
        if (typeof root[rootMethod] === 'function') {
            $[rootMethod] = root[rootMethod].bind(root);
        }
    }
    return $;
}
"#;

/// The `require()` shim and small `fetch`/`Response`/`FormData`/
/// `URLSearchParams` polyfills LNReader plugins are compiled against.
/// `@libs/fetch`'s `fetchApi`/`fetchText` bodies are ported near-verbatim
/// from `lnreader-plugins`' `src/lib/fetch.ts` (MIT), built on top of the
/// native `fetch` below instead of a real browser one.
const RUNTIME_PRELUDE: &str = r#"
// `console` polyfill, covering the complete standard surface (MDN) -- found
// missing entirely (`ReferenceError: console is not defined`) via
// `novelrest.js`'s `console.error(...)` in a catch block. This runtime has
// no side-channel for console output (the NDJSON worker protocol's stdout
// is the response itself), so these are deliberate no-ops rather than
// forwarding anywhere -- just enough that calling any of them doesn't
// crash the plugin.
var console = {
    log: function () {},
    warn: function () {},
    error: function () {},
    info: function () {},
    debug: function () {},
    trace: function () {},
    dir: function () {},
    dirxml: function () {},
    table: function () {},
    group: function () {},
    groupCollapsed: function () {},
    groupEnd: function () {},
    assert: function () {},
    count: function () {},
    countReset: function () {},
    time: function () {},
    timeEnd: function () {},
    timeLog: function () {},
    clear: function () {},
    exception: function () {},
    profile: function () {},
    profileEnd: function () {},
    timeStamp: function () {},
};
function Response(raw) {
    this.ok = raw.__ok;
    this.status = raw.__status;
    this.statusText = raw.__statusText;
    this.url = raw.__url;
    this.__body = raw.__body;
    var rawHeaders = raw.__headers;
    this.headers = {
        get: function (name) {
            var v = rawHeaders[String(name).toLowerCase()];
            return v === undefined ? null : v;
        },
    };
}
Response.prototype.text = function () { return this.__body; };
Response.prototype.json = function () { return JSON.parse(this.__body); };
Response.prototype.arrayBuffer = function () {
    throw new Error('not implemented: Response.arrayBuffer()');
};

// `Headers` -- found missing (`ReferenceError: Headers is not defined`) via
// `readfrom.js`, one of 8 real corpus sources constructing one (`this.headers
// = new Headers(s)`, then handed to `fetchApi` as `init.headers`). Data is
// stored as plain, lower-cased, own-enumerable properties directly on `this`
// (not in a private map) so it round-trips through `fetch()`'s existing
// `JSON.stringify(init.headers || {})` line exactly like a plain object
// already does -- the real `Headers` class's `.get()`/`.set()`/`.append()`/
// `.has()`/`.forEach()` API is layered on top of that same plain storage,
// not a separate implementation, so either calling convention (treat it like
// a plain object, or use the real Headers methods) sees the same data.
function Headers(init) {
    if (init) {
        for (var key in init) {
            if (Object.prototype.hasOwnProperty.call(init, key)) {
                this[String(key).toLowerCase()] = init[key];
            }
        }
    }
}
Headers.prototype.get = function (name) {
    var v = this[String(name).toLowerCase()];
    return v === undefined ? null : v;
};
Headers.prototype.set = function (name, value) {
    this[String(name).toLowerCase()] = String(value);
};
Headers.prototype.append = function (name, value) {
    var key = String(name).toLowerCase();
    this[key] = this[key] !== undefined ? this[key] + ', ' + String(value) : String(value);
};
Headers.prototype.has = function (name) {
    return Object.prototype.hasOwnProperty.call(this, String(name).toLowerCase());
};
Headers.prototype['delete'] = function (name) {
    delete this[String(name).toLowerCase()];
};
Headers.prototype.forEach = function (callback) {
    for (var key in this) {
        if (Object.prototype.hasOwnProperty.call(this, key)) callback(this[key], key, this);
    }
};
// .entries/.keys/.values -- closes out the real Headers surface (MDN,
// §1.2.8.1) except `.getSetCookie()`, deliberately not added: this runtime
// never processes a `Set-Cookie` response header anywhere (no cookie jar,
// no session concept), so it would have no data source to read from,
// unlike every other gap on this list which had a real, if narrow, use.
Headers.prototype.entries = function () {
    var self = this;
    var keys = [];
    for (var k in this) {
        if (Object.prototype.hasOwnProperty.call(this, k)) keys.push(k);
    }
    var i = 0;
    return {
        next: function () {
            if (i >= keys.length) return { done: true, value: undefined };
            var k = keys[i++];
            return { done: false, value: [k, self[k]] };
        },
    };
};
Headers.prototype.keys = function () {
    var keys = [];
    for (var k in this) {
        if (Object.prototype.hasOwnProperty.call(this, k)) keys.push(k);
    }
    var i = 0;
    return { next: function () {
        if (i >= keys.length) return { done: true, value: undefined };
        return { done: false, value: keys[i++] };
    } };
};
Headers.prototype.values = function () {
    var self = this;
    var keys = [];
    for (var k in this) {
        if (Object.prototype.hasOwnProperty.call(this, k)) keys.push(k);
    }
    var i = 0;
    return { next: function () {
        if (i >= keys.length) return { done: true, value: undefined };
        return { done: false, value: self[keys[i++]] };
    } };
};
Headers.prototype[Symbol.iterator] = function () {
    return this.entries();
};

// `atob`/`btoa` -- found missing via 3 real corpus sources: `ln.hako.js`/
// `WTRLAB.js` (`Uint8Array.from(atob(chunk), ...)`, a common
// image/font-obfuscation-decoding idiom in this corpus) and `komga.js`
// (`btoa(email + ":" + password)` for a Basic-Auth header). Plain-JS
// implementations (no native base64 codec assumed available) operating on
// "binary strings" the same way the real Web APIs do -- one JS UTF-16 code
// unit per byte, 0-255, not real Unicode text -- matching how callers in
// this corpus already use them.
var __B64_CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function btoa(input) {
    var str = String(input);
    var out = '';
    for (var i = 0; i < str.length; i += 3) {
        var b0 = str.charCodeAt(i) & 0xff;
        var has1 = i + 1 < str.length;
        var has2 = i + 2 < str.length;
        var b1 = has1 ? str.charCodeAt(i + 1) & 0xff : 0;
        var b2 = has2 ? str.charCodeAt(i + 2) & 0xff : 0;
        out += __B64_CHARS.charAt(b0 >> 2);
        out += __B64_CHARS.charAt(((b0 & 3) << 4) | (b1 >> 4));
        out += has1 ? __B64_CHARS.charAt(((b1 & 15) << 2) | (b2 >> 6)) : '=';
        out += has2 ? __B64_CHARS.charAt(b2 & 63) : '=';
    }
    return out;
}
function atob(input) {
    var str = String(input).replace(/=+$/, '');
    var out = '';
    var buffer = 0;
    var bits = 0;
    for (var i = 0; i < str.length; i++) {
        var idx = __B64_CHARS.indexOf(str.charAt(i));
        if (idx === -1) continue;
        buffer = (buffer << 6) | idx;
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            out += String.fromCharCode((buffer >> bits) & 0xff);
        }
    }
    return out;
}

// Minimal `TextDecoder` polyfill (UTF-8 only) -- found missing entirely
// (`ReferenceError: TextDecoder is not defined`) via `ln.hako.js`,
// `dreamyTranslations.js`, and `WTRLAB.js` (3/261 corpus sources), all part
// of the same content-deobfuscation family already noted for `atob`/`btoa`
// above: each decodes an obfuscated chapter body into a byte array (via XOR
// or reversal against the `atob`-decoded string), then needs
// `new TextDecoder('utf-8').decode(bytes)` to turn that byte array back into
// a real (potentially non-ASCII) string -- `String.fromCharCode` alone
// can't do this because UTF-8 multi-byte sequences don't map 1:1 to UTF-16
// code units. `encoding` is accepted but ignored (always decodes as UTF-8,
// the only encoding any real corpus call site requests).
function TextDecoder(encoding, options) {
    this.encoding = encoding || 'utf-8';
    // `.fatal`/`.ignoreBOM` -- the rest of the real constructor's read-only
    // properties (MDN, §1.2.8.1): stored so a plugin reading them back gets
    // its own requested values, even though `.decode()` itself doesn't act
    // on `fatal` (never throws) or `ignoreBOM` (this decoder doesn't strip
    // a BOM either way).
    options = options || {};
    this.fatal = !!options.fatal;
    this.ignoreBOM = !!options.ignoreBOM;
}
TextDecoder.prototype.decode = function (bytes) {
    var out = '';
    var i = 0;
    var len = bytes.length;
    while (i < len) {
        var b0 = bytes[i++];
        if (b0 < 0x80) {
            out += String.fromCharCode(b0);
        } else if ((b0 & 0xe0) === 0xc0 && i < len) {
            var b1 = bytes[i++];
            out += String.fromCharCode(((b0 & 0x1f) << 6) | (b1 & 0x3f));
        } else if ((b0 & 0xf0) === 0xe0 && i + 1 < len) {
            var c1 = bytes[i++], c2 = bytes[i++];
            out += String.fromCharCode(((b0 & 0x0f) << 12) | ((c1 & 0x3f) << 6) | (c2 & 0x3f));
        } else if ((b0 & 0xf8) === 0xf0 && i + 2 < len) {
            var d1 = bytes[i++], d2 = bytes[i++], d3 = bytes[i++];
            var cp = ((b0 & 0x07) << 18) | ((d1 & 0x3f) << 12) | ((d2 & 0x3f) << 6) | (d3 & 0x3f);
            cp -= 0x10000;
            out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
        } else {
            out += String.fromCharCode(0xfffd);
        }
    }
    return out;
};

// Minimal `TextEncoder` polyfill (UTF-8 only), the encode-side counterpart
// of `TextDecoder` above -- found missing (`ReferenceError: TextEncoder is
// not defined`) via `dreamyTranslations.js`, which does
// `(new TextEncoder).encode(str).slice(0, n)` to byte-slice a UTF-16 JS
// string at a UTF-8 byte offset before handing it to `TextDecoder`. Returns
// a plain JS array of byte values (not a real `Uint8Array`) since that's
// all `TextDecoder.decode` above and native array methods like `.slice()`
// require.
function TextEncoder() {
    this.encoding = 'utf-8';
}
TextEncoder.prototype.encode = function (str) {
    str = String(str == null ? '' : str);
    var out = [];
    for (var i = 0; i < str.length; i++) {
        var cp = str.charCodeAt(i);
        if (cp >= 0xd800 && cp <= 0xdbff && i + 1 < str.length) {
            var low = str.charCodeAt(i + 1);
            if (low >= 0xdc00 && low <= 0xdfff) {
                cp = ((cp - 0xd800) << 10) + (low - 0xdc00) + 0x10000;
                i++;
            }
        }
        if (cp < 0x80) {
            out.push(cp);
        } else if (cp < 0x800) {
            out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
        } else if (cp < 0x10000) {
            out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
        } else {
            out.push(
                0xf0 | (cp >> 18),
                0x80 | ((cp >> 12) & 0x3f),
                0x80 | ((cp >> 6) & 0x3f),
                0x80 | (cp & 0x3f)
            );
        }
    }
    return out;
};

function FormData() {
    this.__entries = [];
}
FormData.prototype.append = function (key, value) {
    this.__entries.push([String(key), String(value)]);
};
// .get/.getAll/.has/.set/.delete -- the rest of the standard, bounded
// FormData surface (§1.2/F this pass): `.append()` alone only covered
// always-add call sites (building a multipart body to send, `fetch()`'s own
// use of `FormData` above), not reading back or replacing a field already
// added to one before sending it -- a real, plausible idiom once a plugin
// builds a `FormData` object at all. Plain JS array operations over the
// same `__entries` storage `.append()`/`fetch()` already use, mirroring the
// equivalent `URLSearchParams` methods above -- no native primitive
// involved, and no dom_query concept applies here at all (this is a
// request-body shape, not DOM content).
FormData.prototype.get = function (key) {
    key = String(key);
    for (var i = 0; i < this.__entries.length; i++) {
        if (this.__entries[i][0] === key) return this.__entries[i][1];
    }
    return null;
};
FormData.prototype.getAll = function (key) {
    key = String(key);
    var out = [];
    for (var i = 0; i < this.__entries.length; i++) {
        if (this.__entries[i][0] === key) out.push(this.__entries[i][1]);
    }
    return out;
};
FormData.prototype.has = function (key) {
    key = String(key);
    for (var i = 0; i < this.__entries.length; i++) {
        if (this.__entries[i][0] === key) return true;
    }
    return false;
};
FormData.prototype.set = function (key, value) {
    key = String(key);
    value = String(value);
    var found = false;
    this.__entries = this.__entries.filter(function (entry) {
        if (entry[0] !== key) return true;
        if (!found) {
            found = true;
            entry[1] = value;
            return true;
        }
        return false;
    });
    if (!found) this.__entries.push([key, value]);
};
FormData.prototype['delete'] = function (key) {
    key = String(key);
    this.__entries = this.__entries.filter(function (entry) { return entry[0] !== key; });
};
// .entries/.keys/.values -- the rest of the real, bounded FormData surface
// (MDN, §1.2.8.1). Real FormData has NO .forEach() (confirmed against MDN --
// unlike Headers/URLSearchParams, which do), so none is added here either.
FormData.prototype.entries = function () {
    var entries = this.__entries;
    var i = 0;
    return {
        next: function () {
            if (i >= entries.length) return { done: true, value: undefined };
            return { done: false, value: [entries[i][0], entries[i++][1]] };
        },
    };
};
FormData.prototype.keys = function () {
    var entries = this.__entries;
    var i = 0;
    return {
        next: function () {
            if (i >= entries.length) return { done: true, value: undefined };
            return { done: false, value: entries[i++][0] };
        },
    };
};
FormData.prototype.values = function () {
    var entries = this.__entries;
    var i = 0;
    return {
        next: function () {
            if (i >= entries.length) return { done: true, value: undefined };
            return { done: false, value: entries[i++][1] };
        },
    };
};
FormData.prototype[Symbol.iterator] = function () {
    return this.entries();
};

// `init` can be a plain object of key/value pairs (the only form this
// polyfill originally supported) or a real query string, `?`-prefixed or
// not (the far more common real-world call shape, e.g.
// `new URLSearchParams(location.search)` or, since `URL` below builds on
// this same class, whatever `new URL(...).search` parses out) -- both are
// part of the real `URLSearchParams` constructor's spec.
function URLSearchParams(init) {
    this.__pairs = [];
    if (typeof init === 'string') {
        var query = init.charAt(0) === '?' ? init.slice(1) : init;
        if (query) {
            var parts = query.split('&');
            for (var i = 0; i < parts.length; i++) {
                if (!parts[i]) continue;
                var eq = parts[i].indexOf('=');
                var rawKey = eq === -1 ? parts[i] : parts[i].slice(0, eq);
                var rawValue = eq === -1 ? '' : parts[i].slice(eq + 1);
                this.__pairs.push([
                    decodeURIComponent(rawKey.replace(/\+/g, ' ')),
                    decodeURIComponent(rawValue.replace(/\+/g, ' ')),
                ]);
            }
        }
    } else if (init) {
        for (var key in init) {
            if (Object.prototype.hasOwnProperty.call(init, key)) {
                this.__pairs.push([key, init[key]]);
            }
        }
    }
}
URLSearchParams.prototype.append = function (key, value) {
    this.__pairs.push([String(key), String(value)]);
};
// .set/.get/.has/.delete -- the rest of the real `URLSearchParams` surface,
// found missing (`TypeError: not a callable function` calling the
// nonexistent `.set`) via `kakuyomu.js`'s `url.searchParams.set("q", i)`.
// `.append()` alone (added for `bakainua.js`, see the constructor comment
// above) only covers always-add call sites; `.set()` is the far more common
// "replace-or-add a single query param" idiom real plugin code uses when
// building a search/pagination URL on top of `new URL(...)`.
URLSearchParams.prototype.set = function (key, value) {
    key = String(key);
    value = String(value);
    var found = false;
    this.__pairs = this.__pairs.filter(function (pair) {
        if (pair[0] !== key) return true;
        if (!found) {
            found = true;
            pair[1] = value;
            return true;
        }
        return false;
    });
    if (!found) this.__pairs.push([key, value]);
};
URLSearchParams.prototype.get = function (key) {
    key = String(key);
    for (var i = 0; i < this.__pairs.length; i++) {
        if (this.__pairs[i][0] === key) return this.__pairs[i][1];
    }
    return null;
};
URLSearchParams.prototype.has = function (key) {
    key = String(key);
    for (var i = 0; i < this.__pairs.length; i++) {
        if (this.__pairs[i][0] === key) return true;
    }
    return false;
};
URLSearchParams.prototype.delete = function (key) {
    key = String(key);
    this.__pairs = this.__pairs.filter(function (pair) { return pair[0] !== key; });
};
URLSearchParams.prototype.toString = function () {
    return this.__pairs
        .map(function (pair) {
            return encodeURIComponent(pair[0]) + '=' + encodeURIComponent(pair[1]);
        })
        .join('&');
};
// .getAll/.forEach/.entries -- the rest of the standard, bounded
// URLSearchParams surface (§1.2/F this pass): unlike .get() (first match
// only), .getAll() returns every value for a repeated key -- a real,
// well-known query-string shape (`?tag=a&tag=b`). All three are plain JS
// array operations over the same `__pairs` storage every other method here
// already uses, no native primitive involved.
URLSearchParams.prototype.getAll = function (key) {
    key = String(key);
    var out = [];
    for (var i = 0; i < this.__pairs.length; i++) {
        if (this.__pairs[i][0] === key) out.push(this.__pairs[i][1]);
    }
    return out;
};
URLSearchParams.prototype.forEach = function (callback) {
    for (var i = 0; i < this.__pairs.length; i++) {
        callback(this.__pairs[i][1], this.__pairs[i][0], this);
    }
};
URLSearchParams.prototype.entries = function () {
    var pairs = this.__pairs;
    var i = 0;
    return {
        next: function () {
            if (i >= pairs.length) return { done: true, value: undefined };
            return { done: false, value: [pairs[i][0], pairs[i++][1]] };
        },
    };
};
// .keys/.values/.sort/.size -- closes out the real URLSearchParams surface
// (MDN, §1.2.8.1). `[Symbol.iterator]` is deliberately aliased to
// `.entries()` (real spec's own default iteration behavior) so
// `for (var pair of params)` works the same as `for (var pair of
// params.entries())`.
URLSearchParams.prototype.keys = function () {
    var pairs = this.__pairs;
    var i = 0;
    return {
        next: function () {
            if (i >= pairs.length) return { done: true, value: undefined };
            return { done: false, value: pairs[i++][0] };
        },
    };
};
URLSearchParams.prototype.values = function () {
    var pairs = this.__pairs;
    var i = 0;
    return {
        next: function () {
            if (i >= pairs.length) return { done: true, value: undefined };
            return { done: false, value: pairs[i++][1] };
        },
    };
};
URLSearchParams.prototype.sort = function () {
    this.__pairs.sort(function (a, b) {
        if (a[0] < b[0]) return -1;
        if (a[0] > b[0]) return 1;
        return 0;
    });
};
Object.defineProperty(URLSearchParams.prototype, 'size', {
    get: function () { return this.__pairs.length; },
});
URLSearchParams.prototype[Symbol.iterator] = function () {
    return this.entries();
};

// Minimal `URL` polyfill -- found missing entirely (`ReferenceError: URL is
// not defined`) via `bakainua.js`, one of ~37/261 real `lnreader-plugins`
// sources (confirmed by grepping the live corpus) that call `new URL(...)`,
// almost always to build on top of the `URLSearchParams` above (parse an
// existing query string, `.searchParams.append(...)` a few params, then
// `.toString()`/read `.href` to get the final URL back) -- exactly what's
// implemented here, not the full WHATWG URL spec (no relative-URL edge
// cases beyond a plain absolute-path or bare-relative segment, no
// username/password/port-less-host edge cases). `search`/`href` are real
// getters (not fields snapshotted at construction) because plugin code
// mutates `.searchParams` *after* constructing the `URL` and expects
// `.toString()` to reflect that -- same reasoning as `CheerioSelection`'s
// `length` getter elsewhere in this file.
function URL(input, base) {
    var url = String(input);
    if (base !== undefined && base !== null && !/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(url)) {
        var baseStr = String(base);
        var origin = (baseStr.match(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^\/]*/) || [baseStr])[0];
        url = url.charAt(0) === '/' ? origin + url : baseStr.replace(/\/[^\/]*$/, '/') + url;
    }
    var match = /^([a-zA-Z][a-zA-Z0-9+.-]*:)\/\/([^\/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/.exec(url);
    if (!match) {
        throw new TypeError('Failed to construct \'URL\': Invalid URL: ' + url);
    }
    this.protocol = match[1];
    // `match[2]` is the whole authority section (`user:pass@host:port`) --
    // split off userinfo before assigning `.host`, which real URL (and
    // `.origin` below) must NEVER include (§1.2.8.1 fix: an earlier version
    // of this constructor assigned the raw authority straight to `.host`,
    // which happened to round-trip through the old `.toString()` by
    // accident, but was wrong the moment `.username`/`.password`/`.origin`
    // needed a correctly-scoped `.host` to build on).
    var authority = match[2];
    var userinfo = '';
    var hostport = authority;
    var atIndex = authority.indexOf('@');
    if (atIndex !== -1) {
        userinfo = authority.slice(0, atIndex);
        hostport = authority.slice(atIndex + 1);
    }
    var colonIndex = userinfo.indexOf(':');
    this.username = colonIndex === -1 ? userinfo : userinfo.slice(0, colonIndex);
    this.password = colonIndex === -1 ? '' : userinfo.slice(colonIndex + 1);
    this.host = hostport;
    this.hostname = this.host.replace(/:\d+$/, '');
    var portMatch = /:(\d+)$/.exec(this.host);
    this.port = portMatch ? portMatch[1] : '';
    this.pathname = match[3] || '/';
    this.hash = match[5] || '';
    this.searchParams = new URLSearchParams(match[4] || '');
    Object.defineProperty(this, 'search', {
        get: function () {
            var s = this.searchParams.toString();
            return s ? '?' + s : '';
        },
    });
    Object.defineProperty(this, 'href', {
        get: function () { return this.toString(); },
    });
    // .origin -- protocol + host (correctly excluding userinfo, see the fix
    // above), the other real-world-common URL field besides the ones
    // already implemented. A real getter, like `search`/`href` above, so it
    // stays correct if `protocol`/`host` are ever mutated directly after
    // construction.
    Object.defineProperty(this, 'origin', {
        get: function () { return this.protocol + '//' + this.host; },
    });
}
URL.prototype.toString = function () {
    var userinfo = '';
    if (this.username || this.password) {
        userinfo = this.username + (this.password ? ':' + this.password : '') + '@';
    }
    return this.protocol + '//' + userinfo + this.host + this.pathname + this.search + this.hash;
};
// .toJSON() -- real URL's own alias for .href (MDN, §1.2.8.1), used
// implicitly by JSON.stringify(urlInstance).
URL.prototype.toJSON = function () {
    return this.href;
};
// Static URL.canParse() -- real WHATWG URL's validity-check helper (MDN,
// §1.2.8.1): attempts a real construction and reports success/failure
// instead of throwing, the same "try the real operation, catch instead of
// re-implementing the validation logic separately" approach as everywhere
// else in this shim. `URL.parse()`/`.createObjectURL()`/`.revokeObjectURL()`
// deliberately NOT added: `.parse()` is a thin, low-value wrapper over
// `new URL()` + try/catch a plugin can already write itself, and the
// Blob-URL statics have no concept to attach to in a runtime with no `Blob`
// at all.
URL.canParse = function (input, base) {
    try {
        // eslint-disable-next-line no-new
        new URL(input, base);
        return true;
    } catch (e) {
        return false;
    }
};

// Minimal `Intl` polyfill -- found missing entirely (`ReferenceError: Intl is
// not defined`) via `ranobelib.js`, which reads
// `Intl.DateTimeFormat().resolvedOptions().timeZone` (with its own `||
// 'Europe/Moscow'` fallback) to fill in a `client-time-zone` request header
// at PLUGIN CONSTRUCTION TIME, not inside `searchNovels` -- meaning this
// wasn't just a runtime search failure but a packaging-time one (the plugin
// object never finished constructing, so metadata extraction itself failed).
// A real `Intl.DateTimeFormat().resolvedOptions()` reports the *host's*
// timezone, which has no meaningful equivalent in this sandboxed runtime;
// returning an object with no `timeZone` key is enough to make the plugin's
// own fallback (`|| 'Europe/Moscow'`) kick in correctly, and is honest about
// not knowing a real timezone rather than guessing one.
var Intl = {
    DateTimeFormat: function () {
        return { resolvedOptions: function () { return {}; } };
    },
};

// The native side of `fetch` is a plain synchronous call under the hood (it
// blocks on reqwest and comes back with the full response already in hand),
// but `fetch()` itself must still return a real `Promise`: real-world plugin
// code chains `.then()` directly on `fetchApi(...)`'s return value without
// `await` (`@libs/fetch`'s own `fetchText`/`fetchApi` do exactly this, see
// below) -- a bare `Response` object has no `.then`, and that call would
// throw "not a callable function" (found by testing against a real source).
// `await`ing a non-Promise value would have been fine on its own (tslib's
// `__awaiter` wraps it in `Promise.resolve(...)` itself), but that only
// covers `await`, not direct `.then()` chaining -- so `Promise.resolve(...)`
// happens here instead, once, covering both call shapes uniformly.
function fetch(url, init) {
    init = init || {};
    var method = (init.method || 'GET').toUpperCase();
    var headersJson = JSON.stringify(init.headers || {});
    var body = init.body;
    var bodyJson;
    if (body === undefined || body === null) {
        bodyJson = JSON.stringify({ kind: 'none' });
    } else if (body instanceof FormData) {
        bodyJson = JSON.stringify({ kind: 'multipart', entries: body.__entries });
    } else {
        bodyJson = JSON.stringify({ kind: 'string', value: String(body) });
    }
    var raw = __native_fetch(String(url), method, headersJson, bodyJson);
    return Promise.resolve(new Response(raw));
}

function __libs_fetchApi(url, init) {
    init = init || {};
    var defaultHeaders = {
        'Connection': 'keep-alive',
        'Accept': '*/*',
        'Accept-Language': '*',
        'Sec-Fetch-Mode': 'cors',
        'Accept-Encoding': 'gzip, deflate',
    };
    init.headers = Object.assign({}, defaultHeaders, init.headers || {});
    return fetch(url, init);
}
// `fetchText` skips real fetchApi.ts's arrayBuffer()+TextDecoder detour
// (unimplemented here, see Response.arrayBuffer above) and uses .text()
// directly -- equivalent for UTF-8 content, which covers plugins that don't
// pass a custom `encoding` argument.
function __libs_fetchText(url, init) {
    return __libs_fetchApi(url, init).then(function (res) {
        return res.ok ? res.text() : '';
    });
}

// `require('htmlparser2')`'s `Parser` class. `.write(chunk)` only buffers --
// the actual parse (native, single-shot, see `htmlparser2.rs`) happens in
// `.end()`, which hands the whole accumulated document to
// `__native_htmlparser2_parse` and lets it dispatch onopentag/onattribute/
// ontext/onclosetag/onend directly into `this.__handlers`. Matches the real
// source's usage found in `ranobes.js`: `new Parser({onopentag, ontext,
// onclosetag}); parser.write(html); parser.end();` -- no streaming.
function HtmlParser2Parser(handlers) {
    this.__handlers = handlers || {};
    this.__chunks = [];
    // .onparserinit -- real htmlparser2 fires this once, at construction,
    // with the Parser instance itself (MDN-equivalent: fb55/htmlparser2's
    // own Handler interface, §1.2.8.1). Pure JS, no native call involved --
    // unlike every other handler here, this one fires before any parsing
    // happens at all.
    if (typeof this.__handlers.onparserinit === 'function') {
        this.__handlers.onparserinit(this);
    }
}
HtmlParser2Parser.prototype.write = function (chunk) {
    this.__chunks.push(chunk);
};
HtmlParser2Parser.prototype.end = function (chunk) {
    if (chunk !== undefined) this.__chunks.push(chunk);
    __native_htmlparser2_parse(this.__chunks.join(''), this.__handlers);
};
// .isVoidElement(name) -- real htmlparser2's `Parser` exposes this so
// `onclosetag` handlers can tell a self-closing HTML tag (never gets its own
// closing event) from one that does. A fixed, standard HTML5 list (this
// runtime's own `__native_htmlparser2_parse` already never SYNTHESIZES a
// close event for these either, so this mirrors what the native tokenizer
// actually does, not an independent guess). Found missing (`TypeError: not a
// callable function`) via `royalroad.js`'s `parseChapter`, which calls
// `parser.isVoidElement(tagName)` from inside its own `onclosetag` handler
// while reconstructing chapter HTML tag-by-tag.
var __VOID_ELEMENTS = {
    area: true, base: true, br: true, col: true, embed: true, hr: true,
    img: true, input: true, link: true, meta: true, param: true,
    source: true, track: true, wbr: true,
};
HtmlParser2Parser.prototype.isVoidElement = function (name) {
    return !!__VOID_ELEMENTS[String(name).toLowerCase()];
};
// Some plugin code parses in one shot without write()/end() -- an alias over
// the same native call.
HtmlParser2Parser.prototype.parseComplete = function (html) {
    __native_htmlparser2_parse(html, this.__handlers);
};

// `require('dayjs')`. Thin JS wrapper around the native `__native_dayjs_*`
// primitives (`dayjs.rs`) -- date parsing/formatting/arithmetic itself is
// all native (chrono), not reimplemented here.
function Dayjs(ms) {
    this.__ms = ms;
}
Dayjs.prototype.format = function (token) {
    return __native_dayjs_format(this.__ms, token === undefined ? 'YYYY-MM-DDTHH:mm:ssZ' : token);
};
Dayjs.prototype.add = function (amount, unit) {
    return new Dayjs(__native_dayjs_add(this.__ms, amount, unit));
};
Dayjs.prototype.subtract = function (amount, unit) {
    return new Dayjs(__native_dayjs_add(this.__ms, -amount, unit));
};
Dayjs.prototype.diff = function (other, unit) {
    var otherMs = other instanceof Dayjs ? other.__ms : dayjs(other).__ms;
    return __native_dayjs_diff(this.__ms, otherMs, unit === undefined ? 'millisecond' : unit);
};
Dayjs.prototype.fromNow = function () {
    return __native_dayjs_from_now(this.__ms);
};
Dayjs.prototype.valueOf = function () {
    return this.__ms;
};
Dayjs.prototype.unix = function () {
    return Math.floor(this.__ms / 1000);
};
Dayjs.prototype.toDate = function () {
    return new Date(this.__ms);
};
Dayjs.prototype.isValid = function () {
    return this.__ms !== null && !isNaN(this.__ms);
};
function dayjs(input) {
    if (input === undefined) return new Dayjs(__native_dayjs_now());
    if (input instanceof Dayjs) return new Dayjs(input.__ms);
    if (typeof input === 'number') return new Dayjs(input);
    if (input instanceof Date) return new Dayjs(input.getTime());
    return new Dayjs(__native_dayjs_parse(String(input)));
}

// Generic "loud stub" for any `require()`d module this runtime doesn't
// implement for real (lodash-es, urlencode, @libs/aes,
// protobufjs, ...): every member access resolves to a function that throws a
// specific, attributable error only once actually called -- not at
// `require()` time, and not for merely accessing/holding the reference.
function __lnreader_makeLoudStub(moduleName) {
    var target = function () {};
    return new Proxy(target, {
        get: function (_target, prop) {
            if (prop === 'then' || typeof prop === 'symbol') return undefined;
            return function () {
                throw new Error("not implemented: require('" + moduleName + "')." + String(prop));
            };
        },
        apply: function () {
            throw new Error("not implemented: require('" + moduleName + "') called as a function");
        },
    });
}

var __lnreader_modules = {
    cheerio: { load: cheerio_load },
    htmlparser2: { Parser: HtmlParser2Parser },
    dayjs: dayjs,
    '@libs/filterInputs': {
        FilterTypes: {
            TextInput: 'Text',
            Picker: 'Picker',
            CheckboxGroup: 'Checkbox',
            Switch: 'Switch',
            ExcludableCheckboxGroup: 'XCheckbox',
        },
    },
    '@libs/novelStatus': {
        NovelStatus: {
            Unknown: 'Unknown',
            Ongoing: 'Ongoing',
            Completed: 'Completed',
            Licensed: 'Licensed',
            PublishingFinished: 'Publishing Finished',
            Cancelled: 'Cancelled',
            OnHiatus: 'On Hiatus',
            STUB: 'STUB',
            Inactive: 'Inactive',
        },
    },
    // Ported verbatim from lnreader-plugins' src/lib/utils.ts (MIT).
    '@libs/isAbsoluteUrl': {
        isUrlAbsolute: function (url) {
            if (url) {
                if (url.indexOf('//') === 0) return true;
                if (url.indexOf('://') === -1) return false;
                if (url.indexOf('.') === -1) return false;
                if (url.indexOf('/') === -1) return false;
                if (url.indexOf(':') > url.indexOf('/')) return false;
                if (url.indexOf('://') < url.indexOf('.')) return true;
            }
            return false;
        },
    },
    '@libs/defaultCover': {
        defaultCover:
            'https://github.com/LNReader/lnreader-plugins/blob/main/icons/src/coverNotAvailable.jpg?raw=true',
    },
    '@libs/storage': {
        storage: {
            get: function (key) {
                var v = __native_storage_get(key);
                return v === null ? undefined : v;
            },
            // Real writes now (unlike Aidoku's own `defaults.set`, still a
            // no-op today -- see `wasm_imports/defaults.rs`): collected
            // native-side and persisted by the parent process via
            // `SourceSettings::save` once this worker call finishes
            // successfully (see `worker.rs`) -- this process has no live
            // `SourceManager` to write through directly.
            set: function (key, value) {
                __native_storage_set(key, JSON.stringify(value === undefined ? null : value));
            },
        },
        localStorage: { get: function () { return undefined; } },
        sessionStorage: { get: function () { return undefined; } },
    },
    // `fetchFile` deliberately has no entry here (unlike `fetchProto`
    // below): a full 261-source corpus grep found zero real call sites, so
    // a hand-written "not implemented" stub for it would itself be exactly
    // the kind of speculative, never-exercised code this module's own
    // `__lnreader_makeLoudStub` mechanism exists to avoid writing by hand.
    // If a future source ever calls it, member access on this object
    // returns `undefined` and the resulting "not a function" is less
    // friendly than a named stub's message, but that trade only matters the
    // day a 262nd source actually needs it -- trivial to add back then.
    '@libs/fetch': {
        fetchApi: __libs_fetchApi,
        fetchText: __libs_fetchText,
        // `fetchProto` (gRPC-Web + protobuf request/response framing) DOES
        // have one confirmed real caller -- `wuxiaworld.js`'s `parseNovel`/
        // chapter-list/`parseChapter`, all three built entirely on it
        // (`searchNovels` uses `fetchApi`/JSON instead and works today).
        // Kept unimplemented anyway: a correct protobuf message
        // encoder/decoder plus gRPC-Web's length-prefix+compression-flag
        // wire framing is a real binary-protocol subsystem, not a small
        // shim -- same risk/value call already made for `@libs/aes`
        // (primitive/protocol-level code, one confirmed caller, high cost
        // of a subtly wrong implementation vs. the value of 1/261 sources).
        fetchProto: function () {
            throw new Error("not implemented: require('@libs/fetch').fetchProto");
        },
    },
};

function require(name) {
    if (Object.prototype.hasOwnProperty.call(__lnreader_modules, name)) {
        return __lnreader_modules[name];
    }
    return __lnreader_makeLoudStub(name);
}
"#;

/// Wraps `boa_engine::Context`. Like `wasm_store.rs`'s existing `JsContext`
/// (which has the exact same underlying reason), `Context` isn't `Send`
/// (uses `Rc` internally) — this type is only ever accessed from inside
/// `Arc<Mutex<BlockingSource>>`, under the lock, never cloned or aliased
/// across threads.
/// Backing storage for `__native_storage_get`'s settings snapshot —
/// replaceable in place (see [`JsRuntime::update_settings_snapshot`])
/// without rebuilding the whole `Context`.
type SettingsSnapshot = Rc<RefCell<HashMap<String, SourceSettingValue>>>;
/// `storage.set(key, value)` calls collected during one worker process's
/// lifetime, drained per call by [`JsRuntime::take_pending_writes`].
type PendingWrites = Rc<RefCell<Vec<(String, SourceSettingValue)>>>;

pub(super) struct JsRuntime {
    context: Context,
    /// Kept so [`JsRuntime::call_plugin_method`] can clear it after every
    /// top-level operation — see [`cheerio::Store`]'s doc comment for why
    /// that matters.
    cheerio_store: cheerio::SharedStore,
    /// Backing store for `__native_storage_get`, shared with the closure
    /// registered in [`register_storage`]. The worker process is now
    /// persistent (one process per loaded source, reused across every call —
    /// see `worker.rs`'s module doc comment), so unlike a one-shot process
    /// this can't just be captured once at construction: each new call
    /// carries its own fresh settings snapshot (taken by the parent right
    /// before sending the request, same as before), which
    /// [`JsRuntime::update_settings_snapshot`] pushes in here before that
    /// call's operation runs.
    settings: SettingsSnapshot,
    /// `storage.set(key, value)` calls collected during execution (see
    /// [`register_storage`]) — this process has no live `SourceManager` to
    /// persist through directly (it's a worker subprocess, see `worker.rs`),
    /// so the parent process applies these after the call succeeds, via
    /// [`JsRuntime::take_pending_writes`].
    pending_writes: PendingWrites,
    /// Short-lived, single-entry `parseNovel()` result cache — see
    /// [`NOVEL_CACHE_TTL`]/[`JsRuntime::take_cached_novel`]/
    /// [`JsRuntime::cache_novel`]. At most one novel's result is ever held.
    novel_cache: Option<NovelCacheEntry>,
}
unsafe impl Send for JsRuntime {}

/// A settings snapshot has no `Trace`/`Finalize` impl of its own (external,
/// holds no `JsValue`s) so it needs the same empty-trace newtype treatment
/// as `net::ClientHandle` to be captured by a native function closure.
/// Wrapped in `Rc<RefCell<...>>` (not a bare `HashMap`) so
/// [`JsRuntime::update_settings_snapshot`] can replace its contents between
/// calls without rebuilding the whole `Context` — see [`JsRuntime::settings`].
struct SettingsHandle(SettingsSnapshot);
unsafe impl Trace for SettingsHandle {
    empty_trace!();
}
impl Finalize for SettingsHandle {}

/// Same empty-trace treatment for the writes collector — `Rc<RefCell<...>>`
/// of plain data, no `JsValue`s.
struct WritesHandle(PendingWrites);
unsafe impl Trace for WritesHandle {
    empty_trace!();
}
impl Finalize for WritesHandle {}

pub(super) fn eval(context: &mut Context, src: &str, label: &str) -> Result<JsValue> {
    context
        .eval(JsSource::from_bytes(src.as_bytes()))
        .map_err(|e| anyhow::anyhow!("{label}: {}", describe_js_error(&e, context)))
}

/// Best-effort human-readable rendering of a thrown JS value/error.
pub(super) fn describe_js_error(error: &boa_engine::JsError, context: &mut Context) -> String {
    error
        .to_opaque(context)
        .to_string(context)
        .ok()
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|| error.to_string())
}

fn setting_value_to_js(value: &SourceSettingValue, context: &mut Context) -> JsValue {
    match value {
        SourceSettingValue::Bool(b) => JsValue::from(*b),
        SourceSettingValue::Int(i) => JsValue::from(*i as f64),
        SourceSettingValue::Float(f) => JsValue::from(*f),
        SourceSettingValue::String(s) => JsValue::from(js_string!(s.as_str())),
        SourceSettingValue::Vec(items) => {
            let values: Vec<JsValue> = items
                .iter()
                .map(|s| JsValue::from(js_string!(s.as_str())))
                .collect();
            JsValue::from(boa_engine::object::builtins::JsArray::from_iter(
                values, context,
            ))
        }
        SourceSettingValue::Data(_) | SourceSettingValue::Null => JsValue::null(),
    }
}

/// Inverse of [`setting_value_to_js`], from the JSON `storage.set` encodes
/// its value as (see `RUNTIME_PRELUDE`'s `@libs/storage`) rather than from a
/// live `JsValue` — keeps the native surface to one simple string argument,
/// same philosophy as `net::do_fetch`'s body encoding.
fn json_to_setting_value(value: serde_json::Value) -> SourceSettingValue {
    match value {
        serde_json::Value::Bool(b) => SourceSettingValue::Bool(b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => SourceSettingValue::Int(i),
            None => SourceSettingValue::Float(n.as_f64().unwrap_or_default()),
        },
        serde_json::Value::String(s) => SourceSettingValue::String(s),
        serde_json::Value::Array(items) => SourceSettingValue::Vec(
            items
                .into_iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect(),
        ),
        serde_json::Value::Null | serde_json::Value::Object(_) => SourceSettingValue::Null,
    }
}

/// Registers `__native_storage_get`/`__native_storage_set`, backing
/// `@libs/storage`'s `storage.get()`/`storage.set()`. Reads come from a
/// snapshot (mirrors `wasm_imports/defaults.rs::get`'s use of
/// `source_settings.get(&key)` for Aidoku sources — same fallback-to-default
/// behavior, just pre-flattened since this process can't call back into a
/// live `SourceSettings`) that starts as `snapshot` and can be replaced
/// between calls via the returned handle (see [`JsRuntime::settings`]);
/// writes are collected for the parent process to apply, see
/// [`JsRuntime::pending_writes`].
fn register_storage(
    context: &mut Context,
    snapshot: HashMap<String, SourceSettingValue>,
) -> (SettingsSnapshot, PendingWrites) {
    let settings = Rc::new(RefCell::new(snapshot));
    let get_native = NativeFunction::from_copy_closure_with_captures(
        move |_this, args, handle: &SettingsHandle, context| {
            let key = args
                .get_or_undefined(0)
                .to_string(context)?
                .to_std_string_escaped();
            Ok(match handle.0.borrow().get(&key) {
                Some(value) => setting_value_to_js(value, context),
                None => JsValue::null(),
            })
        },
        SettingsHandle(settings.clone()),
    );
    let get_func = FunctionObjectBuilder::new(context.realm(), get_native)
        .name("__native_storage_get")
        .length(1)
        .build();
    context
        .global_object()
        .set(js_string!("__native_storage_get"), get_func, false, context)
        .expect("registering __native_storage_get should not fail");

    let writes = Rc::new(RefCell::new(Vec::new()));
    let set_native = NativeFunction::from_copy_closure_with_captures(
        move |_this, args, handle: &WritesHandle, context| {
            let key = args
                .get_or_undefined(0)
                .to_string(context)?
                .to_std_string_escaped();
            let value_json = args
                .get_or_undefined(1)
                .to_string(context)?
                .to_std_string_escaped();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&value_json) {
                handle
                    .0
                    .borrow_mut()
                    .push((key, json_to_setting_value(value)));
            }
            Ok(JsValue::undefined())
        },
        WritesHandle(writes.clone()),
    );
    let set_func = FunctionObjectBuilder::new(context.realm(), set_native)
        .name("__native_storage_set")
        .length(2)
        .build();
    context
        .global_object()
        .set(js_string!("__native_storage_set"), set_func, false, context)
        .expect("registering __native_storage_set should not fail");

    (settings, writes)
}

/// Builds a dedicated JS context for one LNReader source: registers every
/// native binding, evaluates the cheerio/runtime preludes, then evaluates
/// `main_js` wrapped as a CommonJS module (`Payload/main.js`'s real format —
/// `require(...)`/`module.exports.default = new Plugin()`). `settings_snapshot`
/// is a flattened read-only copy (defaults + stored overrides already
/// merged, see `SourceSettings::snapshot`) — this runs inside a persistent
/// worker subprocess (`worker.rs`, reused across many calls for the same
/// source) with no live access to `SourceManager`, so this is only the
/// INITIAL snapshot; later calls refresh it via
/// [`JsRuntime::update_settings_snapshot`] instead of rebuilding the whole
/// `Context`.
pub(super) fn new(
    settings_snapshot: HashMap<String, SourceSettingValue>,
    main_js: &str,
) -> Result<JsRuntime> {
    let mut context = Context::default();

    let cheerio_store = cheerio::register(&mut context);
    net::register(&mut context, net::build_client());
    htmlparser2::register(&mut context);
    dayjs::register(&mut context);
    let (settings, pending_writes) = register_storage(&mut context, settings_snapshot);

    eval(&mut context, CHEERIO_PRELUDE, "cheerio prelude")?;
    eval(&mut context, RUNTIME_PRELUDE, "runtime prelude")?;

    let wrapped = format!(
        "var module = {{ exports: {{}} }};\n\
         (function(require, module, exports) {{\n{main_js}\n}})(require, module, module.exports);\n\
         var __lnreader_plugin = (module.exports && module.exports.default) || module.exports;\n"
    );
    eval(&mut context, &wrapped, "Payload/main.js")?;

    let plugin = context
        .global_object()
        .get(js_string!("__lnreader_plugin"), &mut context)
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to read plugin instance: {}",
                describe_js_error(&e, &mut context)
            )
        })?;
    if plugin.as_object().is_none() {
        bail!("Payload/main.js did not export a plugin object (module.exports.default)");
    }

    Ok(JsRuntime {
        context,
        cheerio_store,
        settings,
        pending_writes,
        novel_cache: None,
    })
}

impl JsRuntime {
    pub(super) fn context(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Replaces the settings `__native_storage_get` reads from, without
    /// touching the rest of the `Context` — called before each call's
    /// operation runs (see `worker.rs::run`), since this `JsRuntime` now
    /// outlives any single call (one persistent worker process per loaded
    /// source, not one process per call).
    pub(super) fn update_settings_snapshot(
        &mut self,
        snapshot: HashMap<String, SourceSettingValue>,
    ) {
        *self.settings.borrow_mut() = snapshot;
    }

    /// Drains every `storage.set(key, value)` call made so far — the
    /// worker's caller (`worker.rs`) applies these via `SourceSettings::save`
    /// once the whole operation has succeeded.
    pub(super) fn take_pending_writes(&mut self) -> Vec<(String, SourceSettingValue)> {
        std::mem::take(&mut *self.pending_writes.borrow_mut())
    }

    /// Returns a cached `parseNovel()` result for `manga_id`, if one was
    /// stashed by [`JsRuntime::cache_novel`] within the last
    /// [`NOVEL_CACHE_TTL`] — and consumes it either way this returns
    /// `Some`, so a third call for the same novel re-fetches normally
    /// rather than serving indefinitely stale data. See
    /// [`NOVEL_CACHE_TTL`]'s doc comment for why this is deliberately
    /// one-shot, not a general cache.
    pub(super) fn take_cached_novel(&mut self, manga_id: &str) -> Option<(Manga, Vec<Chapter>)> {
        let entry = self.novel_cache.as_ref()?;
        if entry.manga_id != manga_id || entry.cached_at.elapsed() > NOVEL_CACHE_TTL {
            return None;
        }
        let entry = self.novel_cache.take()?;
        Some((entry.manga, entry.chapters))
    }

    /// Stashes a `parseNovel()` result for a short window, in case the
    /// immediately-following call is the other half of the
    /// `get_manga_details`/`get_chapter_list` pair for the same novel (see
    /// [`NOVEL_CACHE_TTL`]). Overwrites any previous entry unconditionally
    /// — at most one novel's result is ever held.
    pub(super) fn cache_novel(&mut self, manga_id: String, manga: Manga, chapters: Vec<Chapter>) {
        self.novel_cache = Some(NovelCacheEntry {
            manga_id,
            manga,
            chapters,
            cached_at: Instant::now(),
        });
    }

    /// Reads `plugin.<name>` (e.g. `id`, `name`, `filters`) without calling
    /// anything.
    pub(super) fn plugin_property(&mut self, name: &str) -> Result<JsValue> {
        let context = &mut self.context;
        let plugin = context
            .global_object()
            .get(js_string!("__lnreader_plugin"), context)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to read plugin instance: {}",
                    describe_js_error(&e, context)
                )
            })?;
        let plugin_obj = plugin
            .as_object()
            .context("plugin instance is not an object")?;
        let key: PropertyKey = js_string!(name).into();
        plugin_obj.get(key, context).map_err(|e| {
            anyhow::anyhow!(
                "failed to read plugin.{name}: {}",
                describe_js_error(&e, context)
            )
        })
    }

    /// Calls `plugin.<method_name>(args...)`, drains the microtask queue,
    /// and returns the settled value of the `Promise` the (`async`) method
    /// returned. The plugin instance is re-fetched from the global object on
    /// every call rather than cached as a `JsValue` field — holding a
    /// `JsValue`/`Gc<T>` outside of what boa's collector can trace from its
    /// own roots risks a use-after-free on collection (see
    /// `NativeFunction::from_closure`'s safety docs); the global object is
    /// always a valid root, so re-reading a property off it is safe.
    ///
    /// Clears the cheerio store ([`cheerio::Store::clear`]) after the call,
    /// on every exit path (success, JS rejection, or a Rust-level error) —
    /// nothing loaded by one top-level operation is needed by the next, and
    /// this is what actually bounds memory use across a source's lifetime
    /// (a source with a large paginated listing crashed the process
    /// otherwise — see [`cheerio::Store`]'s doc comment).
    pub(super) fn call_plugin_method(
        &mut self,
        method_name: &str,
        args: &[JsValue],
    ) -> Result<JsValue> {
        let result = self.call_plugin_method_inner(method_name, args);
        self.cheerio_store.borrow_mut().clear();
        result
    }

    fn call_plugin_method_inner(&mut self, method_name: &str, args: &[JsValue]) -> Result<JsValue> {
        let context = &mut self.context;

        let plugin = context
            .global_object()
            .get(js_string!("__lnreader_plugin"), context)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to read plugin instance: {}",
                    describe_js_error(&e, context)
                )
            })?;
        let plugin_obj = plugin
            .as_object()
            .context("plugin instance is not an object")?;

        let method_key: PropertyKey = js_string!(method_name).into();
        let method = plugin_obj.get(method_key, context).map_err(|e| {
            anyhow::anyhow!(
                "failed to read {method_name}: {}",
                describe_js_error(&e, context)
            )
        })?;
        let method_obj = method.as_object().with_context(|| {
            format!("plugin has no `{method_name}` method (Payload/main.js doesn't implement it)")
        })?;

        let result = method_obj.call(&plugin, args, context).map_err(|e| {
            anyhow::anyhow!("{method_name} threw: {}", describe_js_error(&e, context))
        })?;

        context.run_jobs().map_err(|e| {
            anyhow::anyhow!(
                "microtask queue error while running {method_name}: {}",
                describe_js_error(&e, context)
            )
        })?;

        let Some(result_obj) = result.as_object() else {
            // Not an object at all -- can't be a Promise, treat as already
            // settled (shouldn't normally happen: plugin methods are all
            // declared `async`).
            return Ok(result);
        };
        let Ok(promise) = JsPromise::from_object(result_obj) else {
            return Ok(result);
        };
        match promise.state() {
            PromiseState::Fulfilled(value) => Ok(value),
            PromiseState::Rejected(reason) => {
                let reason_str = reason
                    .to_string(context)
                    .ok()
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_else(|| "<unprintable rejection>".to_string());
                // `Error.stack` (if the rejection is a real `Error`) is far
                // more useful for pinpointing *where* inside the plugin's
                // minified code something went wrong than the bare message.
                let stack = reason
                    .as_object()
                    .and_then(|obj| obj.get(js_string!("stack"), context).ok())
                    .and_then(|v| v.to_string(context).ok())
                    .map(|s| s.to_std_string_escaped());
                match stack {
                    Some(stack) => bail!("{method_name} rejected: {reason_str}\n{stack}"),
                    None => bail!("{method_name} rejected: {reason_str}"),
                }
            }
            PromiseState::Pending => {
                bail!(
                    "{method_name} did not settle synchronously after draining the microtask queue"
                )
            }
        }
    }
}

#[cfg(test)]
mod cheerio_prelude_tests {
    use super::*;

    /// In-process (no `lnreader_worker` subprocess) harness for the three
    /// corpus-confirmed cheerio-prelude bugs fixed alongside `REFERENCE.md`
    /// §1.2's rewrite: `.remove(selector)` (mangatr.js), `.prop("outerHTML")`
    /// (novelupdates.js), and `$(htmlString)` detached-element creation
    /// (komga.js). A minimal plugin object satisfies `new()`'s "did this
    /// export something" check without needing a real `Source`/`.aix` file
    /// or network access, unlike the subprocess-based end-to-end fixtures in
    /// `mod.rs`.
    fn eval_bool(js: &str) -> bool {
        let mut runtime =
            new(std::collections::HashMap::new(), "module.exports.default = {};")
                .expect("runtime construction should not fail");
        let context = runtime.context();
        eval(context, js, "test snippet")
            .unwrap_or_else(|e| panic!("test snippet failed: {e}"))
            .as_boolean()
            .expect("test snippet should evaluate to a boolean")
    }

    #[test]
    fn remove_with_selector_only_removes_matching_children() {
        // Real cheerio: `.remove(selector)` filters the CURRENT set by
        // selector first -- only matching elements are removed, the rest
        // stay. Before the fix, the selector argument was silently ignored
        // and every child was removed.
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div id="w"><h3>t</h3><div>a</div><p>keep</p></div>');
            $('#w').children().remove('h3, div');
            $('#w').children().length === 1 && $('#w').children().first().is('p')
            "#
        ));
    }

    #[test]
    fn remove_without_selector_still_removes_everything() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div id="w"><h3>t</h3><p>p</p></div>');
            $('#w').children().remove();
            $('#w').children().length === 0
            "#
        ));
    }

    #[test]
    fn prop_outer_html_serializes_the_element() {
        // novelupdates.js builds chapter content via
        // `.map((i, el) => el.prop("outerHTML")).get().join("")` -- before
        // the fix this returned null (attr("outerHTML") never matches a
        // real attribute), joining into the literal string "null".
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<p class="x">hi</p>');
            var html = $('p').prop('outerHTML');
            html.indexOf('<p') === 0 && html.indexOf('hi') !== -1
            "#
        ));
    }

    #[test]
    fn prop_tag_name_still_works_after_outer_html_special_case() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<img src="a.png">');
            $('img').prop('tagName') === 'IMG'
            "#
        ));
    }

    #[test]
    fn dollar_with_html_string_creates_detached_element() {
        // komga.js: `a("<img />").attr({src, width, height})` then
        // `.replaceWith()`'d into the loaded document -- before the fix,
        // "<img />" was handed to the CSS selector engine as if it were a
        // selector string and threw "invalid CSS selector".
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div id="w"></div>');
            var img = $('<img />');
            img.attr({src: 'http://example.com/a.png', width: '10', height: '20'});
            img.is('img') && img.attr('src') === 'http://example.com/a.png'
                && img.attr('width') === '10'
            "#
        ));
    }

    #[test]
    fn dollar_with_plain_selector_is_unaffected() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><span class="a">x</span></div>');
            $('.a').text() === 'x'
            "#
        ));
    }

    /// Regression coverage for §1.2.3's second optimization pass: `.children`/
    /// `.remove`/`.next`/`.prev`/`.siblings`'s selector forms, and
    /// `cheerio.load`/`$(htmlString)`/`$.html()`'s combined load+select
    /// natives, were rewired onto new/changed native primitives
    /// (`__native_children_filtered`/`__native_remove_filtered`/
    /// `__native_{next,prev}_sibling_filtered`/`__native_siblings`'s new
    /// second argument/`__native_load_and_select_root`/
    /// `__native_select_and_outer_html`) purely to reduce native-call count
    /// -- these confirm observable behavior didn't move at all.
    #[test]
    fn children_selector_only_returns_matching_children() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div id="w"><h3>t</h3><div>a</div><p>keep</p></div>');
            var kids = $('#w').children('p');
            kids.length === 1 && kids.first().is('p') && kids.first().text() === 'keep'
            "#
        ));
    }

    #[test]
    fn children_no_selector_still_returns_every_child() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div id="w"><h3>t</h3><p>keep</p></div>');
            $('#w').children().length === 2
            "#
        ));
    }

    #[test]
    fn next_selector_matches_only_the_immediate_sibling() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><h3 id="h">t</h3><span>x</span><p>p</p></div>');
            // Immediate next sibling ("span") doesn't match "p" -- real
            // cheerio doesn't skip ahead to find one that does.
            $('#h').next('p').length === 0 && $('#h').next('span').length === 1
            "#
        ));
    }

    #[test]
    fn prev_selector_matches_only_the_immediate_sibling() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><h3>t</h3><span>x</span><p id="p">p</p></div>');
            $('#p').prev('h3').length === 0 && $('#p').prev('span').length === 1
            "#
        ));
    }

    #[test]
    fn siblings_selector_filters_by_selector() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><h3 id="me">t</h3><span class="a">x</span><p class="a">p</p></div>');
            var sibs = $('#me').siblings('.a');
            sibs.length === 2 && sibs.first().is('span')
            "#
        ));
    }

    #[test]
    fn siblings_no_selector_still_returns_every_sibling() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><h3 id="me">t</h3><span>x</span><p>p</p></div>');
            $('#me').siblings().length === 2
            "#
        ));
    }

    #[test]
    fn cheerio_load_static_html_serializes_whole_document() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<p>hi</p>');
            var whole = $.html();
            whole.indexOf('<html') === 0 && whole.indexOf('<p>hi</p>') !== -1
            "#
        ));
    }

    #[test]
    fn dollar_html_with_element_still_serializes_just_that_element() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><span id="s">x</span></div>');
            $.html($('#s')) === '<span id="s">x</span>'
            "#
        ));
    }

    // Regression coverage for the newly-implemented cheerio methods (G this
    // pass): .add()/.empty()/.parents()/.nextAll()/.prevAll()/.toString().

    #[test]
    fn add_selector_unions_from_document_root() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><p class="a">1</p><span class="b">2</span></div>');
            var combined = $('.a').add('.b');
            combined.length === 2 && combined.eq(0).is('.a') && combined.eq(1).is('.b')
            "#
        ));
    }

    #[test]
    fn add_selection_unions_two_already_matched_selections() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><p class="a">1</p><span class="b">2</span></div>');
            var combined = $('.a').add($('.b'));
            combined.length === 2
            "#
        ));
    }

    #[test]
    fn empty_removes_children_but_keeps_the_element() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div id="w"><p>gone</p></div>');
            $('#w').empty();
            $('#w').length === 1 && $('#w').html() === ''
            "#
        ));
    }

    #[test]
    fn to_string_serializes_outer_html_like_prop_outer_html() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<p id="x">hi</p>');
            var coerced = '' + $('#x');
            coerced === $('#x').prop('outerHTML')
            "#
        ));
    }

    #[test]
    fn parents_returns_farthest_ancestor_first() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div id="root"><section id="mid"><span id="me">x</span></section></div>');
            var p = $('#me').parents();
            // Farthest ancestor first: html > body > #root > #mid.
            p.length === 4 && p.eq(2).is('#root') && p.eq(3).is('#mid')
            "#
        ));
    }

    #[test]
    fn parents_selector_filters_the_whole_chain() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div class="x" id="root"><section id="mid"><span id="me">x</span></section></div>');
            var p = $('#me').parents('.x');
            p.length === 1 && p.eq(0).is('#root')
            "#
        ));
    }

    #[test]
    fn next_all_returns_every_following_sibling_nearest_first() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><h3 id="me">t</h3><span>a</span><p>b</p><i>c</i></div>');
            var n = $('#me').nextAll();
            n.length === 3 && n.eq(0).is('span') && n.eq(1).is('p') && n.eq(2).is('i')
            "#
        ));
    }

    #[test]
    fn prev_all_returns_every_preceding_sibling_nearest_first() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><span>a</span><p>b</p><i id="me">c</i></div>');
            var p = $('#me').prevAll();
            // Nearest first: the immediately-preceding <p>, then <span> -- NOT document order.
            p.length === 2 && p.eq(0).is('p') && p.eq(1).is('span')
            "#
        ));
    }

    #[test]
    fn next_all_selector_filters_without_stopping_the_walk() {
        assert!(eval_bool(
            r#"
            var $ = cheerio_load('<div><h3 id="me">t</h3><span class="x">a</span><p>b</p><i class="x">c</i></div>');
            var n = $('#me').nextAll('.x');
            // Unlike nextUntil(), a non-matching <p> in between doesn't stop the walk.
            n.length === 2 && n.eq(0).is('span') && n.eq(1).is('i')
            "#
        ));
    }

    // Regression coverage for the F additions: console/URL/FormData/
    // URLSearchParams/htmlparser2 oncomment.

    #[test]
    fn console_full_surface_is_callable_without_crashing() {
        assert!(eval_bool(
            r#"
            console.trace(); console.dir({}); console.table([]); console.group('x');
            console.groupCollapsed('y'); console.groupEnd(); console.assert(true, 'ok');
            console.count('c'); console.countReset('c'); console.time('t');
            console.timeLog('t'); console.timeEnd('t'); console.dirxml({});
            true
            "#
        ));
    }

    #[test]
    fn url_origin_combines_protocol_and_host() {
        assert!(eval_bool(
            r#"
            var u = new URL('https://example.com:8080/a/b?x=1');
            u.origin === 'https://example.com:8080'
            "#
        ));
    }

    #[test]
    fn form_data_set_get_has_delete_round_trip() {
        assert!(eval_bool(
            r#"
            var fd = new FormData();
            fd.append('a', '1');
            fd.append('b', '2');
            fd.set('a', '9');
            var ok = fd.get('a') === '9' && fd.has('b') === true && fd.getAll('a').length === 1;
            fd['delete']('b');
            ok && fd.has('b') === false
            "#
        ));
    }

    #[test]
    fn url_search_params_get_all_and_for_each() {
        assert!(eval_bool(
            r#"
            var sp = new URLSearchParams('tag=a&tag=b&x=1');
            var all = sp.getAll('tag');
            var seen = [];
            sp.forEach(function (value, key) { seen.push(key + '=' + value); });
            all.length === 2 && all[0] === 'a' && all[1] === 'b' && seen.length === 3
            "#
        ));
    }

    #[test]
    fn htmlparser2_dispatches_oncomment() {
        assert!(eval_bool(
            r#"
            var Parser = require('htmlparser2').Parser;
            var seen = [];
            var parser = new Parser({
                oncomment: function (text) { seen.push(text); },
            });
            parser.write('<div><!-- hello -->x</div>');
            parser.end();
            seen.length === 1 && seen[0] === ' hello '
            "#
        ));
    }

    // Regression coverage for F's spec-completeness pass: htmlparser2's
    // remaining low-cost handlers, and the Web API gaps closed alongside
    // the ones already tested above (console/URL.origin/FormData/
    // URLSearchParams basics).

    #[test]
    fn htmlparser2_dispatches_onopentagname_oncommentend_onparserinit() {
        assert!(eval_bool(
            r#"
            var Parser = require('htmlparser2').Parser;
            var events = [];
            var sawInit = false;
            var parser = new Parser({
                onparserinit: function (p) { sawInit = p instanceof Parser; },
                onopentagname: function (name) { events.push('open:' + name); },
                oncomment: function (text) { events.push('comment:' + text); },
                oncommentend: function () { events.push('commentend'); },
            });
            parser.write('<p><!--hi--></p>');
            parser.end();
            sawInit && events.join(',') === 'open:p,comment:hi,commentend'
            "#
        ));
    }

    #[test]
    fn url_username_password_port_are_parsed_and_excluded_from_host() {
        assert!(eval_bool(
            r#"
            var u = new URL('https://alice:secret@example.com:8080/a');
            u.username === 'alice' && u.password === 'secret' && u.port === '8080'
                && u.host === 'example.com:8080' && u.origin === 'https://example.com:8080'
                && u.toString() === 'https://alice:secret@example.com:8080/a'
            "#
        ));
    }

    #[test]
    fn url_to_json_and_can_parse() {
        assert!(eval_bool(
            r#"
            var u = new URL('https://example.com/a');
            u.toJSON() === u.href && URL.canParse('https://example.com') === true
                && URL.canParse('not a url') === false
            "#
        ));
    }

    #[test]
    fn url_search_params_keys_values_size_sort_iterator() {
        assert!(eval_bool(
            r#"
            var sp = new URLSearchParams('b=2&a=1');
            var keys = [];
            var it = sp.keys();
            for (var r = it.next(); !r.done; r = it.next()) keys.push(r.value);
            var values = [];
            var it2 = sp.values();
            for (var r2 = it2.next(); !r2.done; r2 = it2.next()) values.push(r2.value);
            var fromIter = [];
            for (var pair of sp) fromIter.push(pair[0]);
            sp.sort();
            var sortedFirst = sp.entries().next().value[0];
            keys.join(',') === 'b,a' && values.join(',') === '2,1' && sp.size === 2
                && fromIter.join(',') === 'b,a' && sortedFirst === 'a'
            "#
        ));
    }

    #[test]
    fn headers_iteration_methods() {
        assert!(eval_bool(
            r#"
            var h = new Headers({ 'X-A': '1', 'X-B': '2' });
            var count = 0;
            for (var pair of h) count++;
            var keys = [];
            var it = h.keys();
            for (var r = it.next(); !r.done; r = it.next()) keys.push(r.value);
            count === 2 && keys.indexOf('x-a') !== -1 && keys.indexOf('x-b') !== -1
            "#
        ));
    }

    #[test]
    fn form_data_iteration_methods() {
        assert!(eval_bool(
            r#"
            var fd = new FormData();
            fd.append('a', '1');
            fd.append('b', '2');
            var keys = [];
            var it = fd.keys();
            for (var r = it.next(); !r.done; r = it.next()) keys.push(r.value);
            var pairs = [];
            for (var e = fd.entries(), r2 = e.next(); !r2.done; r2 = e.next()) pairs.push(r2.value.join('='));
            keys.join(',') === 'a,b' && pairs.join(',') === 'a=1,b=2'
            "#
        ));
    }

    #[test]
    fn form_data_is_directly_iterable() {
        assert!(eval_bool(
            r#"
            var fd = new FormData();
            fd.append('a', '1');
            fd.append('b', '2');
            var pairs = [];
            for (var pair of fd) pairs.push(pair.join('='));
            pairs.join(',') === 'a=1,b=2'
            "#
        ));
    }

    #[test]
    fn text_decoder_encoder_expose_options_and_encoding() {
        assert!(eval_bool(
            r#"
            var dec = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
            var enc = new TextEncoder();
            dec.fatal === true && dec.ignoreBOM === true && dec.encoding === 'utf-8'
                && enc.encoding === 'utf-8'
            "#
        ));
    }

    #[test]
    fn console_extended_surface_is_callable_without_crashing() {
        assert!(eval_bool(
            r#"
            console.clear(); console.exception('x'); console.profile('p');
            console.profileEnd('p'); console.timeStamp('t');
            true
            "#
        ));
    }
}
