use serde::Serialize;

use shared::model::{
    Chapter as DomainChapter, Manga as DomainManga, SourceInformation as DomainSourceInformation,
};

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
}

impl From<DomainChapter> for Chapter {
    fn from(
        DomainChapter {
            information: chapter_information,
            state,
            downloaded,
        }: DomainChapter,
    ) -> Self {
        Self {
            // FIXME what the fuck why
            source_id: chapter_information.id.source_id().value().clone(),
            manga_id: chapter_information.id.manga_id().value().clone(),
            id: chapter_information.id.value().clone(),
            title: chapter_information.title.unwrap_or("Unknown title".into()),
            scanlator: chapter_information.scanlator,
            chapter_num: chapter_information
                .chapter_number
                .map(|decimal| decimal.try_into().unwrap()),
            volume_num: chapter_information
                .volume_number
                .map(|decimal| decimal.try_into().unwrap()),
            read: state.read,
            last_read: state.last_read,
            downloaded,
        }
    }
}
