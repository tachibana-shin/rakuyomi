use pared::sync::Parc;
use std::collections::{BTreeMap, HashMap};
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
    pub base_uri: Option<Url>,
}

impl HTMLElement {
    pub fn element_ref(&'_ self) -> ElementRef<'_> {
        ElementRef::wrap(self.document.tree.get(self.node_id).unwrap()).unwrap()
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
    #[from(ignore)]
    Array(Vec<Value>),
    #[from(ignore)]
    Object(ObjectValue),
    HTMLElements(Vec<HTMLElement>),
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
    requests: Vec<RequestState>,
}

impl WasmStore {
    pub fn new(id: String, source_settings: SourceSettings, settings: Settings) -> Self {
        Self {
            id,
            source_settings,
            settings,
            ..Default::default()
        }
    }

    pub fn get_std_value(&self, descriptor: usize) -> Option<ValueRef> {
        self.std_descriptors.get(&descriptor).cloned()
    }

    pub fn take_std_value(&mut self, descriptor: usize) -> Option<ValueRef> {
        self.std_descriptors.remove(&descriptor)
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
        self.std_descriptors.remove(&descriptor);
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
            SourceSettingValue::Bool(v) => Value::Bool(v),
            SourceSettingValue::String(v) => Value::String(v),
        }
    }
}
