use anyhow::{anyhow, Context, Result};

use dom_query::Document;
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::source::html_element::HTMLElement;
use crate::source::wasm_store::{Value, WasmStore};

pub fn register_html_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "html", "parse", parse)?;
    register_wasm_function!(linker, "html", "parse_fragment", parse_fragment)?;
    register_wasm_function!(linker, "html", "parse_with_uri", parse_with_uri)?;
    register_wasm_function!(
        linker,
        "html",
        "parse_fragment_with_uri",
        parse_fragment_with_uri
    )?;

    register_wasm_function!(linker, "html", "select", select)?;
    register_wasm_function!(linker, "html", "attr", attr)?;

    register_wasm_function!(linker, "html", "set_text", set_text)?;
    register_wasm_function!(linker, "html", "set_html", set_html)?;
    register_wasm_function!(linker, "html", "prepend", prepend)?;
    register_wasm_function!(linker, "html", "append", append)?;
    register_wasm_function!(linker, "html", "first", first)?;
    register_wasm_function!(linker, "html", "last", last)?;
    register_wasm_function!(linker, "html", "next", next)?;
    register_wasm_function!(linker, "html", "previous", previous)?;

    register_wasm_function!(linker, "html", "base_uri", base_uri)?;
    register_wasm_function!(linker, "html", "body", body)?;
    register_wasm_function!(linker, "html", "text", text)?;
    register_wasm_function!(linker, "html", "untrimmed_text", untrimmed_text)?;
    register_wasm_function!(linker, "html", "own_text", own_text)?;

    register_wasm_function!(linker, "html", "data", data)?;
    register_wasm_function!(linker, "html", "array", array)?;
    register_wasm_function!(linker, "html", "html", html)?;
    register_wasm_function!(linker, "html", "outer_html", outer_html)?;

    register_wasm_function!(linker, "html", "escape", escape)?;
    register_wasm_function!(linker, "html", "unescape", unescape)?;
    register_wasm_function!(linker, "html", "id", id)?;
    register_wasm_function!(linker, "html", "tag_name", tag_name)?;
    register_wasm_function!(linker, "html", "class_name", class_name)?;
    register_wasm_function!(linker, "html", "has_class", has_class)?;
    register_wasm_function!(linker, "html", "has_attr", has_attr)?;

    Ok(())
}

#[aidoku_wasm_function]
fn parse(caller: Caller<'_, WasmStore>, data: Option<String>) -> Result<i32> {
    parse_with_uri(caller, data, None)
}

#[aidoku_wasm_function]
fn parse_fragment(caller: Caller<'_, WasmStore>, data: Option<String>) -> Result<i32> {
    parse_fragment_with_uri(caller, data, None)
}

#[aidoku_wasm_function]
fn parse_with_uri(
    mut caller: Caller<'_, WasmStore>,
    data: Option<String>,
    base_uri: Option<String>,
) -> Result<i32> {
    let store = caller.data_mut();
    let document = Document::from(data.context("data is required for parse_with_uri")?);

    let node_id = document.root().id;
    let html_element = HTMLElement {
        document: store.set_html(document),
        node_id,
        base_uri,
    };

    Ok(store.store_std_value(Value::from(vec![html_element]).into(), None) as i32)
}

#[aidoku_wasm_function]
fn parse_fragment_with_uri(
    mut caller: Caller<'_, WasmStore>,
    data: Option<String>,
    base_uri: Option<String>,
) -> Result<i32> {
    let store = caller.data_mut();
    let fragment =
        Document::fragment(data.context("data is required for parse_fragment_with_uri")?);
    let node_id = fragment.root().id;
    let html_element = HTMLElement {
        document: store.set_html(fragment),
        node_id,
        base_uri,
    };

    Ok(store.store_std_value(Value::from(vec![html_element]).into(), None) as i32)
}

#[aidoku_wasm_function]
pub fn select(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    selector: Option<String>,
) -> Result<i32> {
    let descriptor: usize = descriptor_i32
        .try_into()
        .context("couldn't convert descriptor to i32")?;
    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .ok_or_else(|| anyhow!("failed to get value from store"))?;
    let html_elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    // TODO NAMING IS PURE GARBAGE
    let selector = selector.context("selector is required for select function")?;
    let selected_elements: Vec<_> = html_elements
        .iter()
        .map(|element| element.select_soup(wasm_store, &selector))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .flatten()
        .collect();

    Ok(wasm_store.store_std_value(Value::from(selected_elements).into(), Some(descriptor)) as i32)
}

#[derive(Debug)]
pub struct AttributeNotFound;

impl std::fmt::Display for AttributeNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "attribute not found")
    }
}

impl std::error::Error for AttributeNotFound {}
#[aidoku_wasm_function]
pub fn attr(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    selector: Option<String>,
) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;
    let selector = selector.context("selector is required for attr function")?;

    let has_abs_prefix = selector.starts_with("abs:");
    let selector = if has_abs_prefix {
        selector.strip_prefix("abs:").unwrap().to_owned()
    } else {
        selector
    };

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get standard value")?;
    let elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    let attr = elements
        .iter()
        .find_map(|element| element.attr(wasm_store, &selector))
        .context(AttributeNotFound)?
        .to_owned();

    let attr = if has_abs_prefix {
        let base_uri = elements
            .iter()
            .find_map(|element| element.base_uri.as_ref())
            .map_or("", |v| v);

        let absolute_url = url::Url::parse(base_uri)
            .unwrap_or_else(|_| url::Url::parse("file:///").unwrap())
            .join(&attr)
            .context("failed to join base URI and attribute URL")?;

        absolute_url.to_string()
    } else {
        attr
    };

    Ok(wasm_store.store_std_value(Value::from(attr).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn set_text(
    mut caller: Caller<'_, WasmStore>,
    descriptor: i32,
    text: Option<String>,
) -> Result<i32> {
    let text = text.unwrap_or_default();

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor as usize)
        .context("failed to get standard value")?;

    let Some(first_element) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => elements.first(),
        _ => None,
    }) else {
        return Ok(-1);
    };

    first_element.set_text(wasm_store, &text);

    Ok(0)
}

#[aidoku_wasm_function]
pub fn set_html(
    mut caller: Caller<'_, WasmStore>,
    descriptor: i32,
    html: Option<String>,
) -> Result<i32> {
    let html = html.unwrap_or_default();

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor as usize)
        .context("failed to get standard value")?;

    let Some(first_element) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => elements.first(),
        _ => None,
    }) else {
        return Ok(-1);
    };

    first_element.set_html(wasm_store, &html);

    Ok(0)
}

#[aidoku_wasm_function]
pub fn prepend(
    mut caller: Caller<'_, WasmStore>,
    descriptor: i32,
    text: Option<String>,
) -> Result<i32> {
    let text = text.unwrap_or_default();

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor as usize)
        .context("failed to get standard value")?;

    let Some(first_element) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => elements.first(),
        _ => None,
    }) else {
        return Ok(-1);
    };

    first_element.prepend(wasm_store, &text);

    Ok(0)
}

#[aidoku_wasm_function]
pub fn append(
    mut caller: Caller<'_, WasmStore>,
    descriptor: i32,
    text: Option<String>,
) -> Result<i32> {
    let text = text.unwrap_or_default();

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor as usize)
        .context("failed to get standard value")?;

    let Some(first_element) = (match std_value.as_ref() {
        Value::HTMLElements(elements) => elements.first(),
        _ => None,
    }) else {
        return Ok(-1);
    };

    first_element.append(wasm_store, &text);

    Ok(0)
}

#[aidoku_wasm_function]
pub fn first(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let element = std_value
        .try_unwrap_html_elements_ref()
        .map_err(|_| anyhow!("expected HTMLElements value"))?
        .first()
        .cloned()
        .context("no elements found in HTMLElements")?;

    Ok(wasm_store.store_std_value(Value::from(vec![element]).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn last(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let element = std_value
        .try_unwrap_html_elements_ref()
        .map_err(|_| anyhow!("expected HTMLElements value"))?
        .last()
        .cloned()
        .context("no elements found in HTMLElements")?;

    Ok(wasm_store.store_std_value(Value::from(vec![element]).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn next(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let element = match std_value.as_ref() {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let new_element = element
        .next(wasm_store)
        .context("no next sibling element found")?;

    Ok(wasm_store.store_std_value(Value::from(vec![new_element]).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn previous(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let element = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let new_element = element
        .previous(wasm_store)
        .context("no previous sibling element found")?;

    Ok(wasm_store.store_std_value(Value::from(vec![new_element]).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn base_uri(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let element = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let base_uri = element.base_uri.unwrap_or("".to_owned());

    Ok(wasm_store.store_std_value(Value::from(base_uri).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn body(caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    select(caller, descriptor_i32, Some("body".into()))
}

#[aidoku_wasm_function]
pub fn text(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    let text = elements
        .iter()
        .filter_map(|element| element.text(wasm_store))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned();

    Ok(wasm_store.store_std_value(Value::from(text).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn untrimmed_text(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    let text = elements
        .iter()
        .filter_map(|element| element.untrimmed_text(wasm_store))
        .collect::<Vec<_>>()
        .join(" ")
        .to_owned();

    Ok(wasm_store.store_std_value(Value::from(text).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn own_text(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let own_text = match std_value.as_ref() {
        Value::HTMLElements(elements) => elements.first().and_then(|n| n.own_text(wasm_store)),
        _ => None,
    }
    .context("expected single HTMLElement or String value")?;

    Ok(wasm_store.store_std_value(Value::String(own_text).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn data(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let text = match std_value.as_ref() {
        Value::HTMLElements(elements) => elements.first().and_then(|n| n.data(wasm_store)),
        _ => None,
    }
    .context("expected single HTMLElement or String value")?;

    Ok(wasm_store.store_std_value(Value::String(text).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn array(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    let array_value: Vec<Value> = elements
        .iter()
        .map(|element| vec![element.clone()].into())
        .collect();

    Ok(wasm_store.store_std_value(Value::from(array_value).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn html(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    let inner_htmls = elements
        .iter()
        .filter_map(|element| element.html(wasm_store))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(wasm_store.store_std_value(Value::from(inner_htmls).into(), None) as i32)
}

#[aidoku_wasm_function]
pub fn outer_html(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?;
    let elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    let htmls = elements
        .iter()
        .filter_map(|element| element.outer_html(wasm_store))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(wasm_store.store_std_value(Value::from(htmls).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn escape(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let text = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) => {
            let text = elements
                .iter()
                .filter_map(|element| element.text(wasm_store))
                .collect::<Vec<_>>()
                .join(" ");
            Some(text)
        }
        Value::String(s) => Some(s.to_owned()),
        _ => None,
    }
    .context("expected HTMLElements or String value")?;

    let escaped = html_escape::encode_safe(&text).to_string();

    Ok(wasm_store.store_std_value(Value::from(escaped).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
fn unescape(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let text = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) => {
            let text = elements
                .iter()
                .filter_map(|element| element.text(wasm_store))
                .collect::<Vec<_>>()
                .join(" ");
            Some(text)
        }
        Value::String(s) => Some(s.to_owned()),
        _ => None,
    }
    .context("expected HTMLElements or String value")?;

    let unescaped = html_escape::decode_html_entities(&text).to_string();

    Ok(wasm_store.store_std_value(Value::from(unescaped).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn id(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let element = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let id = element
        .id(wasm_store)
        .context("element has no id attribute")?
        .to_owned();

    Ok(wasm_store.store_std_value(Value::from(id).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn tag_name(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let element = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let tag_name = element
        .tag(wasm_store)
        .context("not exists element")?
        .to_owned();

    Ok(wasm_store.store_std_value(Value::from(tag_name).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn class_name(mut caller: Caller<'_, WasmStore>, descriptor_i32: i32) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let element = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let class_name = element
        .class(wasm_store)
        .context("element has no class attribute")?;

    Ok(wasm_store.store_std_value(Value::from(class_name).into(), Some(descriptor)) as i32)
}

#[aidoku_wasm_function]
pub fn has_class(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    class_name: Option<String>,
) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;
    let class_name = class_name.context("class_name is required")?;

    let wasm_store = caller.data_mut();
    let element = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let has_class = element
        .has_class(wasm_store, &class_name)
        .unwrap_or_default();

    Ok(if has_class { 1 } else { 0 })
}

#[aidoku_wasm_function]
pub fn has_attr(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    attr_name: Option<String>,
) -> Result<i32> {
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;
    let attr_name = attr_name.context("attr_name is required")?;

    let wasm_store = caller.data_mut();
    let element = match wasm_store
        .get_std_value(descriptor)
        .context("failed to get value from store")?
        .as_ref()
    {
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.last().unwrap().clone())
        }
        _ => None,
    }
    .context("expected a single HTMLElement")?;

    let has_attr = element.has_attr(wasm_store, &attr_name).unwrap_or_default();

    Ok(if has_attr { 1 } else { 0 })
}
