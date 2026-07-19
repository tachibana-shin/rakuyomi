use serde::{Deserialize, Serialize};
use url::Url;

use crate::source::{
    model::{Chapter as SourceChapter, Manga as SourceManga, MangaViewer},
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
    pub viewer: MangaViewer,
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
#[serde(rename_all = "snake_case")]
pub enum TrackingService {
    Anilist,
    MyAnimeList,
    Shikimori,
    Kavita,
    Bangumi,
    Mangabaka,
    Komga,
    Suwayomi,
}

impl TrackingService {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anilist => "anilist",
            Self::MyAnimeList => "myanimelist",
            Self::Shikimori => "shikimori",
            Self::Kavita => "kavita",
            Self::Bangumi => "bangumi",
            Self::Mangabaka => "mangabaka",
            Self::Komga => "komga",
            Self::Suwayomi => "suwayomi",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Anilist => "AniList",
            Self::MyAnimeList => "MyAnimeList",
            Self::Shikimori => "Shikimori",
            Self::Kavita => "Kavita",
            Self::Bangumi => "Bangumi",
            Self::Mangabaka => "MangaBaka",
            Self::Komga => "Komga",
            Self::Suwayomi => "Suwayomi",
        }
    }
}

impl TryFrom<&str> for TrackingService {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> anyhow::Result<Self> {
        match value {
            "anilist" => Ok(Self::Anilist),
            "myanimelist" => Ok(Self::MyAnimeList),
            "shikimori" => Ok(Self::Shikimori),
            "kavita" => Ok(Self::Kavita),
            "bangumi" => Ok(Self::Bangumi),
            "mangabaka" => Ok(Self::Mangabaka),
            "komga" => Ok(Self::Komga),
            "suwayomi" => Ok(Self::Suwayomi),
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
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
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
    /// Unix timestamp in seconds when the user started reading.
    pub started_at: Option<i64>,
    /// Unix timestamp in seconds when the user completed the series.
    pub completed_at: Option<i64>,
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
    pub on_tmpfs: bool,
}

pub struct Manga {
    pub source_information: SourceInformation,
    pub information: MangaInformation,
    pub state: MangaState,
    pub unread_chapters_count: Option<usize>,
    pub last_read: Option<i64>,
    pub in_library: bool,
    pub state_viewer: bool,
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
            viewer: value.viewer,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_id_new_and_value() {
        let id = SourceId::new("test_source".to_string());
        assert_eq!(id.value(), "test_source");
    }

    #[test]
    fn test_source_id_clone() {
        let id = SourceId::new("test_source".to_string());
        let cloned = id.clone();
        assert_eq!(id, cloned);
    }

    #[test]
    fn test_source_id_equality() {
        let id1 = SourceId::new("source_a".to_string());
        let id2 = SourceId::new("source_a".to_string());
        let id3 = SourceId::new("source_b".to_string());
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_manga_id_from_strings() {
        let id = MangaId::from_strings("source1".to_string(), "manga1".to_string());
        assert_eq!(id.source_id().value(), "source1");
        assert_eq!(id.value(), "manga1");
    }

    #[test]
    fn test_manga_id_new() {
        let source_id = SourceId::new("source1".to_string());
        let id = MangaId::new(source_id, "manga1".to_string());
        assert_eq!(id.source_id().value(), "source1");
        assert_eq!(id.value(), "manga1");
    }

    #[test]
    fn test_manga_id_equality() {
        let id1 = MangaId::from_strings("src".to_string(), "manga".to_string());
        let id2 = MangaId::from_strings("src".to_string(), "manga".to_string());
        let id3 = MangaId::from_strings("src".to_string(), "other".to_string());
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_manga_id_hash() {
        let id1 = MangaId::from_strings("src".to_string(), "manga".to_string());
        let id2 = MangaId::from_strings("src".to_string(), "manga".to_string());
        let mut map = std::collections::HashMap::new();
        map.insert(id1, "value1");
        assert_eq!(map.get(&id2), Some(&"value1"));
    }

    #[test]
    fn test_chapter_id_from_strings() {
        let id = ChapterId::from_strings(
            "source1".to_string(),
            "manga1".to_string(),
            "chapter1".to_string(),
        );
        assert_eq!(id.source_id().value(), "source1");
        assert_eq!(id.manga_id().value(), "manga1");
        assert_eq!(id.value(), "chapter1");
    }

    #[test]
    fn test_chapter_id_new() {
        let manga_id = MangaId::from_strings("src".to_string(), "manga".to_string());
        let id = ChapterId::new(manga_id, "ch1".to_string());
        assert_eq!(id.source_id().value(), "src");
        assert_eq!(id.manga_id().value(), "manga");
        assert_eq!(id.value(), "ch1");
    }

    #[test]
    fn test_chapter_id_equality() {
        let id1 =
            ChapterId::from_strings("src".to_string(), "manga".to_string(), "ch1".to_string());
        let id2 =
            ChapterId::from_strings("src".to_string(), "manga".to_string(), "ch1".to_string());
        let id3 =
            ChapterId::from_strings("src".to_string(), "manga".to_string(), "ch2".to_string());
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_chapter_id_hash() {
        let id1 =
            ChapterId::from_strings("src".to_string(), "manga".to_string(), "ch1".to_string());
        let id2 =
            ChapterId::from_strings("src".to_string(), "manga".to_string(), "ch1".to_string());
        let mut set = std::collections::HashSet::new();
        set.insert(id1);
        assert!(set.contains(&id2));
    }

    #[test]
    fn test_manga_state_default() {
        let state = MangaState::default();
        assert!(state.preferred_scanlator.is_none());
    }

    #[test]
    fn test_chapter_state_default() {
        let state = ChapterState::default();
        assert!(!state.read);
        assert!(state.last_read.is_none());
    }

    #[test]
    fn test_source_information_from_manifest() {
        let manifest = crate::source::SourceManifest {
            info: crate::source::SourceInfo {
                id: "test_id".to_string(),
                lang: Some("en".to_string()),
                #[cfg(not(feature = "all"))]
                languages: None,
                #[cfg(not(feature = "all"))]
                content_rating: None,
                name: "Test Source".to_string(),
                version: 1,
                url: None,
                urls: None,
                min_app_version: None,
            },
            config: None,
            source_of_source: Some("test_sos".to_string()),
        };
        let info = SourceInformation::from(manifest);
        assert_eq!(info.id.value(), "test_id");
        assert_eq!(info.name, "Test Source");
        assert_eq!(info.version, 1);
        assert_eq!(info.source_of_source, Some("test_sos".to_string()));
    }
}
