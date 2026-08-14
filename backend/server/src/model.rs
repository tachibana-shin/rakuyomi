use serde::Serialize;

use shared::{
    chapter_storage::ChapterStorage,
    model::{
        Chapter as DomainChapter, Manga as DomainManga,
        SourceInformation as DomainSourceInformation,
    },
    source::model::MangaViewer,
};

/// Resolves each manga's cover to a local `file://` URL served from the
/// chapter storage's downloaded poster, if one exists -- otherwise leaves the
/// existing (typically remote) `cover_url` as-is, rather than clearing it.
pub fn resolve_manga_covers(mangas: &mut [DomainManga], chapter_storage: &ChapterStorage) {
    for manga in mangas.iter_mut() {
        if let Some(local_cover_url) = chapter_storage
            .poster_exists(&manga.information.id)
            .and_then(|path| path_to_file_url(&path))
        {
            manga.information.cover_url = Some(local_cover_url);
        }
    }
}

/// Converts a local filesystem path into a `file://` URL, falling back to
/// canonicalizing the path first if the direct conversion fails (e.g. for a
/// relative path `Url::from_file_path` can't handle on its own).
pub fn path_to_file_url(path: &std::path::Path) -> Option<url::Url> {
    match url::Url::from_file_path(path) {
        Ok(url) => Some(url),
        Err(_) => match path.canonicalize() {
            Ok(canonical_path) => url::Url::from_file_path(canonical_path).ok(),
            Err(e) => {
                println!("Error canonicalizing path: {}", e);
                None
            }
        },
    }
}

#[derive(Serialize)]
pub struct SourceInformation {
    id: String,
    name: String,
    version: usize,
    source_of_source: Option<String>,
}

impl From<DomainSourceInformation> for SourceInformation {
    fn from(value: DomainSourceInformation) -> Self {
        Self {
            id: value.id.value().clone(),
            name: value.name,
            version: value.version,
            source_of_source: value.source_of_source,
        }
    }
}

#[derive(Serialize)]
pub struct Manga {
    // FIXME maybe both `id` and `source_id` should be encoded into a single field
    // imo it makes more sense from the frontend perspective
    id: String,
    source: SourceInformation,
    title: String,
    unread_chapters_count: Option<usize>,
    last_read: Option<i64>,
    in_library: bool,
    manga_cover: Option<url::Url>,
    viewer: MangaViewer,
    state_viewer: bool,
}

impl From<DomainManga> for Manga {
    fn from(value: DomainManga) -> Self {
        Self {
            id: value.information.id.value().clone(),
            source: value.source_information.into(),
            title: value.information.title.unwrap_or("Unknown title".into()),
            unread_chapters_count: value.unread_chapters_count,
            last_read: value.last_read,
            in_library: value.in_library,
            manga_cover: value.information.cover_url,
            viewer: value.information.viewer,
            state_viewer: value.state_viewer,
        }
    }
}

#[derive(Serialize)]
pub struct Chapter {
    source_id: String,
    manga_id: String,
    id: String,
    title: String,
    scanlator: Option<String>,
    chapter_num: Option<f32>,
    volume_num: Option<f32>,
    read: bool,
    last_read: Option<i64>,
    downloaded: bool,
    locked: bool,
    lang: Option<String>,
    on_tmpfs: bool,
}

impl From<DomainChapter> for Chapter {
    fn from(
        DomainChapter {
            information: chapter_information,
            state,
            downloaded,
            on_tmpfs,
        }: DomainChapter,
    ) -> Self {
        Self {
            // FIXME what the fuck why
            source_id: chapter_information.id.source_id().value().clone(),
            manga_id: chapter_information.id.manga_id().value().clone(),
            id: chapter_information.id.value().clone(),
            title: chapter_information.title.unwrap_or("Unknown title".into()),
            scanlator: chapter_information.scanlator,
            chapter_num: chapter_information.chapter_number,
            volume_num: chapter_information.volume_number,
            read: state.read,
            last_read: state.last_read,
            downloaded,
            locked: chapter_information.locked.unwrap_or_default(),
            lang: chapter_information.lang,
            on_tmpfs,
        }
    }
}
