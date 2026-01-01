use font_kit::{family_name::FamilyName, font::Font, properties::Properties, source::SystemSource};
use image::ImageReader;
use pared::sync::Parc;
use raqote::DrawTarget;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Cursor,
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

use crate::settings::{Settings, SourceSettingValue};

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

// FIXME THIS IS BORKED AS FUCK
unsafe impl Send for Html {}
unsafe impl Sync for Html {}

#[derive(Debug, Clone)]
pub struct HTMLElement {
    pub document: Parc<Html>,
    pub node_id: NodeId,
    pub base_uri: Option<String>,
}

impl HTMLElement {
    pub fn element_ref(&'_ self) -> ElementRef<'_> {
        ElementRef::wrap(self.document.tree.get(self.node_id).unwrap()).unwrap()
    }
    pub fn data(&'_ self) -> String {
        let mut result = String::new();

        for child in self.element_ref().children() {
            if let scraper::Node::Text(text) = child.value() {
                result.push_str(text)
            }
        }

        result
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

#[derive(From)]
pub struct Canvas(pub(crate) DrawTarget);
unsafe impl Send for Canvas {}
unsafe impl Sync for Canvas {}

pub struct WasmStore {
    pub id: String,
    pub context: OperationContext,
    pub source_settings: SourceSettings,
    // FIXME this probably should be source-specific, and not a copy of all settigns
    // we do rely on the `languages` global setting right now, so maybe this is really needed? idk
    pub settings: Settings,
    std_descriptor_pointer: usize,
    std_descriptors: HashMap<usize, ValueRef>,
    std_references: HashMap<usize, Vec<usize>>,
    std_strs_encode: HashSet<usize>,

    requests: HashMap<usize, RequestState>,
    // canvas
    canvass: HashMap<usize, Canvas>,
    // image
    images: HashMap<usize, ImageData>,
    // font
    fonts: HashMap<usize, (String, Properties)>,
    fonts_online: HashMap<usize, Vec<u8>>,
    // js context
    jscontexts: HashMap<usize, JsContext>,
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

            std_descriptor_pointer: 0,
            std_descriptors: HashMap::new(),
            std_references: HashMap::new(),
            std_strs_encode: HashSet::new(),
            requests: HashMap::new(),

            canvass: HashMap::new(),

            images: HashMap::new(),

            fonts: HashMap::new(),
            fonts_online: HashMap::new(),

            jscontexts: HashMap::new(),
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

    pub fn take_std_value(&mut self, descriptor: usize) {
        // println!("Free memory pointer {descriptor}");
        self.std_strs_encode.remove(&descriptor);

        self.free_std_reference(descriptor);

        macro_rules! try_remove {
            ($map:expr) => {
                if $map.remove(&descriptor).is_some() {
                    return; // stop searching
                }
            };
        }

        try_remove!(self.std_descriptors);
        try_remove!(self.requests);
        try_remove!(self.canvass);
        try_remove!(self.images);
        try_remove!(self.fonts);
        try_remove!(self.fonts_online);
        try_remove!(self.jscontexts);
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

    pub fn store_std_value(&mut self, data: ValueRef, from: Option<usize>) -> usize {
        let pointer = self.increase_and_get_std_desciptor_pointer();
        self.std_descriptors.insert(pointer, data);

        if let Some(from) = from {
            self.add_std_reference(from, pointer);
        }

        pointer
    }

    pub fn remove_std_value(&mut self, descriptor: usize) {
        self.take_std_value(descriptor);
    }

    pub fn add_std_reference(&mut self, descriptor: usize, reference: usize) {
        let references_to_descriptor = self.std_references.entry(descriptor).or_default();

        references_to_descriptor.push(reference);
    }
    fn free_std_reference(&mut self, descriptor: usize) {
        if let Some(ids) = self.std_references.remove(&descriptor) {
            for id in ids {
                self.take_std_value(id);
            }
        }
    }

    // TODO change this into a request descriptor
    pub fn create_request(&mut self) -> usize {
        let new_request_state = RequestState::Building(RequestBuildingState::default());
        let idx = self.increase_and_get_std_desciptor_pointer();

        self.requests.insert(idx, new_request_state);

        idx
    }

    pub fn get_mut_request(&mut self, descriptor: usize) -> Option<&mut RequestState> {
        self.requests.get_mut(&descriptor)
    }

    pub fn remove_request(&mut self, descriptor: usize) -> Option<RequestState> {
        self.requests.remove(&descriptor)
    }

    fn increase_and_get_std_desciptor_pointer(&mut self) -> usize {
        let idx = self.std_descriptor_pointer;
        self.std_descriptor_pointer += 1;

        idx
    }

    // canvas.rs
    pub fn create_canvas(&mut self, width: f32, height: f32) -> usize {
        let new_canvas_state = DrawTarget::new(width as i32, height as i32);
        let idx = self.increase_and_get_std_desciptor_pointer();

        self.canvass.insert(idx, Canvas(new_canvas_state));

        idx
    }
    pub fn get_mut_canvas(&mut self, descriptor: usize) -> Option<&mut Canvas> {
        self.canvass.get_mut(&descriptor)
    }
    pub fn create_image(&mut self, data: &[u8]) -> Option<usize> {
        let cursor = Cursor::new(data);
        let rgba_img = ImageReader::new(cursor)
            .with_guessed_format()
            .ok()
            .and_then(|r| r.decode().ok())
            .map(|img| img.to_rgba8())?;

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
    pub fn get_image(&mut self, descriptor: usize) -> Option<&ImageData> {
        self.images.get(&descriptor)
    }
    pub fn set_image_data(&mut self, image: ImageData) -> usize {
        let idx = self.increase_and_get_std_desciptor_pointer();

        self.images.insert(idx, image);

        idx
    }
    pub fn create_font(&mut self, name: String, property: Option<&Properties>) -> Option<usize> {
        let font = SystemSource::new()
            .select_best_match(
                &[FamilyName::Title(name)],
                &property.copied().unwrap_or_default(),
            )
            .ok()
            .and_then(|h| h.load().ok())?;

        let idx = self.increase_and_get_std_desciptor_pointer();

        self.fonts
            .insert(idx, (font.family_name(), font.properties()));

        Some(idx)
    }
    pub fn get_font(&mut self, descriptor: usize) -> Option<Font> {
        let Some((family_name, properties)) = self.fonts.get(&descriptor.clone()) else {
            let buffer = self.fonts_online.get(&descriptor)?;

            return Font::from_bytes(buffer.clone().into(), 0).ok();
        };

        SystemSource::new()
            .select_best_match(&[FamilyName::Title(family_name.clone())], properties)
            .ok()
            .and_then(|h| h.load().ok())
    }
    pub fn set_font_online(&mut self, buffer: &[u8]) -> usize {
        let idx = self.increase_and_get_std_desciptor_pointer();

        self.fonts_online.insert(idx, buffer.to_vec());

        idx
    }
    pub fn create_js_context(&mut self) -> usize {
        let idx = self.increase_and_get_std_desciptor_pointer();

        self.jscontexts
            .insert(idx, JsContext(boa_engine::Context::default()));

        idx
    }
    pub fn get_js_context(&mut self, pointer: usize) -> Option<&mut JsContext> {
        self.jscontexts.get_mut(&pointer)
    }
}

impl TryFrom<&RequestBuildingState> for BlockingRequest {
    type Error = anyhow::Error;

    fn try_from(value: &RequestBuildingState) -> Result<Self, Self::Error> {
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

    fn try_from(value: &RequestBuildingState) -> Result<Self, Self::Error> {
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
            SourceSettingValue::Vec(v) => Value::Array(v.into_iter().map(Value::String).collect()),
            SourceSettingValue::Null => Value::Null,
        }
    }
}
