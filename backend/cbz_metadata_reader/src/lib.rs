use std::path::Path;

use serde::Serialize;
use shared::cbz_metadata::{non_empty_string, ComicInfo};
use thiserror::Error;

/// Errors that can occur while extracting CBZ metadata.
#[derive(Debug, Error)]
pub enum MetadataError {
    /// The archive contains no `ComicInfo.xml` entry, so no metadata exists.
    #[error("no ComicInfo.xml entry in the archive")]
    MissingComicInfo,
    /// Any other failure (file I/O, invalid archive, malformed XML, ...).
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Simplified metadata in the shape expected by KOReader's document properties.
/// Serialized fields that are `None` are omitted from the JSON output.
#[derive(Serialize, Debug, Default)]
pub struct KoReaderMetadata {
    /// Chapter title, from `ComicInfo/Title`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Series name, from `ComicInfo/Series`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// Chapter number within the series, from `ComicInfo/Number`, kept as a
    /// string so values like "1.5" or "42" are preserved as-is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_index: Option<String>,
    /// Writer, penciller and inker concatenated with " & ", from
    /// `ComicInfo/Writer`, `Penciller` and `Inker`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<String>,
    /// Publisher name, from `ComicInfo/Publisher`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Publication year, from `ComicInfo/Year`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_year: Option<i32>,
    /// Language code (ISO 639), from `ComicInfo/LanguageISO`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Synopsis, mapped from `ComicInfo/Summary` (KOReader's `notes`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Genre tags, mapped from `ComicInfo/Genre` (KOReader's `keywords`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<String>,
    /// Community rating on a 0-5 scale, from `ComicInfo/CommunityRating`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
}

/// Reads `ComicInfo.xml` from a CBZ file and returns the simplified KOReader
/// metadata, in the same shape the `cbz_metadata_reader` binary prints.
///
/// Returns [`MetadataError::MissingComicInfo`] when the archive exists but
/// has no `ComicInfo.xml` entry; all other failures are
/// [`MetadataError::Other`].
pub fn extract_metadata(file_path: &Path) -> Result<KoReaderMetadata, MetadataError> {
    let comic_info = ComicInfo::from_file(file_path).map_err(|err| {
        if err
            .chain()
            .any(|cause| cause.to_string() == "Couldn't find ComicInfo.xml in archive")
        {
            MetadataError::MissingComicInfo
        } else {
            MetadataError::Other(err)
        }
    })?;

    Ok(transform_from_comic_info_xml(comic_info))
}

// Transform ComicInfo.xml data into KoReaderMetadata
fn transform_from_comic_info_xml(comic_info: ComicInfo) -> KoReaderMetadata {
    let mut ko_meta = KoReaderMetadata {
        title: non_empty_string(comic_info.title),
        series: non_empty_string(comic_info.series),
        series_index: non_empty_string(comic_info.number),
        publisher: non_empty_string(comic_info.publisher),
        language: non_empty_string(comic_info.language_iso),
        notes: non_empty_string(comic_info.summary),
        keywords: non_empty_string(comic_info.genre),
        ..Default::default()
    };

    // Combine writer, penciller, inker as authors
    let mut authors = Vec::new();
    if !comic_info.writer.is_empty() {
        authors.push(comic_info.writer);
    }

    if !comic_info.penciller.is_empty() && !authors.contains(&comic_info.penciller) {
        authors.push(comic_info.penciller);
    }

    if !comic_info.inker.is_empty() && !authors.contains(&comic_info.inker) {
        authors.push(comic_info.inker);
    }

    if !authors.is_empty() {
        ko_meta.authors = Some(authors.join(" & "));
    }

    if comic_info.year > 0 {
        ko_meta.publication_year = Some(comic_info.year);
    }

    // Community rating (0-5 scale)
    ko_meta.rating = comic_info.community_rating.map(|r| r.into());

    ko_meta
}
