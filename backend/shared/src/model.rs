use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::source::{
    model::{Chapter as SourceChapter, Manga as SourceManga},
    SourceManifest,
};

#[derive(Clone, Eq, PartialEq, Hash, Deserialize, Debug, Serialize)]
#[serde(transparent)]
pub struct SourceId {
    source_id: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize)]
pub struct MangaId {
    source_id: SourceId,
    manga_id: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize)]
pub struct ChapterId {
    manga_id: MangaId,
    chapter_id: String,
}

impl SourceId {
    pub fn new(value: String) -> Self {
        Self { source_id: value }
    }

    pub fn value(&self) -> &String {
        &self.source_id
    }
}

impl MangaId {
    pub fn new(source_id: SourceId, value: String) -> Self {
        Self {
            source_id,
            manga_id: value,
        }
    }

    pub fn from_strings(source_id: String, manga_id: String) -> Self {
        let source_id = SourceId::new(source_id);

        Self {
            source_id,
            manga_id,
        }
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn value(&self) -> &String {
        &self.manga_id
    }
}

impl ChapterId {
    pub fn new(manga_id: MangaId, value: String) -> Self {
        Self {
            manga_id,
            chapter_id: value,
        }
    }

    pub fn from_strings(source_id: String, manga_id: String, chapter_id: String) -> Self {
        let manga_id = MangaId::from_strings(source_id, manga_id);

        Self {
            manga_id,
            chapter_id,
        }
    }

    pub fn source_id(&self) -> &SourceId {
        self.manga_id.source_id()
    }

    pub fn manga_id(&self) -> &MangaId {
        &self.manga_id
    }

    pub fn value(&self) -> &String {
        &self.chapter_id
    }
}

#[derive(Clone, Deserialize, Debug)]
pub struct SourceInformation {
    pub id: SourceId,
    pub name: String,
    pub version: usize,

    // source of source
    #[serde(skip)]
    pub source_of_source: Option<String>
}

#[derive(Clone, Debug)]
pub struct MangaInformation {
    pub id: MangaId,
    pub title: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub cover_url: Option<Url>,
}

#[derive(Clone, Debug)]
pub struct ChapterInformation {
    pub id: ChapterId,
    pub title: Option<String>,
    pub scanlator: Option<String>,
    pub chapter_number: Option<Decimal>,
    pub volume_number: Option<Decimal>,
    pub last_updated: Option<i64>,
    pub thumbnail: Option<Url>,
    pub lang: Option<String>,
    pub url: Option<Url>,
}

#[derive(Default, Clone, Debug)]
pub struct MangaState {
    pub preferred_scanlator: Option<String>,
}

#[derive(Default)]
pub struct ChapterState {
    pub read: bool,
    pub last_read: Option<i64>,
}

pub struct Chapter {
    pub information: ChapterInformation,
    pub state: ChapterState,
    pub downloaded: bool,
}

pub struct Manga {
    pub source_information: SourceInformation,
    pub information: MangaInformation,
    pub state: MangaState,
    pub unread_chapters_count: Option<usize>,
    pub last_read: Option<i64>,
    pub in_library: bool,
}

impl From<SourceManifest> for SourceInformation {
    fn from(value: SourceManifest) -> Self {
        Self {
            id: SourceId::new(value.info.id),
            name: value.info.name,
            version: value.info.version,
            source_of_source: value.source_of_source,
        }
    }
}

impl From<SourceManga> for MangaInformation {
    fn from(value: SourceManga) -> Self {
        Self {
            id: MangaId::from_strings(value.source_id, value.id),
            title: value.title,
            author: value.author,
            artist: value.artist,
            cover_url: value.cover_url,
        }
    }
}

impl From<SourceChapter> for ChapterInformation {
    fn from(value: SourceChapter) -> Self {
        Self {
            id: ChapterId::from_strings(value.source_id, value.manga_id, value.id),
            title: value.title,
            scanlator: value.scanlator,
            // FIXME is this ever fallible?
            chapter_number: value.chapter_num.map(|num| num.try_into().unwrap()),
            volume_number: value.volume_num.map(|num| num.try_into().unwrap()),
            last_updated: value.date_uploaded.map(|d| d.timestamp()),
            thumbnail: value.thumbnail,
            lang: value.lang,
            url: value.url,
        }
    }
}

#[derive(Serialize)]
pub struct NotificationInformation {
    pub id: i64,
    pub chapter_id: ChapterId,
    pub manga_title: String,
    pub manga_cover: Option<Url>,
    pub manga_status: Option<i64>,
    pub chapter_title: String,
    pub chapter_number: f64,
    pub created_at: i64,
}
