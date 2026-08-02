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

use super::{cheerio, dayjs, htmlparser2, net};

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
    arr.each = function (callback) {
        for (var i = 0; i < this.length; i++) callback(i, this[i]);
        return this;
    };
    arr.map = function (callback) {
        var out = [];
        for (var i = 0; i < this.length; i++) out.push(callback(i, this[i]));
        return toChain(out);
    };
    // .get(index?) -- without an argument, returns the array itself (mirrors
    // real cheerio converting a collection to a plain JS array); with an
    // index, returns that single element.
    arr.get = function (index) {
        if (index === undefined) return this;
        return this[index];
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
                return null;
            }
            var first = this[0];
            return first[name].apply(first, arguments);
        };
    });
    var mutateMethods = ['remove', 'addClass', 'removeClass', 'removeAttr'];
    mutateMethods.forEach(function (name) {
        arr[name] = function () {
            var args = arguments;
            this.each(function (_i, el) { el[name].apply(el, args); });
            return this;
        };
    });
    return arr;
}

function CheerioSelection(id) {
    this.__id = id;
    Object.defineProperty(this, 'length', {
        get: function () { return __native_each_count(this.__id); },
    });
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
            var newText = newTextOrFn(i, el.text());
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
// .children(selector?) -- dom_query's children() takes no selector, so the
// filter happens on the JS side, same pattern as .siblings(selector).
CheerioSelection.prototype.children = function (selector) {
    var base = new CheerioSelection(__native_children(this.__id));
    if (!selector) return base;
    return toChain(base.toArray().filter(function (el) { return el.is(selector); }));
};
// .next(selector?) -- dom_query's next_sibling() takes no selector -- filter
// on the JS side, same pattern as children(selector). If the immediate next
// sibling doesn't match the selector, this returns an empty selection rather
// than searching further (matches real cheerio: .next(sel) doesn't "skip"
// multiple siblings).
CheerioSelection.prototype.next = function (selector) {
    var n = new CheerioSelection(__native_next_sibling(this.__id));
    if (!selector) return n;
    if (n.exists() && n.is(selector)) return n;
    return toChain([]);
};
CheerioSelection.prototype.nextSibling = function () {
    return new CheerioSelection(__native_next_sibling(this.__id));
};
CheerioSelection.prototype.prevSibling = function () {
    return new CheerioSelection(__native_prev_sibling(this.__id));
};
CheerioSelection.prototype.remove = function () {
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
// .nodeType() -- "text" | "tag" | "comment" | "other". Needed to
// distinguish .contents() items, same idea as `element.type` in real
// LNReader plugin code.
CheerioSelection.prototype.nodeType = function () {
    return __native_node_type(this.__id);
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
CheerioSelection.prototype.siblings = function (selector) {
    var handles = __native_siblings(this.__id);
    var kids = [];
    for (var i = 0; i < handles.length; i++) {
        kids.push(new CheerioSelection(handles[i]));
    }
    if (selector) {
        kids = kids.filter(function (el) { return el.is(selector); });
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
        callback(i, new CheerioSelection(handles[i]));
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
    this.each(function (i, el) { out.push(callback(i, el)); });
    return toChain(out);
};
CheerioSelection.prototype.toArray = function () {
    return this.map(function (_i, el) { return el; });
};
CheerioSelection.prototype.slice = function (start, end) {
    return toChain(this.toArray().slice(start, end));
};
// _filterBy(predicate) -- shared helper for filter/not/has: all three did
// the exact same each()+collect loop, only the test changed.
function _filterBy(selection, predicate) {
    var out = [];
    selection.each(function (i, el) {
        if (predicate(i, el)) out.push(el);
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
    var docId = __native_load(html);
    // `$(el)` -- re-wrapping an already-loaded element (typically the
    // second argument of an `.each((i, el) => ...)` callback) is a real,
    // common cheerio idiom, distinct from `$('selector')`. Found missing
    // while testing against real sources (lnori.ts's getLibraryNovels does
    // `n(e).attr(...)` inside `.each()`, `n` being the loaded `$`).
    var $ = function (selectorOrElement) {
        if (selectorOrElement instanceof CheerioSelection) {
            return selectorOrElement;
        }
        return new CheerioSelection(__native_select_root(docId, selectorOrElement));
    };
    // $.html(el) -- static form found in real plugin code, distinct from
    // $(el).html(). In real cheerio, $.html(el) serializes the passed
    // element (equivalent to el.outerHtml() here).
    $.html = function (el) {
        return el.outerHtml();
    };
    return $;
}
"#;

/// The `require()` shim and small `fetch`/`Response`/`FormData`/
/// `URLSearchParams` polyfills LNReader plugins are compiled against.
/// `@libs/fetch`'s `fetchApi`/`fetchText` bodies are ported near-verbatim
/// from `lnreader-plugins`' `src/lib/fetch.ts` (MIT), built on top of the
/// native `fetch` below instead of a real browser one.
const RUNTIME_PRELUDE: &str = r#"
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

function FormData() {
    this.__entries = [];
}
FormData.prototype.append = function (key, value) {
    this.__entries.push([String(key), String(value)]);
};

function URLSearchParams(init) {
    this.__pairs = [];
    if (init) {
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
URLSearchParams.prototype.toString = function () {
    return this.__pairs
        .map(function (pair) {
            return encodeURIComponent(pair[0]) + '=' + encodeURIComponent(pair[1]);
        })
        .join('&');
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
}
HtmlParser2Parser.prototype.write = function (chunk) {
    this.__chunks.push(chunk);
};
HtmlParser2Parser.prototype.end = function (chunk) {
    if (chunk !== undefined) this.__chunks.push(chunk);
    __native_htmlparser2_parse(this.__chunks.join(''), this.__handlers);
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
    '@libs/fetch': {
        fetchApi: __libs_fetchApi,
        fetchText: __libs_fetchText,
        fetchFile: function () {
            throw new Error("not implemented: require('@libs/fetch').fetchFile");
        },
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

fn eval(context: &mut Context, src: &str, label: &str) -> Result<JsValue> {
    context
        .eval(JsSource::from_bytes(src.as_bytes()))
        .map_err(|e| anyhow::anyhow!("{label}: {}", describe_js_error(&e, context)))
}

/// Best-effort human-readable rendering of a thrown JS value/error.
fn describe_js_error(error: &boa_engine::JsError, context: &mut Context) -> String {
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
