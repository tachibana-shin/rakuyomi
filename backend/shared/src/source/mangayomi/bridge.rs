//! Bridge registration for MangaYomi extensions.
//!
//! Mirrors the `registrer.dart` + `bridge/*.dart` surface of the MangaYomi
//! app: bridged classes (`MSource`, `MProvider`, `MManga`, `MChapter`,
//! `MPages`, `MStatus`, `Client`, `Response`, `MDocument`, `MElement`,
//! filters) plus the top-level helper functions extensions import from
//! `package:mangayomi/bridge_lib.dart`.
//!
//! All bridged objects are plain `Value` maps/lists, so nothing crosses the
//! thread boundary. The interpreter runs on a single thread; the shared
//! [`BridgeState`] and the registered classes are reachable from closures
//! through thread-locals (bridged-class closures must be `Send + Sync`, so
//! they cannot capture `Rc` values directly).

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

use d4rt_rs::{
    class::{BridgedClassDef, BridgedEnumValueData},
    stdlib::helpers,
    value::{value_display, DartMap, Value},
    InterpError,
};

use reqwest::blocking::Response as BlockingResponse;
use reqwest::header::HeaderMap;

use crate::{settings::SourceSettingValue, util::DEFAULT_USER_AGENT};

use super::html::{element_attr, MangaYomiDom};
use chrono::TimeZone;

/// Shared state captured by every bridge closure. Lives on the runtime
/// thread (never `Send` across threads, matching the interpreter).
pub(crate) struct BridgeState {
    pub client: reqwest::blocking::Client,
    pub dom: MangaYomiDom,
    /// Source preference values (`getPreferenceValue`), shared with the
    /// source so settings updates are visible to the extension.
    pub prefs: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
}

pub(crate) type StateRef = Rc<RefCell<BridgeState>>;

thread_local! {
    /// The [`BridgeState`], installed by [`register_bridge`] on the runtime
    /// thread.
    static STATE: RefCell<Option<StateRef>> = const { RefCell::new(None) };
    /// Bridged class definitions, populated at the end of [`register_bridge`].
    /// Looked up lazily from closures, which only ever run after registration
    /// completes.
    static CLASSES: RefCell<Option<HashMap<String, Arc<BridgedClassDef>>>> =
        const { RefCell::new(None) };
    /// The registered `MStatus` enum, used by `parseStatus`.
    static STATUS_ENUM: RefCell<Option<Rc<d4rt_rs::class::BridgedEnumDef>>> =
        const { RefCell::new(None) };
}

/// Accessor for [`BridgeState`] from within interpreter closures.
pub(crate) fn state() -> StateRef {
    STATE.with(|s| s.borrow().as_ref().expect("mangayomi bridge state").clone())
}

/// Looks up a registered bridged class by name.
fn class_ref(name: &str) -> Arc<BridgedClassDef> {
    CLASSES.with(|c| {
        c.borrow()
            .as_ref()
            .expect("bridged classes")
            .get(name)
            .expect("bridged class")
            .clone()
    })
}

/// Whether a bridged class with the given name is registered.
fn classes_contains(name: &str) -> bool {
    CLASSES.with(|c| {
        c.borrow()
            .as_ref()
            .map(|m| m.contains_key(name))
            .unwrap_or(false)
    })
}

/// Converts an `Option` into a `Result` for use with `?` inside closures
/// that return `Result<_, InterpError>`.
fn need<T>(opt: Option<T>, what: &str) -> Result<T, InterpError> {
    opt.ok_or_else(|| InterpError::runtime(format!("mangayomi: missing {what}")))
}

// ---------------------------------------------------------------------------
// Map helpers (native representations)
// ---------------------------------------------------------------------------

pub(crate) fn nmap() -> Value {
    Value::Map(Rc::new(RefCell::new(DartMap::new())))
}

pub(crate) fn map_get(v: &Value, key: &str) -> Value {
    match v {
        Value::Map(m) => m.borrow().get(&Value::str(key)).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

pub(crate) fn map_int(v: &Value, key: &str) -> Option<i64> {
    match map_get(v, key) {
        Value::Int(i) => Some(i),
        _ => None,
    }
}

pub(crate) fn map_set(v: &Value, key: &str, value: Value) {
    if let Value::Map(m) = v {
        m.borrow_mut().set(Value::str(key), value);
    }
}

fn as_map(v: &Value) -> Option<Rc<RefCell<DartMap>>> {
    match v {
        Value::Map(m) => Some(m.clone()),
        _ => None,
    }
}

fn as_str(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        _ => None,
    }
}

/// Converts a Dart `Map` value to a `HashMap<String, String>`.
fn str_map(v: &Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(m) = as_map(v) {
        for (k, val) in &m.borrow().entries {
            if let (Some(k), Some(v)) = (as_str(k), as_str(val)) {
                out.insert(k, v);
            }
        }
    }
    out
}

fn dart_map_of(headers: &HeaderMap) -> Value {
    let out = nmap();
    if let Value::Map(m) = &out {
        for (name, value) in headers {
            if let Ok(v) = value.to_str() {
                m.borrow_mut().set(Value::str(name.as_str()), Value::str(v));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Bridged classes
// ---------------------------------------------------------------------------

/// The result of [`register_bridge`]: everything the runtime needs to build
/// the `MSource` argument for `main()`.
pub(crate) struct BridgeClasses {
    pub msource: Arc<BridgedClassDef>,
}

/// Registers the whole MangaYomi bridge into `ctx`. Must be called on the
/// runtime thread before `execute`.
pub(crate) fn register_bridge(ctx: &mut d4rt_rs::Context, bridge_state: StateRef) -> BridgeClasses {
    STATE.with(|s| *s.borrow_mut() = Some(bridge_state));

    let mut classes: HashMap<String, Arc<BridgedClassDef>> = HashMap::new();
    let mut all: Vec<Arc<BridgedClassDef>> = Vec::new();

    let msource = register_msource(&mut classes, &mut all);
    register_mprovider(&mut classes, &mut all);
    register_mmanga(&mut classes, &mut all);
    register_mchapter(&mut classes, &mut all);
    register_mpages(&mut classes, &mut all);
    let status_enum = register_status_enum();
    register_filter_classes(&mut classes, &mut all);
    register_filter_list(&mut classes, &mut all);
    register_client_class(&mut classes, &mut all);
    register_response_class(&mut classes, &mut all);
    register_document_class(&mut classes, &mut all);
    register_element_class(&mut classes, &mut all);

    // Register everything into the context. The class map must be populated
    // before any closure runs; registration itself never invokes them.
    CLASSES.with(|c| *c.borrow_mut() = Some(classes));
    STATUS_ENUM.with(|s| *s.borrow_mut() = Some(status_enum.clone()));
    for bc in &all {
        ctx.register_bridged_class(bc.clone());
    }
    ctx.register_package_library("package:mangayomi/bridge_lib.dart", all.clone());
    ctx.register_bridged_enum(status_enum.clone());

    let st = state();
    register_top_level_functions(ctx, &st);

    BridgeClasses { msource }
}

/// Registers a bridged class whose instances are plain maps: the constructor
/// copies the named arguments into the map, and every key gets a getter (and,
/// optionally, a setter). Used by `MSource`, `MManga`, `MChapter` and the
/// filter/preference model classes.
fn register_named_class(name: &str, keys: &[&str], with_setters: bool) -> Arc<BridgedClassDef> {
    let ctor_name = name.to_string();
    let ctor_keys: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
    let mut bc = BridgedClassDef::new(name);
    bc.constructors.insert(
        "".into(),
        helpers::ctor(move |_v, _positional, named| {
            let n = nmap();
            for key in &ctor_keys {
                let v = named.get(key).cloned().unwrap_or(Value::Null);
                map_set(&n, key, v);
            }
            Ok(wrap(&class_ref(&ctor_name), n))
        }),
    );
    for key in keys {
        let getter_k = key.to_string();
        bc.getters.insert(
            key.to_string(),
            helpers::getter(move |_v, target| Ok(map_get(&target, &getter_k))),
        );
        if with_setters {
            let setter_k = key.to_string();
            bc.setters.insert(
                key.to_string(),
                helpers::setter(move |_v, target, value| {
                    map_set(&target, &setter_k, value);
                    Ok(())
                }),
            );
        }
    }
    Arc::new(bc)
}

/// Registers `MSource` (the argument of `main()`); the returned class is also
/// stored in [`BridgeClasses`].
fn register_msource(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) -> Arc<BridgedClassDef> {
    let msource = register_named_class(
        "MSource",
        &[
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
        ],
        false,
    );
    classes.insert("MSource".to_string(), msource.clone());
    all.push(msource.clone());
    msource
}

/// Registers `MProvider`, the bridged superclass of every extension class.
/// Its getters and methods are the defaults extensions inherit when they do
/// not override them.
fn register_mprovider(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mut provider = BridgedClassDef::new("MProvider");
    provider
        .constructors
        .insert("".into(), helpers::ctor(|_v, _p, _n| Ok(Value::Null)));
    provider.getters.insert(
        "supportsLatest".into(),
        helpers::getter(|_v, _t| Ok(Value::Bool(true))),
    );
    provider
        .getters
        .insert("baseUrl".into(), helpers::getter(|_v, _t| Ok(Value::Null)));
    provider
        .getters
        .insert("headers".into(), helpers::getter(|_v, _t| Ok(nmap())));
    // Defaults so extensions that do not override these still work.
    provider.methods.insert(
        "getSourcePreferences".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::List(Rc::new(RefCell::new(Vec::new()))))),
    );
    provider.methods.insert(
        "getFilterList".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::List(Rc::new(RefCell::new(Vec::new()))))),
    );
    let provider = Arc::new(provider);
    classes.insert("MProvider".to_string(), provider.clone());
    all.push(provider);
}

/// Registers `MManga`: a named-args map class with getters and setters.
fn register_mmanga(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mmanga = register_named_class(
        "MManga",
        &[
            "name",
            "artist",
            "author",
            "description",
            "genre",
            "status",
            "imageUrl",
            "link",
            "chapters",
        ],
        true,
    );
    classes.insert("MManga".to_string(), mmanga.clone());
    all.push(mmanga);
}

/// Registers `MChapter`: a named-args map class with getters and setters.
fn register_mchapter(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mchapter = register_named_class(
        "MChapter",
        &[
            "name",
            "url",
            "dateUpload",
            "scanlator",
            "isFiller",
            "thumbnailUrl",
            "description",
            "downloadSize",
            "duration",
        ],
        true,
    );
    classes.insert("MChapter".to_string(), mchapter.clone());
    all.push(mchapter);
}

/// Registers `MPages`: a `[mangaList, hasNextPage]` pair, as MangaYomi
/// models the result list.
fn register_mpages(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mut mpages = BridgedClassDef::new("MPages");
    mpages.constructors.insert(
        "".into(),
        helpers::ctor(|_v, positional, _named| {
            let native = Value::List(Rc::new(RefCell::new(vec![
                positional.first().cloned().unwrap_or(Value::Null),
                positional.get(1).cloned().unwrap_or(Value::Bool(false)),
            ])));
            Ok(wrap(&class_ref("MPages"), native))
        }),
    );
    mpages.getters.insert(
        "list".into(),
        helpers::getter(|_v, target| Ok(map_get(&target, "list"))),
    );
    mpages.getters.insert(
        "hasNextPage".into(),
        helpers::getter(|_v, target| Ok(map_get(&target, "hasNextPage"))),
    );
    mpages.setters.insert(
        "list".into(),
        helpers::setter(|_v, target, value| {
            map_set(&target, "list", value);
            Ok(())
        }),
    );
    mpages.setters.insert(
        "hasNextPage".into(),
        helpers::setter(|_v, target, value| {
            map_set(&target, "hasNextPage", value);
            Ok(())
        }),
    );
    let mpages = Arc::new(mpages);
    classes.insert("MPages".to_string(), mpages.clone());
    all.push(mpages);
}

/// Registers the `MStatus` bridged enum, also installed into the thread-local
/// used by `parseStatus`.
fn register_status_enum() -> Rc<d4rt_rs::class::BridgedEnumDef> {
    d4rt_rs::class::BridgedEnumDef::new(
        "MStatus",
        vec![
            ("ongoing", Value::Int(0)),
            ("completed", Value::Int(1)),
            ("canceled", Value::Int(2)),
            ("unknown", Value::Int(3)),
            ("onHiatus", Value::Int(4)),
            ("publishingFinished", Value::Int(5)),
        ],
    )
}

/// Registers the filter model classes (constructed positionally by
/// `getFilterList`/`getSourcePreferences`), each a map class with getters
/// and setters.
fn register_filter_classes(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    for name in [
        "TextFilter",
        "SelectFilter",
        "SelectFilterOption",
        "SortFilter",
        "SortState",
        "TriStateFilter",
        "GroupFilter",
        "HeaderFilter",
        "FilterHeader",
        "SeparatorFilter",
        "SourcePreference",
        "CheckBoxPreference",
        "SwitchPreferenceCompat",
        "ListPreference",
        "MultiSelectListPreference",
        "EditTextPreference",
    ] {
        let n = name.to_string();
        let ctor_name = n.clone();
        // Positional constructor fields, mirroring the Dart model classes
        // (`TextFilter(type, name, typeName)`, `SelectFilter(type, name,
        // state, values, typeName)`, ...). Preference classes take named
        // arguments only.
        let positional_fields: &[&str] = match name {
            "TextFilter" => &["type", "name", "typeName"],
            "SelectFilter" => &["type", "name", "state", "values", "typeName"],
            "SelectFilterOption" => &["name", "value", "typeName"],
            "SortFilter" => &["type", "name", "state", "values", "typeName"],
            "SortState" => &["index", "ascending", "typeName"],
            "TriStateFilter" => &["type", "name", "value", "typeName"],
            "GroupFilter" => &["type", "name", "state", "typeName"],
            "HeaderFilter" => &["name", "typeName"],
            "SeparatorFilter" => &["typeName"],
            _ => &[],
        };
        let mut bc = BridgedClassDef::new(name);
        bc.constructors.insert(
            "".into(),
            helpers::ctor(move |_v, positional, named| {
                let out = nmap();
                for (i, field) in positional_fields.iter().enumerate() {
                    if let Some(v) = positional.get(i) {
                        map_set(&out, field, v.clone());
                    }
                }
                for (k, v) in named {
                    map_set(&out, &k, v.clone());
                }
                Ok(wrap(&class_ref(&ctor_name), out))
            }),
        );
        for key in [
            "type",
            "name",
            "state",
            "values",
            "value",
            "typeName",
            "key",
            "options",
            "index",
            "ascending",
        ] {
            let getter_k = key.to_string();
            let setter_k = key.to_string();
            bc.getters.insert(
                key.to_string(),
                helpers::getter(move |_v, target| Ok(map_get(&target, &getter_k))),
            );
            bc.setters.insert(
                key.to_string(),
                helpers::setter(move |_v, target, value| {
                    map_set(&target, &setter_k, value);
                    Ok(())
                }),
            );
        }
        let bc = Arc::new(bc);
        classes.insert(n.clone(), bc.clone());
        all.push(bc);
    }
}

/// Registers `FilterList`, the argument wrapper of `search(query, page,
/// FilterList)`.
fn register_filter_list(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mut filter_list = BridgedClassDef::new("FilterList");
    filter_list.constructors.insert(
        "".into(),
        helpers::ctor(|_v, positional, _named| {
            let list = positional
                .first()
                .cloned()
                .unwrap_or_else(|| Value::List(Rc::new(RefCell::new(Vec::new()))));
            let native = Value::Map(Rc::new(RefCell::new({
                let mut m = DartMap::new();
                m.set(Value::str("filters"), list);
                m
            })));
            Ok(wrap(&class_ref("FilterList"), native))
        }),
    );
    filter_list.getters.insert(
        "filters".into(),
        helpers::getter(|_v, target| Ok(map_get(&target, "filters"))),
    );
    filter_list.getters.insert(
        "length".into(),
        helpers::getter(|_v, target| {
            let len = match map_get(&target, "filters") {
                Value::List(l) => l.borrow().len(),
                _ => 0,
            };
            Ok(Value::Int(len as i64))
        }),
    );
    let filter_list = Arc::new(filter_list);
    classes.insert("FilterList".to_string(), filter_list.clone());
    all.push(filter_list);
}

/// Registers `Client` (the HTTP client, see [`client_request`]).
fn register_client_class(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mut client_class = BridgedClassDef::new("Client");
    client_class.constructors.insert(
        "".into(),
        helpers::ctor(|_v, _p, _n| Ok(wrap(&class_ref("Client"), nmap()))),
    );
    for method in [
        "get",
        "post",
        "put",
        "delete",
        "head",
        "patch",
        "read",
        "readBytes",
    ] {
        let m = method.to_string();
        client_class.methods.insert(
            method.to_string(),
            helpers::method(move |_v, _target, positional, named| {
                client_request(&m, &positional, &named)
            }),
        );
    }
    client_class.methods.insert(
        "close".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::Null)),
    );
    let client_class = Arc::new(client_class);
    classes.insert("Client".to_string(), client_class.clone());
    all.push(client_class);
}

/// Registers `Response` (the result of `Client` calls): a map class whose
/// fields are exposed as getters.
fn register_response_class(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mut response_class = BridgedClassDef::new("Response");
    response_class.constructors.insert(
        "".into(),
        helpers::ctor(|_v, _p, _n| Ok(wrap(&class_ref("Response"), nmap()))),
    );
    for key in [
        "statusCode",
        "body",
        "headers",
        "isRedirect",
        "reasonPhrase",
        "contentLength",
        "bodyBytes",
        "persistentConnection",
    ] {
        let k = key.to_string();
        response_class.getters.insert(
            key.to_string(),
            helpers::getter(move |_v, target| Ok(map_get(&target, &k))),
        );
    }
    let response_class = Arc::new(response_class);
    classes.insert("Response".to_string(), response_class.clone());
    all.push(response_class);
}

/// Wraps a native value into a bridged instance of `bc`.
pub(crate) fn wrap(bc: &Arc<BridgedClassDef>, native: Value) -> Value {
    helpers::wrap(bc, native)
}

/// Wraps a Dart list of filter values into a `FilterList` bridged instance,
/// the shape MangaYomi's `search(query, page, FilterList)` expects. Items
/// that carry a `typeName`/`type_name` field are wrapped into the
/// corresponding bridged filter class, so extensions can read
/// `filter.type`/`filter.state`/`filter.values` like in the app. The wrap is
/// recursive: a filter's `values` list and `SortState`-shaped `state` maps
/// are wrapped too.
pub(crate) fn filter_list_value(filters: Value) -> Value {
    let native = nmap();
    map_set(&native, "filters", wrap_filter_deep(filters));
    wrap(&class_ref("FilterList"), native)
}

fn wrap_filter_deep(v: Value) -> Value {
    match v {
        Value::List(list) => {
            let items: Vec<Value> = list
                .borrow()
                .iter()
                .map(|i| wrap_filter_deep(i.clone()))
                .collect();
            Value::List(Rc::new(RefCell::new(items)))
        }
        Value::Map(m) => {
            let type_name = {
                let m = m.borrow();
                ["typeName", "type_name"]
                    .iter()
                    .find_map(|k| m.get(&Value::str(*k)))
                    .and_then(|v| match v {
                        Value::Str(s) => Some(s.to_string()),
                        _ => None,
                    })
            };
            let out = nmap();
            {
                let m = m.borrow();
                for (k, v) in &m.entries {
                    let key = match k {
                        Value::Str(s) => s.to_string(),
                        _ => continue,
                    };
                    let wrapped = match (&key, v) {
                        // A filter's option list or a `SortState`-shaped map
                        // must be bridged instances for `f.state`/`f.values`
                        // accesses to work inside the extension.
                        (key, Value::List(_)) if key == "values" => wrap_filter_deep(v.clone()),
                        (key, Value::Map(_)) if key == "state" => wrap_filter_deep(v.clone()),
                        _ => v.clone(),
                    };
                    map_set(&out, &key, wrapped);
                }
            }
            match type_name {
                Some(t) if classes_contains(&t) => wrap(&class_ref(&t), out),
                _ => out,
            }
        }
        other => other,
    }
}

fn register_document_class(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mut doc = BridgedClassDef::new("MDocument");
    doc.constructors
        .insert("".into(), helpers::ctor(|_v, _p, _n| Ok(Value::Null)));
    doc.getters.insert(
        "body".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            let body = need(st.dom.select_first(node, "body"), "document body")?;
            Ok(element_value(&st.dom, doc_id, body))
        }),
    );
    doc.getters.insert(
        "documentElement".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            Ok(element_value(&st.dom, doc_id, node.id))
        }),
    );
    doc.getters.insert(
        "head".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            let head = need(st.dom.select_first(node, "head"), "document head")?;
            Ok(element_value(&st.dom, doc_id, head))
        }),
    );
    doc.getters.insert(
        "outerHtml".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            Ok(Value::str(node.html().to_string()))
        }),
    );
    doc.getters.insert(
        "text".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            Ok(Value::str(node.text().trim().to_string()))
        }),
    );
    doc.getters.insert(
        "children".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            Ok(element_list(
                &st.dom,
                doc_id,
                node.children().into_iter().map(|c| c.id).collect(),
            ))
        }),
    );
    doc.getters
        .insert("parent".into(), helpers::getter(|_v, _t| Ok(Value::Null)));

    // select / selectFirst / class / tag / id helpers
    doc.methods.insert(
        "select".into(),
        helpers::method(|_v, target, positional, _named| {
            let sel = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            let ids = st.dom.select(node, &sel);
            Ok(element_list(&st.dom, doc_id, ids))
        }),
    );
    doc.methods.insert(
        "selectFirst".into(),
        helpers::method(|_v, target, positional, _named| {
            let sel = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            Ok(match st.dom.select_first(node, &sel) {
                Some(id) => element_value(&st.dom, doc_id, id),
                None => Value::Null,
            })
        }),
    );
    doc.methods.insert(
        "getElementsByClassName".into(),
        helpers::method(|_v, target, positional, _named| {
            let class = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            let ids = st.dom.select(node, &format!(".{}", class));
            Ok(element_list(&st.dom, doc_id, ids))
        }),
    );
    doc.methods.insert(
        "getElementsByTagName".into(),
        helpers::method(|_v, target, positional, _named| {
            let tag = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            let ids = st.dom.select(node, &tag);
            Ok(element_list(&st.dom, doc_id, ids))
        }),
    );
    doc.methods.insert(
        "getElementById".into(),
        helpers::method(|_v, target, positional, _named| {
            let id = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let doc_id = need(map_int(&target, "doc"), "document id")? as usize;
            let root = need(st.dom.root(doc_id), "document root")?;
            let node = need(st.dom.node(doc_id, root), "root node")?;
            Ok(match st.dom.select_first(node, &format!("#{}", id)) {
                Some(id) => element_value(&st.dom, doc_id, id),
                None => Value::Null,
            })
        }),
    );
    doc.methods.insert(
        "attr".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::Null)),
    );
    doc.methods.insert(
        "hasAttr".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::Bool(false))),
    );
    doc.methods.insert(
        "xpath".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::List(Rc::new(RefCell::new(Vec::new()))))),
    );
    doc.methods.insert(
        "xpathFirst".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::Null)),
    );

    let doc = Arc::new(doc);
    classes.insert("MDocument".to_string(), doc.clone());
    all.push(doc);
}

fn register_element_class(
    classes: &mut HashMap<String, Arc<BridgedClassDef>>,
    all: &mut Vec<Arc<BridgedClassDef>>,
) {
    let mut el = BridgedClassDef::new("MElement");
    el.constructors
        .insert("".into(), helpers::ctor(|_v, _p, _n| Ok(Value::Null)));

    el.getters.insert(
        "outerHtml".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(Value::str(node.html().to_string()))
        }),
    );
    el.getters.insert(
        "innerHtml".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(Value::str(node.inner_html().to_string()))
        }),
    );
    el.getters.insert(
        "text".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(Value::str(node.text().trim().to_string()))
        }),
    );
    el.getters.insert(
        "className".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(Value::str(
                node.attr("class").unwrap_or_default().to_string(),
            ))
        }),
    );
    el.getters.insert(
        "localName".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(Value::str(node.node_name().unwrap_or_default().to_string()))
        }),
    );
    el.getters.insert(
        "namespaceUri".into(),
        helpers::getter(|_v, _t| Ok(Value::Null)),
    );
    for (getter_name, attr_name) in [
        ("getSrc", "src"),
        ("getHref", "href"),
        ("getDataSrc", "data-src"),
    ] {
        let attr_name = attr_name.to_string();
        el.getters.insert(
            getter_name.to_string(),
            helpers::getter(move |_v, target| {
                let st = state();
                let st = st.borrow();
                let v = element_attr(&st.dom, &target, &attr_name).unwrap_or_default();
                Ok(Value::str(v))
            }),
        );
    }
    el.getters.insert(
        "getImg".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            let outer = node.html().to_string();
            let v = super::html::attr_regex(&outer, "img").unwrap_or_default();
            Ok(Value::str(v))
        }),
    );
    el.getters.insert(
        "children".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(element_list(
                &st.dom,
                doc_id,
                node.children().into_iter().map(|c| c.id).collect(),
            ))
        }),
    );
    el.getters.insert(
        "parent".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(match node.parent() {
                Some(p) => element_value(&st.dom, doc_id, p.id),
                None => Value::Null,
            })
        }),
    );
    el.getters.insert(
        "nextElementSibling".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(match node.next_element_sibling() {
                Some(n) => element_value(&st.dom, doc_id, n.id),
                None => Value::Null,
            })
        }),
    );
    el.getters.insert(
        "previousElementSibling".into(),
        helpers::getter(|_v, target| {
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(match node.prev_element_sibling() {
                Some(n) => element_value(&st.dom, doc_id, n.id),
                None => Value::Null,
            })
        }),
    );

    // methods
    el.methods.insert(
        "attr".into(),
        helpers::method(|_v, target, positional, _named| {
            let name = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let v = element_attr(&st.dom, &target, &name);
            Ok(match v {
                Some(v) if !v.is_empty() => Value::str(v),
                _ => Value::Null,
            })
        }),
    );
    el.methods.insert(
        "text".into(),
        helpers::method(|_v, target, _p, _n| {
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(Value::str(node.text().trim().to_string()))
        }),
    );
    el.methods.insert(
        "select".into(),
        helpers::method(|_v, target, positional, _named| {
            let sel = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            let ids = st.dom.select(node, &sel);
            Ok(element_list(&st.dom, doc_id, ids))
        }),
    );
    el.methods.insert(
        "selectFirst".into(),
        helpers::method(|_v, target, positional, _named| {
            let sel = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(match st.dom.select_first(node, &sel) {
                Some(id) => element_value(&st.dom, doc_id, id),
                None => Value::Null,
            })
        }),
    );
    el.methods.insert(
        "getElementsByClassName".into(),
        helpers::method(|_v, target, positional, _named| {
            let class = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            let ids = st.dom.select(node, &format!(".{}", class));
            Ok(element_list(&st.dom, doc_id, ids))
        }),
    );
    el.methods.insert(
        "getElementsByTagName".into(),
        helpers::method(|_v, target, positional, _named| {
            let tag = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let (doc_id, _, node) = need(st.dom.node_of(&target), "element")?;
            let ids = st.dom.select(node, &tag);
            Ok(element_list(&st.dom, doc_id, ids))
        }),
    );
    el.methods.insert(
        "xpath".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::Null)),
    );
    el.methods.insert(
        "xpathFirst".into(),
        helpers::method(|_v, _t, _p, _n| Ok(Value::Null)),
    );
    el.methods.insert(
        "hasAttr".into(),
        helpers::method(|_v, target, positional, _named| {
            let name = arg_str(&positional, 0)?;
            let st = state();
            let st = st.borrow();
            let (_, _, node) = need(st.dom.node_of(&target), "element")?;
            Ok(Value::Bool(node.has_attr(&name)))
        }),
    );

    let el = Arc::new(el);
    classes.insert("MElement".to_string(), el.clone());
    all.push(el);
}

/// Builds a bridged `MElement` for a node: native map
/// `{"doc": .., "node": ..}` plus `href`/`src`/`data-src` shortcuts
/// (mirroring the e2e fixtures).
fn element_value(dom: &MangaYomiDom, doc_id: usize, node_id: dom_query::NodeId) -> Value {
    let handle = dom.store_id(doc_id, node_id);
    let v = nmap();
    map_set(&v, "doc", Value::Int(doc_id as i64));
    map_set(&v, "node", Value::Int(handle as i64));
    if let Some(node) = dom.node(doc_id, handle) {
        for attr in ["href", "src", "data-src"] {
            if let Some(val) = node.attr(attr) {
                map_set(&v, attr, Value::str(val.to_string()));
            }
        }
    }
    wrap(&class_ref("MElement"), v)
}

fn element_list(dom: &MangaYomiDom, doc_id: usize, ids: Vec<dom_query::NodeId>) -> Value {
    Value::List(Rc::new(RefCell::new(
        ids.into_iter()
            .map(|id| element_value(dom, doc_id, id))
            .collect(),
    )))
}

// ---------------------------------------------------------------------------
// Client HTTP implementation
// ---------------------------------------------------------------------------

fn arg_str(positional: &[Value], i: usize) -> Result<String, InterpError> {
    match positional.get(i) {
        Some(Value::Str(s)) => Ok(s.to_string()),
        Some(Value::Bridged(b)) => match &b.borrow().native {
            Value::Str(s) => Ok(s.to_string()),
            other => Ok(value_display(other)),
        },
        _ => Err(InterpError::runtime(format!("expected string arg at {i}"))),
    }
}

fn client_request(
    method: &str,
    positional: &[Value],
    named: &HashMap<String, Value>,
) -> Result<Value, InterpError> {
    let url = arg_str(positional, 0)?;
    let headers = named.get("headers").map(str_map).unwrap_or_default();

    let mut builder = state().borrow().client.request(
        reqwest::Method::from_bytes(method.to_uppercase().as_bytes())
            .map_err(|e| InterpError::runtime(format!("invalid method {method}: {e}")))?,
        &url,
    );

    if !headers.contains_key("User-Agent") && !headers.contains_key("user-agent") {
        builder = builder.header(reqwest::header::USER_AGENT, DEFAULT_USER_AGENT);
    }
    for (k, v) in &headers {
        builder = builder.header(k.as_str(), v);
    }
    if let Some(body) = named.get("body") {
        match body {
            Value::Str(s) => {
                builder = builder.body(s.to_string());
            }
            Value::Map(_) => {
                let json = value_to_json(body);
                builder = builder
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(json.to_string());
            }
            _ => {}
        }
    }

    let resp: BlockingResponse = builder
        .send()
        .map_err(|e| InterpError::runtime(format!("request {method} {url} failed: {e}")))?;
    let status = resp.status();
    let headers_out = resp.headers().clone();
    let bytes = resp.bytes().map_err(|e| {
        InterpError::runtime(format!("failed to read response body from {url}: {e}"))
    })?;

    let native = nmap();
    map_set(&native, "statusCode", Value::Int(status.as_u16() as i64));
    map_set(
        &native,
        "reasonPhrase",
        Value::str(status.canonical_reason().unwrap_or_default()),
    );
    map_set(
        &native,
        "body",
        Value::str(String::from_utf8_lossy(&bytes).to_string()),
    );
    map_set(
        &native,
        "bodyBytes",
        Value::List(Rc::new(RefCell::new(
            bytes.iter().map(|b| Value::Int(*b as i64)).collect(),
        ))),
    );
    map_set(&native, "headers", dart_map_of(&headers_out));
    map_set(&native, "isRedirect", Value::Bool(status.is_redirection()));
    map_set(&native, "contentLength", Value::Int(bytes.len() as i64));
    map_set(&native, "persistentConnection", Value::Bool(true));

    Ok(Value::Future(Rc::new(RefCell::new(
        d4rt_rs::async_state::FutureState {
            completed: true,
            value: Some(wrap(&class_ref("Response"), native)),
            error: None,
            continuations: vec![],
        },
    ))))
}

/// Converts a Dart value to JSON (used for request bodies and for
/// serialising method results back to the source).
pub(crate) fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Double(d) => serde_json::Value::Number(
            serde_json::Number::from_f64(*d).unwrap_or(serde_json::Number::from(0)),
        ),
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::List(l) => serde_json::Value::Array(l.borrow().iter().map(value_to_json).collect()),
        Value::Map(m) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in &m.borrow().entries {
                if let Value::Str(k) = k {
                    obj.insert(k.to_string(), value_to_json(val));
                }
            }
            serde_json::Value::Object(obj)
        }
        Value::Bridged(b) => value_to_json(&b.borrow().native),
        // MStatus values become their enum index (0..5), see `parseStatus`.
        Value::BridgedEnumValue(v) => value_to_json(&v.borrow().native_value),
        _ => serde_json::Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Top-level functions
// ---------------------------------------------------------------------------

fn register_top_level_functions(ctx: &mut d4rt_rs::Context, state: &StateRef) {
    let st = state.clone();
    ctx.set(
        "parseHtml",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("parseHtml".to_string()),
            min_arity: 1,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let html = arg_str(&positional, 0)?;
                let doc_id = st.borrow_mut().dom.parse(&html);
                let native = nmap();
                map_set(&native, "doc", Value::Int(doc_id as i64));
                Ok(wrap(&class_ref("MDocument"), native))
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    ctx.set(
        "parseStatus",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("parseStatus".to_string()),
            min_arity: 2,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let text = match positional.first() {
                    Some(Value::Str(s)) => s.to_string().trim().to_lowercase(),
                    _ => String::new(),
                };
                let status_list = match positional.get(1) {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => vec![],
                };
                // The status list maps a label to a `Status` value. Like the
                // app, the first matching entry wins and the value is
                // interpreted by `status_enum_value`, not as an enum index.
                for item in &status_list {
                    if let Some(m) = as_map(item) {
                        for (k, val) in &m.borrow().entries {
                            if let (Some(k), Some(v)) = (as_str(k), as_int(val)) {
                                if k.to_lowercase().contains(&text) {
                                    return status_enum_value(&k, v);
                                }
                            }
                        }
                    }
                }
                status_enum_value("unknown", 3)
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    for (name, f) in [
        (
            "substringAfter",
            substring_after as fn(&str, &str) -> String,
        ),
        ("substringBefore", substring_before),
        ("substringAfterLast", substring_after_last),
        ("substringBeforeLast", substring_before_last),
    ] {
        let n = name.to_string();
        ctx.set(
            name,
            Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
                name: Some(n),
                min_arity: 2,
                max_arity: None,
                is_async: false,
                is_generator: false,
                closure: Box::new(move |_v, positional, _named| {
                    let text = match positional.first() {
                        Some(Value::Str(s)) => s.to_string(),
                        _ => String::new(),
                    };
                    let pat = match positional.get(1) {
                        Some(Value::Str(s)) => s.to_string(),
                        _ => String::new(),
                    };
                    Ok(Value::str(f(&text, &pat)))
                }),
                callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
            })),
        );
    }

    let st = state.clone();
    ctx.set(
        "getPreferenceValue",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("getPreferenceValue".to_string()),
            min_arity: 2,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let key = match positional.get(1) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let value = st
                    .borrow()
                    .prefs
                    .lock()
                    .unwrap()
                    .get(&key)
                    .cloned()
                    .map(setting_to_value)
                    .unwrap_or(Value::Null);
                Ok(value)
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    let st = state.clone();
    ctx.set(
        "getPrefStringValue",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("getPrefStringValue".to_string()),
            min_arity: 3,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let key = match positional.get(1) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let default = match positional.get(2) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let value = st
                    .borrow()
                    .prefs
                    .lock()
                    .unwrap()
                    .get(&key)
                    .cloned()
                    .and_then(|v| match v {
                        SourceSettingValue::String(s) => Some(s),
                        _ => None,
                    })
                    .unwrap_or(default);
                Ok(Value::str(value))
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    ctx.set(
        "parseDates",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("parseDates".to_string()),
            min_arity: 3,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let list = match positional.first() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => vec![],
                };
                let format = match positional.get(1) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let locale = match positional.get(2) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let out = parse_dates(&list, &format, &locale);
                Ok(Value::List(Rc::new(RefCell::new(
                    out.into_iter().map(Value::str).collect(),
                ))))
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    ctx.set(
        "sortMapList",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("sortMapList".to_string()),
            min_arity: 3,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let list = match positional.first() {
                    Some(Value::List(l)) => l.borrow().clone(),
                    _ => vec![],
                };
                let key = match positional.get(1) {
                    Some(Value::Str(s)) => s.to_string(),
                    _ => String::new(),
                };
                let ty = positional.get(2).and_then(as_int).unwrap_or(0);
                let mut list = list;
                list.sort_by(|a, b| {
                    let av = map_get(a, &key);
                    let bv = map_get(b, &key);
                    let cmp = match (&av, &bv) {
                        (Value::Int(a), Value::Int(b)) => a.cmp(b),
                        (Value::Str(a), Value::Str(b)) => a.cmp(b),
                        _ => std::cmp::Ordering::Equal,
                    };
                    if ty == 1 {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
                Ok(Value::List(Rc::new(RefCell::new(list))))
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    ctx.set(
        "regExp",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("regExp".to_string()),
            min_arity: 5,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let expression = arg_str(&positional, 0)?;
                let source = arg_str(&positional, 1)?;
                let replace = arg_str(&positional, 2)?;
                let ty = positional.get(3).and_then(as_int).unwrap_or(0);
                let group = positional.get(4).and_then(as_int).unwrap_or(0);
                let re = regex::Regex::new(&source)
                    .map_err(|e| InterpError::runtime(format!("regExp: {e}")))?;
                if ty == 0 {
                    return Ok(Value::str(
                        re.replace_all(&expression, replace.as_str()).to_string(),
                    ));
                }
                let value = re
                    .captures(&expression)
                    .and_then(|c| c.get(group as usize))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| expression.clone());
                Ok(Value::str(value))
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    ctx.set(
        "getMapValue",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("getMapValue".to_string()),
            min_arity: 2,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, named| {
                let source = arg_str(&positional, 0)?;
                let attr = arg_str(&positional, 1)?;
                let encode = named.get("encode").and_then(as_bool).unwrap_or(false);
                let value = serde_json::from_str::<serde_json::Value>(&source)
                    .ok()
                    .and_then(|v| v.get(&attr).cloned());
                Ok(match value {
                    Some(v) if !encode => Value::str(v.to_string()),
                    Some(v) => Value::str(serde_json::to_string(&v).unwrap_or_default()),
                    None => Value::str(""),
                })
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    ctx.set(
        "getUrlWithoutDomain",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("getUrlWithoutDomain".to_string()),
            min_arity: 1,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let url = arg_str(&positional, 0)?;
                let out = url
                    .split_once("://")
                    .map(|(_, rest)| rest)
                    .unwrap_or(&url)
                    .split_once('/')
                    .map(|(_, path)| format!("/{path}"))
                    .unwrap_or("/".to_string());
                Ok(Value::str(out))
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    ctx.set(
        "print",
        Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
            name: Some("print".to_string()),
            min_arity: 0,
            max_arity: None,
            is_async: false,
            is_generator: false,
            closure: Box::new(move |_v, positional, _named| {
                let msg = positional
                    .iter()
                    .map(value_display)
                    .collect::<Vec<_>>()
                    .join(" ");
                log::debug!("[mangayomi] {msg}");
                Ok(Value::Null)
            }),
            callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
        })),
    );

    // Video-related extractors: not supported, return empty results.
    for name in [
        "cryptoHandler",
        "encryptAESCryptoJS",
        "decryptAESCryptoJS",
        "yourUploadExtractor",
        "quarkVideosExtractor",
        "ucVideosExtractor",
        "quarkFilesExtractor",
        "ucFilesExtractor",
        "unpackJs",
        "unpackJsAndCombine",
    ] {
        let n = name.to_string();
        ctx.set(
            name,
            Value::NativeFn(Rc::new(d4rt_rs::callable::NativeFunction {
                name: Some(n),
                min_arity: 0,
                max_arity: None,
                is_async: false,
                is_generator: false,
                closure: Box::new(move |_v, _p, _n| Ok(Value::str(""))),
                callable_runtime_type: d4rt_rs::rt::FunctionRuntimeType::untyped(),
            })),
        );
    }
}

// ---------------------------------------------------------------------------
// Top-level helper implementations
// ---------------------------------------------------------------------------

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn substring_after(text: &str, pat: &str) -> String {
    match text.split_once(pat) {
        Some((_, after)) => after.to_string(),
        None => String::new(),
    }
}

fn substring_before(text: &str, pat: &str) -> String {
    match text.split_once(pat) {
        Some((before, _)) => before.to_string(),
        None => String::new(),
    }
}

fn substring_after_last(text: &str, pat: &str) -> String {
    match text.rsplit_once(pat) {
        Some((_, after)) => after.to_string(),
        None => String::new(),
    }
}

fn substring_before_last(text: &str, pat: &str) -> String {
    match text.rsplit_once(pat) {
        Some((before, _)) => before.to_string(),
        None => String::new(),
    }
}

/// MangaYomi's `parseStatus` helper: builds a `MStatus` value from the
/// registered enum. The value maps onto the app's `Status` enum exactly like
/// `MBridge.parseStatus` does (it is *not* the enum's own index):
/// `0` ongoing, `1` completed, `2` onHiatus, `3` canceled,
/// `4` publishingFinished, anything else `unknown`.
fn status_enum_value(_label: &str, value: i64) -> Result<Value, InterpError> {
    let name = match value {
        0 => "ongoing",
        1 => "completed",
        2 => "onHiatus",
        3 => "canceled",
        4 => "publishingFinished",
        _ => "unknown",
    };
    let status_def = STATUS_ENUM.with(|s| s.borrow().as_ref().expect("status enum").clone());
    let values: Vec<BridgedEnumValueData> = status_def
        .values
        .borrow()
        .iter()
        .map(|v| v.as_ref().clone())
        .collect();
    let data = values
        .iter()
        .find(|v| v.name == name)
        .cloned()
        .ok_or_else(|| InterpError::runtime(format!("parseStatus: missing {name}")))?;
    Ok(Value::BridgedEnumValue(Rc::new(RefCell::new(data))))
}

/// Simplified `parseDates`: parses each date string with the given chrono
/// format (with a couple of fallbacks) and returns epoch milliseconds as
/// strings; failures fall back to the current time, like the app.
fn parse_dates(list: &[Value], format: &str, _locale: &str) -> Vec<String> {
    let now = chrono::Utc::now().timestamp_millis().to_string();
    list.iter()
        .map(|v| {
            let text = match v {
                Value::Str(s) => s.to_string().trim().to_string(),
                _ => String::new(),
            };
            if text.is_empty() {
                return now.clone();
            }
            let format = if format.is_empty() {
                "%Y-%m-%d"
            } else {
                format
            };
            let parsed = chrono::NaiveDate::parse_from_str(&text, format)
                .ok()
                .and_then(|d| {
                    chrono::Utc
                        .from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
                        .single()
                })
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc3339(&text)
                        .ok()
                        .map(|d| d.with_timezone(&chrono::Utc))
                });
            match parsed {
                Some(t) => t.timestamp_millis().to_string(),
                None => now.clone(),
            }
        })
        .collect()
}

fn setting_to_value(value: SourceSettingValue) -> Value {
    match value {
        SourceSettingValue::String(s) => Value::str(s),
        SourceSettingValue::Int(i) => Value::Int(i),
        SourceSettingValue::Float(f) => Value::Double(f),
        SourceSettingValue::Bool(b) => Value::Bool(b),
        SourceSettingValue::Vec(v) => Value::List(Rc::new(RefCell::new(
            v.into_iter().map(Value::str).collect(),
        ))),
        _ => Value::Null,
    }
}
