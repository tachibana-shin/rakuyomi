use font_kit::{family_name::FamilyName, font::Font, properties::Properties, source::SystemSource};
use image::ImageReader;
use pared::sync::Parc;
use raqote::{DrawTarget, Transform};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Cursor,
    sync::{Arc, RwLock},
};
use tokio_util::sync::CancellationToken;

use anyhow::anyhow;
use chrono::DateTime;
use derive_more::{Deref, From, TryUnwrap};
use ego_tree::NodeId;
use reqwest::{
    blocking::Request as BlockingRequest,
    header::{HeaderMap, HeaderName, HeaderValue},
    Method, Request, StatusCode, Url,
};
use scraper::{ElementRef, Html as ScraperHtml};

use crate::{
    scraper_ext::SelectSoup,
    settings::{Settings, SourceSettingValue},
};

use super::{
    model::{Chapter, DeepLink, Filter, Manga, MangaPageResult, Page},
    source_settings::SourceSettings,
};

// We use a BTreeMap instead of a HashMap due to lower average memory overhead:
// https://ntietz.com/blog/rust-hashmap-overhead/
pub type ValueMap = BTreeMap<String, Value>;

#[derive(Debug, Clone, From, TryUnwrap)]
#[try_unwrap(ref, ref_mut)]
// FIXME Apply the suggestion from the following `clippy` lint
// This enum is needlessly large, maybe we could measure the impact of
// actually changing this.
#[allow(clippy::large_enum_variant, dead_code)]
pub enum ObjectValue {
    ValueMap(ValueMap),
    Manga(Manga),
    MangaPageResult(MangaPageResult),
    Chapter(Chapter),
    Page(Page),
    DeepLink(DeepLink),
    Filter(Filter),
}

#[derive(From, Deref, Debug)]
pub struct Html(ScraperHtml);
impl From<Html> for Arc<RwLock<Html>> {
    fn from(value: Html) -> Self {
        Arc::new(RwLock::new(value))
    }
}

// FIXME THIS IS BORKED AS FUCK
unsafe impl Send for Html {}
unsafe impl Sync for Html {}

#[derive(Debug, Clone)]
pub struct HTMLElement {
    pub document: Arc<RwLock<Html>>,
    pub node_id: NodeId,
    pub base_uri: Option<String>,
}
#[derive(thiserror::Error, Debug)]
#[error("NodeDrop: node_id {0} no longer exists")]
pub struct NodeDrop(String);

impl HTMLElement {
    pub fn data(&'_ self) -> Option<String> {
        let mut result = String::new();
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        for child in node.children() {
            match child.value() {
                scraper::Node::Text(text) => result.push_str(&**text),
                _ => {}
            }
        }

        Some(result)
    }
    pub fn attr(&'_ self, name: &str) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        ElementRef::wrap(node)
            .unwrap()
            .attr(name)
            .map(|v| v.to_owned())
    }
    pub fn select<'a>(&'a self, selector: &scraper::Selector) -> Option<Vec<HTMLElement>> {
        let document_guard = self.document.read().unwrap();
        let node = match document_guard.tree.get(self.node_id) {
            Some(n) => n,
            None => {
                return None;
            }
        };

        let element_ref = ElementRef::wrap(node).unwrap();

        let iter = element_ref
            .select_soup(selector)
            .map(move |selected_ref| HTMLElement {
                document: self.document.clone(),
                node_id: selected_ref.id(),
                base_uri: self.base_uri.clone(),
            })
            .collect::<Vec<_>>();

        Some(iter)
    }
    pub fn select_first<'a>(&'a self, selector: &scraper::Selector) -> Option<HTMLElement> {
        let document_guard = self.document.read().unwrap();
        let node = match document_guard.tree.get(self.node_id) {
            Some(n) => n,
            None => {
                return None;
            }
        };

        let element_ref = ElementRef::wrap(node).unwrap();
        let Some(first_node_id) = element_ref.select_soup(selector).next().map(|v| v.id()) else {
            return None;
        };

        Some(HTMLElement {
            document: self.document.clone(),
            node_id: first_node_id,
            base_uri: self.base_uri.clone(),
        })
    }
    pub fn html(&'_ self) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        Some(ElementRef::wrap(node).unwrap().html())
    }
    pub fn inner_html(&'_ self) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        Some(ElementRef::wrap(node).unwrap().inner_html())
    }
    pub fn text(&'_ self) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        Some(
            ElementRef::wrap(node)
                .unwrap()
                .text()
                .map(|s| s.trim())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string(),
        )
    }
    pub fn text_untrimmed(&'_ self) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        Some(
            ElementRef::wrap(node)
                .unwrap()
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .to_string(),
        )
    }
    pub fn own_text(&'_ self) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        Some(
            ElementRef::wrap(node)
                .unwrap()
                .children()
                .filter_map(|node_ref| match node_ref.value() {
                    // FIXME WHAT
                    // not use .text() is function traverse
                    scraper::Node::Text(text) => Some(&**text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        )
    }
    pub fn next_sibling_element(&'_ self) -> Option<HTMLElement> {
        use scraper::Element;
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        ElementRef::wrap(node)
            .unwrap()
            .next_sibling_element()
            .map(|n| HTMLElement {
                document: self.document.clone(),
                node_id: n.id(),
                base_uri: self.base_uri.clone(),
            })
    }
    pub fn prev_sibling_element(&'_ self) -> Option<HTMLElement> {
        use scraper::Element;
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        ElementRef::wrap(node)
            .unwrap()
            .prev_sibling_element()
            .map(|n| HTMLElement {
                document: self.document.clone(),
                node_id: n.id(),
                base_uri: self.base_uri.clone(),
            })
    }
    pub fn parent(&'_ self) -> Option<HTMLElement> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        ElementRef::wrap(node)
            .unwrap()
            .parent()
            .map(|n| HTMLElement {
                document: self.document.clone(),
                node_id: n.id(),
                base_uri: self.base_uri.clone(),
            })
    }
    pub fn children(&'_ self) -> Option<Vec<HTMLElement>> {
        let document_guard = self.document.read().unwrap();
        let node = match document_guard.tree.get(self.node_id) {
            Some(n) => n,
            None => {
                return None;
            }
        };
        let element_ref = ElementRef::wrap(node).unwrap();

        let iter = element_ref
            .children()
            .map(move |selected_ref| HTMLElement {
                document: self.document.clone(),
                node_id: selected_ref.id(),
                base_uri: self.base_uri.clone(),
            })
            .collect::<Vec<_>>();

        Some(iter)
    }
    pub fn next_siblings(&'_ self) -> Option<Vec<HTMLElement>> {
        let document_guard = self.document.read().unwrap();
        let node = match document_guard.tree.get(self.node_id) {
            Some(n) => n,
            None => {
                return None;
            }
        };
        let element_ref = ElementRef::wrap(node).unwrap();

        let iter = element_ref
            .next_siblings()
            .map(move |selected_ref| HTMLElement {
                document: self.document.clone(),
                node_id: selected_ref.id(),
                base_uri: self.base_uri.clone(),
            })
            .collect::<Vec<_>>();

        Some(iter)
    }
    pub fn next_sibling(&'_ self) -> Option<HTMLElement> {
        let document_guard = self.document.read().unwrap();
        let node = match document_guard.tree.get(self.node_id) {
            Some(n) => n,
            None => {
                return None;
            }
        };
        let element_ref = ElementRef::wrap(node).unwrap();

        element_ref
            .next_sibling()
            .map(move |selected_ref| HTMLElement {
                document: self.document.clone(),
                node_id: selected_ref.id(),
                base_uri: self.base_uri.clone(),
            })
    }
    pub fn prev_sibling(&'_ self) -> Option<HTMLElement> {
        let document_guard = self.document.read().unwrap();
        let node = match document_guard.tree.get(self.node_id) {
            Some(n) => n,
            None => {
                return None;
            }
        };
        let element_ref = ElementRef::wrap(node).unwrap();

        element_ref
            .prev_sibling()
            .map(move |selected_ref| HTMLElement {
                document: self.document.clone(),
                node_id: selected_ref.id(),
                base_uri: self.base_uri.clone(),
            })
    }
    pub fn id(&'_ self) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        ElementRef::wrap(node)
            .unwrap()
            .value()
            .id()
            .map(|s| s.to_string())
    }
    pub fn name(&'_ self) -> Option<String> {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return None;
        };

        Some(ElementRef::wrap(node).unwrap().value().name().to_string())
    }
    pub fn has_class(&'_ self, name: &str) -> bool {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return false;
        };

        ElementRef::wrap(node)
            .unwrap()
            .value()
            .classes()
            .any(|class| class == name)
    }
    pub fn has_attr(&'_ self, attr_name: &str) -> bool {
        let document = self.document.read().unwrap();
        let Some(node) = document.tree.get(self.node_id) else {
            return false;
        };

        ElementRef::wrap(node)
            .unwrap()
            .value()
            .attrs()
            .any(|(name, _)| name == attr_name)
    }
}

#[derive(Debug, Clone, From, TryUnwrap)]
#[try_unwrap(ref, ref_mut)]
// FIXME See above.
#[allow(clippy::large_enum_variant)]
pub enum Value {
    Null,
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Date(DateTime<chrono_tz::Tz>),
    Vec(Vec<u8>),
    #[from(ignore)]
    Array(Vec<Value>),
    #[from(ignore)]
    Object(ObjectValue),
    HTMLElements(Vec<HTMLElement>),
    NextFilters(Vec<aidoku::FilterValue>),
    #[from(ignore)]
    NextManga(aidoku::Manga),
    #[from(ignore)]
    NextChapter(aidoku::Chapter),
    #[from(ignore)]
    NextPageContext(aidoku::PageContext),
    #[from(ignore)]
    NextImageResponse(ImageResponse),
}

pub type ValueRef = Parc<Value>;

#[derive(Debug, Default)]
pub struct RequestBuildingState {
    pub url: Option<Url>,
    pub method: Option<Method>,
    pub body: Option<Vec<u8>>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug)]
pub struct ResponseData {
    pub url: Url,
    pub status_code: StatusCode,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    // FIXME refactor this into a ResponseState struct
    pub bytes_read: usize,
}

#[derive(Debug)]
pub enum RequestState {
    Building(RequestBuildingState),
    Sent(ResponseData),
    Closed,
}

// Determines the current object in which operations are being done.
// TODO think about stuff??
#[derive(Debug, Default)]
pub enum OperationContextObject {
    #[default]
    None,
    Manga {
        id: String,
    },
    Chapter {
        id: String,
    },
}

#[derive(Default, Debug)]
pub struct OperationContext {
    pub cancellation_token: CancellationToken,
    pub current_object: OperationContextObject,
}

#[derive(Default, Debug)]
pub struct Drawer {
    pub width: i32,
    pub height: i32,
    pub vec: Vec<u32>,
    pub transform: Transform,
}
impl From<&mut DrawTarget> for Drawer {
    fn from(dt: &mut DrawTarget) -> Self {
        Self {
            width: dt.width() as i32,
            height: dt.height() as i32,
            vec: dt.get_data().to_vec(),
            transform: *dt.get_transform(),
        }
    }
}

// Convert Drawer -> DrawTarget
impl From<&mut Drawer> for DrawTarget {
    fn from(drawer: &mut Drawer) -> Self {
        let data = &drawer.vec;
        let mut dt = DrawTarget::from_vec(drawer.width, drawer.height, data.to_vec()); // ::new(drawer.width, drawer.height);
        dt.set_transform(&drawer.transform);

        dt
    }
}

pub struct ImageData {
    pub data: Vec<u32>,
    pub width: i32,
    pub height: i32,
}

/// from aidoku sdk
/// The details of a HTTP request.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageRef {
    pub rid: i32,
    pub externally_managed: bool,
}

/// from aidoku sdk
/// The details of a HTTP request.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageRequest {
    pub url: Option<String>,
    pub headers: HashMap<String, String>,
}

/// from aidoku sdk
/// A response from a network image request.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ImageResponse {
    /// The HTTP status code.
    pub code: u16,
    /// The HTTP response headers.
    pub headers: HashMap<String, String>,
    /// The HTTP request details.
    pub request: ImageRequest,
    /// A reference to image data.
    pub image: ImageRef,
}

#[derive(From, Debug)]
pub struct JsContext(pub(crate) boa_engine::Context);

// FIXME THIS IS BORKED AS FUCK
unsafe impl Send for JsContext {}
unsafe impl Sync for JsContext {}

pub struct WasmStore {
    pub id: String,
    pub context: OperationContext,
    pub source_settings: SourceSettings,
    // FIXME this probably should be source-specific, and not a copy of all settigns
    // we do rely on the `languages` global setting right now, so maybe this is really needed? idk
    pub settings: Settings,
    std_descriptor_pointer: Option<usize>,
    std_descriptors: HashMap<usize, ValueRef>,
    std_references: HashMap<usize, Vec<usize>>,
    std_strs_encode: HashSet<usize>,
    requests: Vec<RequestState>,
    // canvas
    canvass_pointer: i32,
    canvass: HashMap<i32, Drawer>,
    // image
    images_pointer: i32,
    images: HashMap<i32, ImageData>,
    // font
    fonts_pointer: i32,
    fonts: HashMap<i32, (String, Properties)>,
    fonts_online: HashMap<i32, Vec<u8>>,
    // js context
    jscontext_pointer: i32,
    jscontexts: HashMap<i32, JsContext>,

    pub id_counter: i32,
}
impl std::fmt::Debug for WasmStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmStore")
            .field("id", &self.id)
            .field("context", &self.context)
            .field("source_settings", &self.source_settings)
            .field("settings", &self.settings)
            .field("std_descriptor_pointer", &self.std_descriptor_pointer)
            .field("std_descriptors", &self.std_descriptors)
            .field("std_references", &self.std_references)
            .field("requests", &self.requests)
            // ignore non-debug fields
            // .field("canvass", &self.canvass)
            // .field("images", &self.images)
            // .field("fonts", &self.fonts)
            .finish()
    }
}
impl WasmStore {
    pub fn default(source_settings: SourceSettings) -> Self {
        Self {
            id: String::new(),
            context: OperationContext::default(),

            source_settings,

            settings: Settings::default(),

            std_descriptor_pointer: None,
            std_descriptors: HashMap::new(),
            std_references: HashMap::new(),
            std_strs_encode: HashSet::new(),
            requests: Vec::new(),

            canvass_pointer: 0,
            canvass: HashMap::new(),

            images_pointer: 0,
            images: HashMap::new(),

            fonts_pointer: 0,
            fonts: HashMap::new(),
            fonts_online: HashMap::new(),

            jscontext_pointer: 0,
            jscontexts: HashMap::new(),

            id_counter: 0,
        }
    }
}

impl WasmStore {
    pub fn new(id: String, source_settings: SourceSettings, settings: Settings) -> Self {
        Self {
            id,
            settings,
            ..WasmStore::default(source_settings)
        }
    }

    pub fn get_std_value(&self, descriptor: usize) -> Option<ValueRef> {
        self.std_descriptors.get(&descriptor).cloned()
    }

    pub fn take_std_value(&mut self, descriptor: usize) -> Option<ValueRef> {
        // println!("Free memory pointer {descriptor}");
        self.std_strs_encode.remove(&descriptor);
        self.std_descriptors.remove(&descriptor)
    }

    pub fn mark_str_encode(&mut self, pointer: usize) {
        self.std_strs_encode.insert(pointer);
    }

    pub fn is_str_encode(&mut self, pointer: usize) -> bool {
        self.std_strs_encode.contains(&pointer)
    }

    pub fn set_std_value(&mut self, descriptor: usize, data: ValueRef) {
        self.std_descriptors.insert(descriptor, data);
    }

    pub fn store_std_value(&mut self, data: ValueRef, _from: Option<usize>) -> usize {
        let pointer = self.increase_and_get_std_desciptor_pointer();
        self.std_descriptors.insert(pointer, data);

        pointer
    }

    pub fn remove_std_value(&mut self, descriptor: usize) {
        self.take_std_value(descriptor);
    }

    // This might be used by some Aidoku unimplemented functions
    #[allow(dead_code)]
    pub fn add_std_reference(&mut self, descriptor: usize, reference: usize) {
        let references_to_descriptor = self.std_references.entry(descriptor).or_default();

        references_to_descriptor.push(reference);
    }

    // TODO change this into a request descriptor
    pub fn create_request(&mut self) -> usize {
        let new_request_state = RequestState::Building(RequestBuildingState::default());
        self.requests.push(new_request_state);

        self.requests.len() - 1
    }

    pub fn get_mut_request(&mut self, descriptor: usize) -> Option<&mut RequestState> {
        self.requests.get_mut(descriptor)
    }

    fn increase_and_get_std_desciptor_pointer(&mut self) -> usize {
        let increased_value = match self.std_descriptor_pointer {
            Some(value) => value + 1,
            None => 0,
        };

        self.std_descriptor_pointer = Some(increased_value);

        increased_value
    }

    // canvas.rs
    pub fn create_canvas(&mut self, width: f32, height: f32) -> i32 {
        let new_canvas_state = &mut DrawTarget::new(width as i32, height as i32);
        let idx = self.canvass_pointer;
        self.canvass_pointer += 1;

        self.canvass.insert(idx, new_canvas_state.into());

        idx
    }
    pub fn get_mut_canvas(&mut self, descriptor: i32) -> Option<DrawTarget> {
        self.canvass
            .get_mut(&descriptor)
            .map(|v| <&mut Drawer as Into<DrawTarget>>::into(v))
    }
    pub fn set_canvas(&mut self, descriptor: i32, draw: &mut DrawTarget) {
        self.canvass.insert(descriptor, draw.into());
    }
    pub fn create_image(&mut self, data: &[u8]) -> Option<i32> {
        let cursor = Cursor::new(data);
        let Some(rgba_img) = ImageReader::new(cursor)
            .with_guessed_format()
            .ok()
            .and_then(|r| r.decode().ok())
            .map(|img| img.to_rgba8())
        else {
            return None;
        };
        let width = rgba_img.width() as i32;
        let height = rgba_img.height() as i32;
        let mut pixels: Vec<u32> = Vec::with_capacity((width * height) as usize);

        for pixel in rgba_img.pixels() {
            let [r, g, b, a] = pixel.0;

            let val = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);

            pixels.push(val);
        }
        let image = ImageData {
            data: pixels,
            width,
            height,
        };

        Some(self.set_image_data(image))
    }
    pub fn get_image(&mut self, descriptor: i32) -> Option<&ImageData> {
        self.images.get(&descriptor)
    }
    pub fn set_image_data(&mut self, image: ImageData) -> i32 {
        let idx = self.images_pointer;
        self.images_pointer += 1;

        self.images.insert(idx, image);

        idx
    }
    pub fn create_font(&mut self, name: String, property: Option<&Properties>) -> Option<i32> {
        let Some(font) = SystemSource::new()
            .select_best_match(
                &[FamilyName::Title(name)],
                &property.map(|v| *v).unwrap_or_else(Properties::new),
            )
            .ok()
            .and_then(|h| h.load().ok())
        else {
            return None;
        };

        let idx = self.fonts_pointer;
        self.fonts_pointer += 1;

        self.fonts
            .insert(idx, (font.family_name(), font.properties()));

        Some(idx)
    }
    pub fn get_font(&mut self, descriptor: i32) -> Option<Font> {
        let Some((family_name, properties)) = self.fonts.get(&descriptor.clone()) else {
            let Some(buffer) = self.fonts_online.get(&descriptor) else {
                return None;
            };

            return Font::from_bytes(buffer.clone().into(), 0).ok();
        };
        let font = SystemSource::new()
            .select_best_match(&[FamilyName::Title(family_name.clone())], &properties)
            .ok()
            .and_then(|h| h.load().ok());

        font
    }
    pub fn set_font_online(&mut self, buffer: &[u8]) -> i32 {
        let idx = self.fonts_pointer;
        self.fonts_pointer += 1;

        self.fonts_online.insert(idx, buffer.to_vec());

        idx
    }
    pub fn create_js_context(&mut self) -> i32 {
        let idx = self.jscontext_pointer;
        self.jscontext_pointer += 1;

        self.jscontexts
            .insert(idx, JsContext(boa_engine::Context::default()));

        idx
    }
    pub fn get_js_context(&mut self, pointer: i32) -> Option<&mut JsContext> {
        self.jscontexts.get_mut(&pointer)
    }
}

impl TryFrom<&RequestBuildingState> for BlockingRequest {
    type Error = anyhow::Error;

    fn try_from(value: &RequestBuildingState) -> core::result::Result<Self, Self::Error> {
        let mut request = BlockingRequest::new(
            value
                .method
                .clone()
                .ok_or(anyhow!("expected to have a request method"))?,
            value
                .url
                .clone()
                .ok_or(anyhow!("expected to have an URL"))?,
        );

        for (k, v) in value.headers.iter() {
            request.headers_mut().append(
                HeaderName::from_bytes(k.clone().as_bytes())?,
                HeaderValue::from_str(v.clone().as_str())?,
            );
        }

        if let Some(body) = &value.body {
            *request.body_mut() = Some(body.clone().into());
        }

        Ok(request)
    }
}

// Duplicating here sucks, but there's no real way to avoid it (aside from macros)
// Maybe we should give up on using the blocking reqwest APIs
impl TryFrom<&RequestBuildingState> for Request {
    type Error = anyhow::Error;

    fn try_from(value: &RequestBuildingState) -> core::result::Result<Self, Self::Error> {
        let mut request = Request::new(
            value
                .method
                .clone()
                .ok_or(anyhow!("expected to have a request method"))?,
            value
                .url
                .clone()
                .ok_or(anyhow!("expected to have an URL"))?,
        );

        for (k, v) in value.headers.iter() {
            request.headers_mut().append(
                HeaderName::from_bytes(k.clone().as_bytes())?,
                HeaderValue::from_str(v.as_str())?,
            );
        }

        if let Some(body) = &value.body {
            *request.body_mut() = Some(body.clone().into());
        }

        Ok(request)
    }
}

impl<T> From<Vec<T>> for Value
where
    T: Into<Value>,
{
    fn from(value: Vec<T>) -> Self {
        Value::Array(value.into_iter().map(|element| element.into()).collect())
    }
}

impl<T> From<T> for Value
where
    T: Into<ObjectValue>,
{
    fn from(value: T) -> Self {
        Value::Object(value.into())
    }
}

impl From<SourceSettingValue> for Value {
    fn from(value: SourceSettingValue) -> Self {
        match value {
            SourceSettingValue::Data(v) => Value::Vec(v),
            SourceSettingValue::Bool(v) => Value::Bool(v),
            SourceSettingValue::Int(v) => Value::Int(v),
            SourceSettingValue::Float(v) => Value::Float(v),
            SourceSettingValue::String(v) => Value::String(v),
            SourceSettingValue::Vec(v) => {
                Value::Array(v.into_iter().map(|v| Value::String(v)).collect())
            }
            SourceSettingValue::Null => Value::Null,
        }
    }
}
