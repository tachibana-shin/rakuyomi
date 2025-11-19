use rust_decimal::Decimal;
use serde::Deserialize;
use url::Url;

use crate::source::{
    model::{Chapter as SourceChapter, Manga as SourceManga},
    SourceManifest,
};

#[derive(Clone, Eq, PartialEq, Hash, Deserialize, Debug)]
#[serde(transparent)]
pub struct SourceId {
    source_id: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct MangaId {
    source_id: SourceId,
    manga_id: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
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
        }
    }
}
