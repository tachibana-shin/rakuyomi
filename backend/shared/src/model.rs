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
    pub source_of_source: Option<String>,
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
    pub chapter_number: Option<f32>,
    pub volume_number: Option<f32>,
    pub last_updated: Option<i64>,
    pub thumbnail: Option<Url>,
    pub lang: Option<String>,
    pub url: Option<Url>,
    pub locked: Option<bool>,
}

#[derive(Default, Clone, Debug)]
pub struct MangaState {
    pub preferred_scanlator: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
pub enum TrackingService {
    #[serde(alias = "anilist")]
    AniList,
    #[serde(alias = "myanimelist")]
    MyAnimeList,
    #[serde(alias = "shikimori")]
    Shikimori,
    #[serde(alias = "kavita")]
    Kavita,
}

impl TrackingService {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AniList => "anilist",
            Self::MyAnimeList => "myanimelist",
            Self::Shikimori => "shikimori",
            Self::Kavita => "kavita",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::AniList => "AniList",
            Self::MyAnimeList => "MyAnimeList",
            Self::Shikimori => "Shikimori",
            Self::Kavita => "Kavita",
        }
    }
}

impl TryFrom<&str> for TrackingService {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> anyhow::Result<Self> {
        match value {
            "anilist" => Ok(Self::AniList),
            "myanimelist" => Ok(Self::MyAnimeList),
            "shikimori" => Ok(Self::Shikimori),
            "kavita" => Ok(Self::Kavita),
            other => Err(anyhow::anyhow!("unsupported tracking service: {other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackingStatus {
    Current,
    Completed,
    Paused,
    Dropped,
    Planning,
    Repeating,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackingSyncDirection {
    Push,
    Pull,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TrackingBinding {
    pub service: TrackingService,
    pub remote_media_id: i64,
    pub remote_title: String,
    pub remote_url: Option<Url>,
    pub total_chapters: Option<i64>,
    pub total_volumes: Option<i64>,
    pub last_synced_progress: Option<i64>,
    pub last_synced_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TrackingCandidate {
    pub service: TrackingService,
    pub remote_media_id: i64,
    pub title: String,
    pub url: Option<Url>,
    pub total_chapters: Option<i64>,
    pub total_volumes: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TrackingProgressSnapshot {
    pub status: Option<TrackingStatus>,
    pub chapter_progress: Option<i64>,
    pub volume_progress: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TrackingSyncResult {
    pub service: TrackingService,
    pub direction: TrackingSyncDirection,
    pub local_progress: Option<i64>,
    pub remote_progress: Option<i64>,
    pub message: String,
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
            chapter_number: value.chapter_num,
            volume_number: value.volume_num,
            last_updated: value.date_uploaded.map(|d| d.timestamp()),
            thumbnail: value.thumbnail,
            lang: value.lang,
            url: value.url,
            locked: value.locked,
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
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct Playlist {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "sqlx", derive(sqlx::FromRow))]
pub struct PlaylistManga {
    pub playlist_id: i64,
    pub source_id: String,
    pub manga_id: String,
}
