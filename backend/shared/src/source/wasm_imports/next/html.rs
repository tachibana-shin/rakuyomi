use anyhow::{anyhow, Context, Result};

use scraper::{Html as CHtml, Selector};
use url::Url;
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::source::wasm_store::{HTMLElement, Html, Value, WasmStore};

pub fn register_html_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "html", "parse", parse)?; // OK
    register_wasm_function!(linker, "html", "parse_fragment", parse_fragment)?; // OK
    register_wasm_function!(linker, "html", "escape", escape)?;
    register_wasm_function!(linker, "html", "unescape", unescape)?;
    register_wasm_function!(linker, "html", "select", select)?; // OK
    register_wasm_function!(linker, "html", "select_first", select_first)?; // OK
    register_wasm_function!(linker, "html", "attr", attr)?; // OK
    register_wasm_function!(linker, "html", "text", text)?; // OK
    register_wasm_function!(linker, "html", "untrimmed_text", untrimmed_text)?;
    register_wasm_function!(linker, "html", "html", html)?; // OK
    register_wasm_function!(linker, "html", "outer_html", outer_html)?;
    register_wasm_function!(linker, "html", "set_text", set_text)?;
    register_wasm_function!(linker, "html", "set_html", set_html)?;
    register_wasm_function!(linker, "html", "prepend", prepend)?;
    register_wasm_function!(linker, "html", "append", append)?;
    register_wasm_function!(linker, "html", "parent", parent)?;
    register_wasm_function!(linker, "html", "children", children)?; // OK
    register_wasm_function!(linker, "html", "siblings", siblings)?;
    register_wasm_function!(linker, "html", "next", next)?;
    register_wasm_function!(linker, "html", "previous", previous)?;
    register_wasm_function!(linker, "html", "base_uri", base_uri)?;
    register_wasm_function!(linker, "html", "own_text", own_text)?;
    register_wasm_function!(linker, "html", "data", data)?; // OK
    register_wasm_function!(linker, "html", "id", id)?;
    register_wasm_function!(linker, "html", "tag_name", tag_name)?;
    register_wasm_function!(linker, "html", "class_name", class_name)?;
    register_wasm_function!(linker, "html", "has_class", has_class)?;
    register_wasm_function!(linker, "html", "has_attr", has_attr)?;
    register_wasm_function!(linker, "html", "first", first)?;
    register_wasm_function!(linker, "html", "last", last)?;
    register_wasm_function!(linker, "html", "get", get)?; // OK: fixed
    register_wasm_function!(linker, "html", "size", size)?; // OK

    Ok(())
}

#[allow(dead_code)]
enum ResultContext {
    // Success,
    InvalidDescriptor,
    InvalidString,
    // InvalidHtml,
    InvalidQuery,
    #[allow(clippy::enum_variant_names)]
    NoResult,
    // SwiftSoupError,
}

impl From<ResultContext> for i32 {
    fn from(result: ResultContext) -> Self {
        match result {
            // Result::Success => 0,
            ResultContext::InvalidDescriptor => -1,
            ResultContext::InvalidString => -2,
            ResultContext::InvalidQuery => -4,
            ResultContext::NoResult => -5,
            // Result::SwiftSoupError => -6,
        }
    }
}
impl From<ResultContext> for Result<i32> {
    fn from(result: ResultContext) -> Self {
        match result {
            // Result::Success => 0,
            ResultContext::InvalidDescriptor => Ok(-1),
            ResultContext::InvalidString => Ok(-2),
            ResultContext::InvalidQuery => Ok(-4),
            ResultContext::NoResult => Ok(-5),
            // Result::SwiftSoupError => -6,
        }
    }
}
type FFIResult = Result<i32>;

#[aidoku_wasm_function]
fn parse(
    mut caller: Caller<'_, WasmStore>,
    data: Option<String>,
    base_url: Option<String>,
) -> FFIResult {
    let store = caller.data_mut();

    let Some(text) = data else {
        return ResultContext::InvalidString.into();
    };
    let Some(base_url_string) = base_url else {
        return ResultContext::InvalidString.into();
    };
    let base_uri = Url::parse(&base_url_string).ok();
    let document = CHtml::parse_document(&text);
    let node_id = document.root_element().id();

    Ok(store.store_std_value(
        Value::from(vec![HTMLElement {
            document: Html::from(document).into(),
            node_id,
            base_uri,
        }])
        .into(),
        None,
    ) as i32)
}

#[aidoku_wasm_function]
pub fn parse_fragment(
    mut caller: Caller<'_, WasmStore>,
    data: Option<String>,
    uri: Option<String>,
) -> FFIResult {
    let store = caller.data_mut();

    let Some(text) = data else {
        return ResultContext::InvalidString.into();
    };
    let Some(base_url_string) = uri else {
        return ResultContext::InvalidString.into();
    };

    let base_uri = Url::parse(&base_url_string).ok();
    let document = CHtml::parse_fragment(&text);
    let node_id = document.root_element().id();

    Ok(store.store_std_value(
        Value::from(vec![HTMLElement {
            document: Html::from(document).into(),
            node_id,
            base_uri,
        }])
        .into(),
        None,
    ) as i32)
}

#[aidoku_wasm_function]
fn escape(mut caller: Caller<'_, WasmStore>, text: Option<String>) -> Result<i32> {
    let Some(text) = text else {
        return ResultContext::InvalidString.into();
    };
    let escaped = html_escape::encode_safe(&text).to_string();

    let store = caller.data_mut();
    Ok(store.store_std_value(Value::from(escaped).into(), None) as i32)
}
#[aidoku_wasm_function]
fn unescape(mut caller: Caller<'_, WasmStore>, text: Option<String>) -> Result<i32> {
    let Some(text) = text else {
        return ResultContext::InvalidString.into();
    };
    let escaped = html_escape::decode_html_entities(&text).to_string();

    let store = caller.data_mut();
    Ok(store.store_std_value(Value::from(escaped).into(), None) as i32)
}
#[aidoku_wasm_function]
fn select(caller: Caller<'_, WasmStore>, ptr: i32, selector: Option<String>) -> Result<i32> {
    Ok(
        crate::source::wasm_imports::html::select(caller, ptr, selector)
            .ok()
            .unwrap_or(ResultContext::InvalidQuery.into()),
    )
}

#[aidoku_wasm_function]
fn select_first(
    mut caller: Caller<'_, WasmStore>,
    descriptor: i32,
    selector: Option<String>,
) -> Result<i32> {
    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor as usize) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    let Some(selector) = selector else {
        return ResultContext::InvalidQuery.into();
    };
    let Some(selector) = Selector::parse(&selector)
        .map_err(|e| anyhow!(e.to_string()))
        .with_context(|| format!("couldn't parse selector '{}'", selector))
        .ok()
    else {
        return ResultContext::InvalidQuery.into();
    };
    let selected_element = html_elements.iter().find_map(|el| {
        el.element_ref()
            .select(&selector)
            .next()
            .map(|selected_ref| HTMLElement {
                document: el.document.clone(),
                node_id: selected_ref.id(),
                base_uri: el.base_uri.clone(),
            })
    });

    let Some(selected_element) = selected_element else {
        return ResultContext::NoResult.into();
    };

    Ok(wasm_store.store_std_value(
        Value::from(vec![selected_element]).into(),
        Some(descriptor as usize),
    ) as i32)
}
#[aidoku_wasm_function]
fn attr(caller: Caller<'_, WasmStore>, ptr: i32, selector: Option<String>) -> Result<i32> {
    crate::source::wasm_imports::html::attr(caller, ptr, selector)
}
#[aidoku_wasm_function]
fn text(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::text(caller, ptr)
}
#[aidoku_wasm_function]
fn untrimmed_text(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::untrimmed_text(caller, ptr)
}

#[aidoku_wasm_function]
fn html(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::html(caller, ptr)
}
#[aidoku_wasm_function]
fn outer_html(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::outer_html(caller, ptr)
}
#[aidoku_wasm_function]
pub fn set_text(caller: Caller<'_, WasmStore>, ptr: i32, text: Option<String>) -> FFIResult {
    crate::source::wasm_imports::html::set_text(caller, ptr, text)
}
#[aidoku_wasm_function]
fn set_html(caller: Caller<'_, WasmStore>, ptr: i32, text: Option<String>) -> FFIResult {
    crate::source::wasm_imports::html::set_html(caller, ptr, text)
}
#[aidoku_wasm_function]
fn prepend(caller: Caller<'_, WasmStore>, ptr: i32, text: Option<String>) -> FFIResult {
    crate::source::wasm_imports::html::prepend(caller, ptr, text)
}
#[aidoku_wasm_function]
fn append(caller: Caller<'_, WasmStore>, ptr: i32, text: Option<String>) -> FFIResult {
    crate::source::wasm_imports::html::append(caller, ptr, text)
}
#[aidoku_wasm_function]
fn parent(mut caller: Caller<'_, WasmStore>, ptr: i32) -> FFIResult {
    let Some(descriptor): Option<usize> = ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    let selected_elements: Vec<_> = html_elements
        .iter()
        .flat_map(|element| {
            element
                .element_ref()
                .parent()
                .map(|selected_element_ref| HTMLElement {
                    document: element.document.clone(),
                    node_id: selected_element_ref.id(),
                    base_uri: element.base_uri.clone(),
                })
        })
        .collect();

    Ok(wasm_store.store_std_value(Value::from(selected_elements).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn children(mut caller: Caller<'_, WasmStore>, ptr: i32) -> FFIResult {
    let Some(descriptor): Option<usize> = ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    let selected_elements: Vec<_> = html_elements
        .iter()
        .flat_map(|element| {
            element
                .element_ref()
                .children()
                .map(|selected_element_ref| HTMLElement {
                    document: element.document.clone(),
                    node_id: selected_element_ref.id(),
                    base_uri: element.base_uri.clone(),
                })
        })
        .collect();

    Ok(wasm_store.store_std_value(Value::from(selected_elements).into(), Some(descriptor)) as i32)
}
#[aidoku_wasm_function]
fn siblings(mut caller: Caller<'_, WasmStore>, ptr: i32) -> FFIResult {
    let Some(descriptor): Option<usize> = ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    let selected_elements: Vec<_> = html_elements
        .iter()
        .flat_map(|element| {
            element
                .element_ref()
                .next_siblings()
                .map(|selected_element_ref| HTMLElement {
                    document: element.document.clone(),
                    node_id: selected_element_ref.id(),
                    base_uri: element.base_uri.clone(),
                })
        })
        .collect();

    Ok(wasm_store.store_std_value(Value::from(selected_elements).into(), Some(descriptor)) as i32)
}
#[aidoku_wasm_function]
fn next(mut caller: Caller<'_, WasmStore>, ptr: i32) -> FFIResult {
    let Some(descriptor): Option<usize> = ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    let selected_elements: Vec<_> = html_elements
        .iter()
        .flat_map(|element| {
            element
                .element_ref()
                .next_sibling()
                .map(|selected_element_ref| HTMLElement {
                    document: element.document.clone(),
                    node_id: selected_element_ref.id(),
                    base_uri: element.base_uri.clone(),
                })
        })
        .collect();

    Ok(wasm_store.store_std_value(Value::from(selected_elements).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn previous(mut caller: Caller<'_, WasmStore>, ptr: i32) -> FFIResult {
    let Some(descriptor): Option<usize> = ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    let selected_elements: Vec<_> = html_elements
        .iter()
        .flat_map(|element| {
            element
                .element_ref()
                .prev_sibling()
                .map(|selected_element_ref| HTMLElement {
                    document: element.document.clone(),
                    node_id: selected_element_ref.id(),
                    base_uri: element.base_uri.clone(),
                })
        })
        .collect();

    Ok(wasm_store.store_std_value(Value::from(selected_elements).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn base_uri(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::base_uri(caller, ptr)
}

#[aidoku_wasm_function]
fn own_text(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::own_text(caller, ptr)
}

#[aidoku_wasm_function]
fn data(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::data(caller, ptr)
}

#[aidoku_wasm_function]
fn id(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::id(caller, ptr)
}

#[aidoku_wasm_function]
fn tag_name(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::tag_name(caller, ptr)
}

#[aidoku_wasm_function]
fn class_name(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::class_name(caller, ptr)
}
#[aidoku_wasm_function]
fn has_class(caller: Caller<'_, WasmStore>, ptr: i32, attr_name: Option<String>) -> Result<i32> {
    crate::source::wasm_imports::html::has_class(caller, ptr, attr_name)
}

#[aidoku_wasm_function]
fn has_attr(caller: Caller<'_, WasmStore>, ptr: i32, attr_name: Option<String>) -> Result<i32> {
    crate::source::wasm_imports::html::has_attr(caller, ptr, attr_name)
}

#[aidoku_wasm_function]
fn first(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::first(caller, ptr)
}

#[aidoku_wasm_function]
fn last(caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    crate::source::wasm_imports::html::last(caller, ptr)
}

//
#[aidoku_wasm_function]
fn get(mut caller: Caller<'_, WasmStore>, ptr: i32, index: i32) -> FFIResult {
    let Some(descriptor): Option<usize> = ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    let len = html_elements.len() as i32;
    if len == 0 {
        return ResultContext::NoResult.into();
    }

    let element = html_elements.get(index as usize);
    if let Some(element) = element {
        return Ok(wasm_store
            .store_std_value(Value::from(vec![element.clone()]).into(), Some(descriptor))
            as i32);
    } else {
        return ResultContext::NoResult.into();
    }
}

#[aidoku_wasm_function]
fn size(mut caller: Caller<'_, WasmStore>, ptr: i32) -> Result<i32> {
    let Some(descriptor): Option<usize> = ptr.try_into().ok() else {
        return ResultContext::InvalidDescriptor.into();
    };

    let wasm_store = caller.data_mut();
    let Some(std_value) = wasm_store.get_std_value(descriptor) else {
        return ResultContext::InvalidDescriptor.into();
    };
    let Some(html_elements) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }) else {
        return ResultContext::NoResult.into();
    };

    Ok(wasm_store.store_std_value(
        Value::from(html_elements.len() as i64).into(),
        Some(descriptor),
    ) as i32)
}
