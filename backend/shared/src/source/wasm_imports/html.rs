use anyhow::{anyhow, Context, Result};

use kuchiki::traits::TendrilSink;
use scraper::{Element, Html as ScraperHtml, Node, Selector};
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasmi::{Caller, Linker};

use crate::source::wasm_store::{HTMLElement, Html, Value, WasmStore};

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
    let document =
        ScraperHtml::parse_document(&data.context("data is required for parse_with_uri")?);
    let node_id = document.root_element().id();
    let html_element = HTMLElement {
        document: Html::from(document).into(),
        node_id,
        base_uri,
    };

    let wasm_store = caller.data_mut();

    Ok(wasm_store.store_std_value(Value::from(vec![html_element]).into(), None) as i32)
}

#[aidoku_wasm_function]
fn parse_fragment_with_uri(
    mut caller: Caller<'_, WasmStore>,
    data: Option<String>,
    base_uri: Option<String>,
) -> Result<i32> {
    let fragment =
        ScraperHtml::parse_fragment(&data.context("data is required for parse_fragment_with_uri")?);
    let node_id = fragment.root_element().id();
    let html_element = HTMLElement {
        document: Html::from(fragment).into(),
        node_id,
        base_uri,
    };

    let wasm_store = caller.data_mut();

    Ok(wasm_store.store_std_value(Value::from(vec![html_element]).into(), None) as i32)
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
    let selector = Selector::parse(&selector)
        .map_err(|e| anyhow!(e.to_string()))
        .with_context(|| format!("couldn't parse selector '{}'", selector))?;
    let selected_elements: Vec<_> = html_elements
        .iter()
        .flat_map(|element| {
            element
                .element_ref()
                .select(&selector)
                .map(|selected_element_ref| HTMLElement {
                    document: element.document.clone(),
                    node_id: selected_element_ref.id(),
                    base_uri: element.base_uri.clone(),
                })
        })
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
        selector.strip_prefix("abs:").unwrap().to_string()
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
        .map(|element| {
            let element_value = element.element_ref().value();

            element_value.attr(&selector)
        })
        .find(|element| element.is_some())
        .flatten()
        .context(AttributeNotFound)?
        .to_string();

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

fn modify_dom<F>(
    caller: &mut Caller<'_, WasmStore>,
    descriptor_i32: i32,
    mut callback: F,
) -> Result<()>
where
    F: FnMut(&mut kuchiki::NodeRef),
{
    let descriptor: usize = descriptor_i32.try_into().context("invalid descriptor")?;

    let wasm_store = caller.data_mut();
    let std_value = wasm_store
        .get_std_value(descriptor)
        .context("failed to get standard value")?;
    let mut elements = match std_value.as_ref() {
        Value::HTMLElements(elements) => Some(elements.clone()),
        _ => None,
    }
    .context("expected HTMLElements value")?;

    let element = elements.first_mut().context("invalid descriptor")?;
    let base_uri = element.base_uri.clone();

    let html = element.element_ref().html();

    let document = kuchiki::parse_html().one(html);
    let mut root = match document.select_first("*") {
        Ok(v) => v,
        Err(_) => return Err(anyhow::anyhow!("kuchiki could not select root element")),
    }
    .as_node()
    .clone();

    callback(&mut root);

    let mut new_html = Vec::new();
    root.serialize(&mut new_html).unwrap();
    let new_html = String::from_utf8(new_html).unwrap();

    let doc_scraper = scraper::Html::parse_fragment(&new_html);
    let node_id = doc_scraper.root_element().id();

    *element = HTMLElement {
        document: Html::from(doc_scraper).into(),
        node_id,
        base_uri,
    };

    wasm_store.set_std_value(descriptor, Value::from(elements).into());

    Ok(())
}

#[aidoku_wasm_function]
pub fn set_text(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    text: Option<String>,
) -> Result<i32> {
    let text = text.unwrap_or_default();

    modify_dom(
        &mut caller,
        descriptor_i32,
        |root: &mut kuchiki::NodeRef| {
            root.children().for_each(|child| child.detach());

            root.append(kuchiki::NodeRef::new_text(text.clone()));
        },
    )?;

    Ok(0)
}

#[aidoku_wasm_function]
pub fn set_html(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    html: Option<String>,
) -> Result<i32> {
    let html = html.unwrap_or_default();

    modify_dom(
        &mut caller,
        descriptor_i32,
        |root: &mut kuchiki::NodeRef| {
            root.children().for_each(|child| child.detach());

            let fragment = kuchiki::parse_html().one(html.clone());
            let frag_root = fragment
                .select_first("*")
                .expect("no root element in fragment")
                .as_node()
                .clone();

            for child in frag_root.children() {
                root.append(child);
            }
        },
    )?;

    Ok(0)
}

#[aidoku_wasm_function]
pub fn prepend(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    text: Option<String>,
) -> Result<i32> {
    let text = text.unwrap_or_default();

    modify_dom(
        &mut caller,
        descriptor_i32,
        |root: &mut kuchiki::NodeRef| {
            let new_node = kuchiki::NodeRef::new_text(text.clone());

            if let Some(first) = root.first_child() {
                first.insert_before(new_node);
            } else {
                root.append(new_node);
            }
        },
    )?;

    Ok(0)
}

#[aidoku_wasm_function]
pub fn append(
    mut caller: Caller<'_, WasmStore>,
    descriptor_i32: i32,
    text: Option<String>,
) -> Result<i32> {
    let text = text.unwrap_or_default();

    modify_dom(
        &mut caller,
        descriptor_i32,
        |root: &mut kuchiki::NodeRef| {
            root.append(kuchiki::NodeRef::new_text(text.clone()));
        },
    )?;

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

    let next_sibling_node_id = element
        .element_ref()
        .next_sibling_element()
        .context("no next sibling element found")?
        .id();
    let new_element = HTMLElement {
        document: element.document.clone(),
        node_id: next_sibling_node_id,
        base_uri: element.base_uri.clone(),
    };

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

    let prev_sibling_node_id = element
        .element_ref()
        .prev_sibling_element()
        .context("no previous sibling element found")?
        .id();
    let new_element = HTMLElement {
        document: element.document.clone(),
        node_id: prev_sibling_node_id,
        base_uri: element.base_uri.clone(),
    };

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

    let base_uri = element.base_uri.unwrap_or("".to_string());

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
        .flat_map(|element| element.element_ref().text())
        .map(|s| s.trim())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

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
        .flat_map(|element| element.element_ref().text())
        .collect::<Vec<_>>()
        .join(" ");

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
        Value::HTMLElements(elements) if elements.len() == 1 => {
            let element = elements.first().unwrap();
            let own_text = element
                .element_ref()
                .children()
                .filter_map(|node_ref| match node_ref.value() {
                    // FIXME WHAT
                    Node::Text(text) => Some(&**text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            Some(own_text)
        }
        Value::String(s) => Some(s.to_string()),
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
        Value::HTMLElements(elements) if elements.len() == 1 => {
            Some(elements.first().unwrap().data())
        }
        Value::String(s) => Some(s.to_string()),
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
        .map(|element| element.element_ref().inner_html())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(wasm_store.store_std_value(Value::from(inner_htmls).into(), Some(descriptor)) as i32)
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
        .map(|element| element.element_ref().html())
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
                .flat_map(|element| element.element_ref().text())
                .map(|s| s.trim())
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
                .flat_map(|element| element.element_ref().text())
                .map(|s| s.trim())
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
        .element_ref()
        .value()
        .id()
        .context("element has no id attribute")?
        .to_string();

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

    let tag_name = element.element_ref().value().name().to_string();

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
        .element_ref()
        .value()
        .attr("class")
        .context("element has no class attribute")?
        .trim()
        .to_string();

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
        .element_ref()
        .value()
        .classes()
        .any(|class| class == class_name);

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

    let has_attr = element
        .element_ref()
        .value()
        .attrs()
        .any(|(name, _)| name == attr_name);

    Ok(if has_attr { 1 } else { 0 })
}
