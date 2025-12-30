use anyhow::{Context, Result};
use quick_xml::SeError;
use quick_xml::{de::from_str, se::to_string};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::model::{ChapterInformation, MangaInformation};
use crate::source::model::Page;

// ComicInfo.xml schema implementation based on ComicRack standard
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct ComicInfo {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub series: String,
    #[serde(default)]
    pub number: String,
    #[serde(default = "default_negative_one")]
    pub count: i32,
    #[serde(default = "default_negative_float_one")]
    pub volume: f32,
    #[serde(default)]
    pub alternate_series: String,
    #[serde(default)]
    pub alternate_number: String,
    #[serde(default = "default_negative_one")]
    pub alternate_count: i32,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "default_negative_one")]
    pub year: i32,
    #[serde(default = "default_negative_one")]
    pub month: i32,
    #[serde(default = "default_negative_one")]
    pub day: i32,
    #[serde(default)]
    pub writer: String,
    #[serde(default)]
    pub penciller: String,
    #[serde(default)]
    pub inker: String,
    #[serde(default)]
    pub colorist: String,
    #[serde(default)]
    pub letterer: String,
    #[serde(default)]
    pub cover_artist: String,
    #[serde(default)]
    pub editor: String,
    #[serde(default)]
    pub translator: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub imprint: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub web: String,
    #[serde(default)]
    pub page_count: i32,
    #[serde(default)]
    pub language_iso: String,
    #[serde(default)]
    pub format: String,
    #[serde(default = "default_unknown")]
    pub black_and_white: String,
    #[serde(default = "default_unknown")]
    pub manga: String,
    #[serde(default)]
    pub characters: String,
    #[serde(default)]
    pub teams: String,
    #[serde(default)]
    pub locations: String,
    #[serde(default)]
    pub scan_information: String,
    #[serde(default)]
    pub story_arc: String,
    #[serde(default)]
    pub story_arc_number: String,
    #[serde(default)]
    pub series_group: String,
    #[serde(default = "default_unknown")]
    pub age_rating: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub community_rating: Option<f32>,
    #[serde(default)]
    pub main_character_or_team: String,
    #[serde(default)]
    pub review: String,
    #[serde(default)]
    pub gtin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<ComicInfoPages>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ComicInfoPages {
    #[serde(rename = "Page")]
    pub pages: Vec<ComicPageInfo>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ComicPageInfo {
    #[serde(rename = "Image")]
    pub image: i32,
    #[serde(rename = "Type", default = "default_page_type")]
    pub page_type: String,
    #[serde(rename = "DoublePage", default = "default_false")]
    pub double_page: bool,
    #[serde(rename = "ImageSize", default)]
    pub image_size: i64,
    #[serde(rename = "Key", default)]
    pub key: String,
    #[serde(rename = "Bookmark", default)]
    pub bookmark: String,
    #[serde(rename = "ImageWidth", default = "default_negative_one")]
    pub image_width: i32,
    #[serde(rename = "ImageHeight", default = "default_negative_one")]
    pub image_height: i32,
}

// Helper functions for default values
fn default_negative_one() -> i32 {
    -1
}
fn default_negative_float_one() -> f32 {
    -1.0
}

fn default_unknown() -> String {
    "Unknown".to_string()
}

fn default_page_type() -> String {
    "Story".to_string()
}

fn default_false() -> bool {
    false
}

impl ComicInfo {
    pub fn from_source_metadata(
        manga_info: MangaInformation,
        chapter_info: ChapterInformation,
        page_list: &[Page],
    ) -> Self {
        // Basic mapping from our data model to ComicInfo
        let mut comic_info = ComicInfo {
            title: chapter_info.title.unwrap_or_default(),
            series: manga_info.title.unwrap_or_default(),
            ..Default::default()
        };

        if comic_info.title.is_empty() {
            if let Some(chapter_number) = chapter_info.chapter_number {
                comic_info.title = format!("Ch.{}", chapter_number);
            }
        }

        // Chapter number
        if let Some(chapter) = chapter_info.chapter_number {
            comic_info.number = chapter.to_string();
        }

        // Volume
        if let Some(volume) = chapter_info.volume_number {
            comic_info.volume = volume.trunc();
        }

        // Credits
        if let Some(author) = &manga_info.author {
            comic_info.writer = author.clone();
        }

        if let Some(artist) = &manga_info.artist {
            comic_info.penciller = artist.clone();
        }

        if let Some(scanlator) = &chapter_info.scanlator {
            comic_info.translator = scanlator.clone();
            comic_info.scan_information = format!("Scanlated by {}", scanlator);
        }

        // Set manga flag for Japanese comics
        comic_info.manga = "YesAndRightToLeft".to_string();

        // Set page count
        comic_info.page_count = page_list.len() as i32;

        comic_info
    }

    pub fn to_xml(&self) -> Result<String, SeError> {
        let xml_header = r#"<?xml version="1.0" encoding="utf-8"?>"#;
        let xml_content = to_string(&self)?;
        Ok(format!("{}\n{}", xml_header, xml_content))
    }

    pub fn from_file(file_path: &Path) -> Result<Self> {
        let file = File::open(file_path)
            .with_context(|| format!("Failed to open file '{}'", file_path.display()))?;

        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("Failed to read zip archive '{}'", file_path.display()))?;

        // ComicInfo.xml should be at the root level of the CBZ file
        let mut xml_file = archive
            .by_name("ComicInfo.xml")
            .context("Couldn't find ComicInfo.xml in archive")?;
        let mut contents = String::new();
        xml_file
            .read_to_string(&mut contents)
            .context("Couldn't read ComicInfo.xml")?;

        // Parse XML to ComicInfo struct
        let comic_info: ComicInfo = from_str(&contents).with_context(|| {
            format!("Failed to parse ComicInfo.xml in '{}'", file_path.display())
        })?;

        Ok(comic_info)
    }
}

// Helper to convert empty strings to None, useful for consumers of this module
pub fn non_empty_string(s: String) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}
