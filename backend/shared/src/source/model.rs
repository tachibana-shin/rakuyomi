use chrono::{DateTime, TimeZone};
use num_enum::FromPrimitive;
use serde::{Deserialize, Serialize};
use url::Url;

// FIXME This model isn't exactly correct, as it allows groups to be nested inside other groups; while
// Aidoku only allows top-level groups (or so it seems). Refactoring this might make this simpler later, but yeah.
//
// REFACT `Serialize` is only needed here because we use it in the `server` code, in order to
// be able to read those setting definitions in the frontend. We should use a separate serializable type in
// the frontend in order to separate concerns.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum SettingDefinition {
    #[serde(rename = "group")]
    Group {
        title: Option<String>,
        items: Vec<SettingDefinition>,
        footer: Option<String>,
    },
    #[serde(rename = "select")]
    // `segment` works just like a `select`, but it's shown as a segmented button in Aidoku.
    #[serde(alias = "segment")]
    Select {
        title: String,
        key: String,
        #[serde(alias = "options")]
        values: Vec<String>,
        titles: Option<Vec<String>>,
        default: Option<String>,
    },
    #[serde(rename = "multi-select")]
    MultiSelect {
        title: String,
        key: String,
        values: Vec<String>,
        titles: Option<Vec<String>>,
        default: Vec<String>,
    },
    #[serde(rename = "login")]
    Login { title: String, key: String },
    #[serde(rename = "editable-list")]
    EditableList {
        title: String,
        key: String,
        placeholder: Option<String>,
        default: Vec<String>,
    },
    #[serde(rename = "switch")]
    Switch {
        title: String,
        key: String,
        default: bool,
    },
    #[serde(rename = "text")]
    Text {
        placeholder: String,
        key: String,
        // FIXME is text the only setting type that's allowed to not have a default?
        default: Option<String>,
    },
    #[serde(rename = "link")]
    Link { title: String, url: String },
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, FromPrimitive)]
#[repr(u8)]
pub enum PublishingStatus {
    #[default]
    Unknown = 0,
    Ongoing = 1,
    Completed = 2,
    Cancelled = 3,
    Hiatus = 4,
    NotPublished = 5,
}

impl From<aidoku::MangaStatus> for PublishingStatus {
    fn from(value: aidoku::MangaStatus) -> Self {
        match value {
            aidoku::MangaStatus::Unknown => Self::Unknown,
            aidoku::MangaStatus::Ongoing => Self::Ongoing,
            aidoku::MangaStatus::Completed => Self::Completed,
            aidoku::MangaStatus::Cancelled => Self::Cancelled,
            aidoku::MangaStatus::Hiatus => Self::Hiatus,
        }
    }
}

#[derive(Serialize, Debug, Clone, Default, FromPrimitive)]
#[repr(u8)]
pub enum MangaContentRating {
    #[default]
    Safe = 0,
    Suggestive = 1,
    Nsfw = 2,
}
impl From<aidoku::ContentRating> for MangaContentRating {
    fn from(value: aidoku::ContentRating) -> Self {
        match value {
            aidoku::ContentRating::Unknown => Self::Safe,
            aidoku::ContentRating::Suggestive => Self::Suggestive,
            aidoku::ContentRating::NSFW => Self::Nsfw,
            aidoku::ContentRating::Safe => Self::Safe,
        }
    }
}

#[derive(Serialize, Debug, Clone, Default, FromPrimitive)]
#[repr(u8)]
pub enum MangaViewer {
    #[default]
    DefaultViewer = 0,
    Rtl = 1,
    Ltr = 2,
    Vertical = 3,
    Scroll = 4,
}
impl From<aidoku::Viewer> for MangaViewer {
    fn from(value: aidoku::Viewer) -> Self {
        match value {
            aidoku::Viewer::Unknown => Self::DefaultViewer,
            aidoku::Viewer::LeftToRight => Self::Ltr,
            aidoku::Viewer::RightToLeft => Self::Rtl,
            aidoku::Viewer::Vertical => Self::Vertical,
            aidoku::Viewer::Webtoon => Self::Scroll,
        }
    }
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Manga {
    pub source_id: String,
    pub id: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub cover_url: Option<Url>,
    pub url: Option<Url>,
    pub status: PublishingStatus,
    pub nsfw: MangaContentRating,
    pub viewer: MangaViewer,
    // FIXME i dont think those are needed, the sources have no way of creating them
    pub last_updated: Option<DateTime<chrono_tz::Tz>>,
    pub last_opened: Option<DateTime<chrono_tz::Tz>>,
    pub last_read: Option<DateTime<chrono_tz::Tz>>,
    pub date_added: Option<DateTime<chrono_tz::Tz>>,
}

impl Manga {
    pub fn from(value: aidoku::Manga, source_id: String) -> Self {
        Self {
            source_id: source_id,
            title: Some(value.title),
            id: value.key,
            author: value.authors.map(|v| v.join(", ")),
            artist: value.artists.map(|v| v.join(", ")),
            description: value.description,
            tags: value.tags,
            cover_url: value.cover.and_then(|u| url::Url::parse(&u).ok()),
            url: value.url.and_then(|u| url::Url::parse(&u).ok()),
            status: value.status.into(),
            nsfw: value.content_rating.into(),
            viewer: value.viewer.into(),
            last_updated: None,
            last_opened: None,
            last_read: None,
            date_added: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MangaPageResult {
    // FIXME should not this be `mangas` instead?
    pub manga: Vec<Manga>,
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Chapter {
    pub source_id: String,
    pub id: String,
    pub manga_id: String,
    pub title: Option<String>,
    pub scanlator: Option<String>,
    pub url: Option<Url>,
    pub lang: String,
    pub chapter_num: Option<f32>,
    pub volume_num: Option<f32>,
    pub date_uploaded: Option<DateTime<chrono_tz::Tz>>,
    // FIXME do we like really need this? aidoku only uses it to order stuff
    // on the display page, but we already return an array on the get chapter list
    // call, so there's already an ordering there
    pub source_order: usize,
}
impl Chapter {
    pub fn from(value: aidoku::Chapter, source_id: String, manga_id: String) -> Self {
        Self {
            source_id,
            id: value.key,
            manga_id,
            title: value.title,
            scanlator: value.scanlators.map(|v| v.join(", ")),
            url: value.url.and_then(|v| url::Url::parse(&v).ok()),
            lang: value.language.unwrap_or("en".to_owned()),
            chapter_num: value.chapter_number,
            volume_num: value.volume_number,
            date_uploaded: value.date_uploaded.map(|v| {
                chrono::Utc
                    .timestamp_opt(v, 0)
                    .single()
                    .map(|d| d.with_timezone(&chrono_tz::UTC))
                    .unwrap()
            }),
            source_order: usize::MAX,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    pub source_id: String,
    pub chapter_id: String,
    pub index: usize,
    pub image_url: Option<Url>,
    pub base64: Option<String>,
    pub text: Option<String>,
    pub ctx: Option<aidoku::PageContext>,
}
impl Page {
    pub fn from(index: usize, page: aidoku::Page, source_id: String, chapter_id: String) -> Self {
        Self {
            source_id,
            chapter_id,
            index,
            image_url: match &page.content {
                aidoku::PageContent::Url(ref url, _) => Some(url::Url::parse(&url).unwrap()),
                _ => None,
            },
            base64: None,
            text: match &page.content {
                aidoku::PageContent::Text(ref text) => Some(text.clone()),
                _ => None,
            },
            ctx: match &page.content {
                aidoku::PageContent::Url(_, ref ctx) => ctx.clone(),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeepLink {
    // FIXME should we store references here?
    pub manga: Option<Manga>,
    pub chapter: Option<Chapter>,
}

#[derive(Debug, Copy, Clone, Default, FromPrimitive)]
#[repr(u8)]
pub enum FilterType {
    #[default]
    Base = 0,
    Group = 1,
    Text = 2,
    Check = 3,
    Select = 4,
    Sort = 5,
    SortSelection = 6,
    Title = 7,
    Author = 8,
    Genre = 9,
}

#[derive(Debug, Clone)]
pub enum Filter {
    Title(String),
}

impl From<&Filter> for FilterType {
    fn from(value: &Filter) -> Self {
        match &value {
            Filter::Title(_) => FilterType::Title,
        }
    }
}

impl Filter {
    pub fn name(&self) -> String {
        match &self {
            Filter::Title(_) => "Title".into(),
        }
    }
}
