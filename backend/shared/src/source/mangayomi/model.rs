//! MangaYomi index metadata parsing and conversion of extension results to
//! the RakuYomi source models.

use serde_json::Value;
use url::Url;

use crate::source::model::{
    Chapter, Manga, MangaContentRating, MangaViewer, Page, PublishingStatus,
};

/// The fields of an `index.json` entry (mangayomi-extensions) the source
/// needs beyond the raw metadata, which is passed to the runtime to build
/// the `MSource` instance.
#[derive(Debug, Clone, Default)]
pub struct ExtensionMeta {
    /// Stringified extension id (numeric in the index).
    pub id: String,
    pub name: String,
    pub lang: String,
    pub base_url: String,
    pub version: String,
    pub source_code_url: Option<String>,
    /// Extension kind: `0` manga, `1` anime, `2` light novel. Anime
    /// extensions (`1`) are rejected during install.
    pub item_type: u8,
    /// The language the extension is written in: `0` Dart (run by the
    /// embedded d4rt_rs interpreter), `1` JavaScript (run by the embedded
    /// QuickJS runtime).
    pub source_code_language: u8,
}

impl ExtensionMeta {
    pub fn from_value(value: &Value) -> Self {
        let str_field = |key: &str| -> String {
            match value.get(key) {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            }
        };
        Self {
            id: match value.get("id") {
                Some(Value::Number(n)) => n.to_string(),
                _ => str_field("id"),
            },
            name: str_field("name"),
            lang: str_field("lang"),
            base_url: str_field("baseUrl"),
            version: str_field("version"),
            source_code_url: match value.get("sourceCodeUrl") {
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                _ => None,
            },
            item_type: match value.get("itemType") {
                Some(Value::Number(n)) => n.as_u64().map(|n| n as u8).unwrap_or(0),
                _ => 0,
            },
            source_code_language: match value.get("sourceCodeLanguage") {
                Some(Value::Number(n)) => n.as_u64().map(|n| n as u8).unwrap_or(0),
                _ => 0,
            },
        }
    }
}

/// Maps the MStatus enum index (see `parseStatus`) onto the rakuyomi
/// publishing status.
fn status_from_index(index: i64) -> PublishingStatus {
    match index {
        0 => PublishingStatus::Ongoing,
        1 => PublishingStatus::Completed,
        2 => PublishingStatus::Cancelled,
        4 => PublishingStatus::Hiatus,
        5 => PublishingStatus::NotPublished,
        _ => PublishingStatus::Unknown,
    }
}

fn str_field(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Resolves an image URL that may be relative against the base URL.
fn resolve_url(base_url: &str, url: &str) -> Option<Url> {
    if url.is_empty() {
        return None;
    }
    if Url::parse(url).is_ok() {
        return Url::parse(url).ok();
    }
    let base = base_url.trim_end_matches('/');
    let path = url.trim_start_matches('/');
    Url::parse(&format!("{base}/{path}")).ok()
}

/// Converts an `MManga` JSON object (as serialised by the bridge) to a
/// rakuyomi `Manga`. The manga URL doubles as its id, mirroring how
/// MangaYomi addresses mangas.
pub fn manga_from_value(source_id: &str, base_url: &str, value: &Value) -> Manga {
    let link = str_field(value, "link");
    let url = resolve_url(base_url, &link);
    let author = str_field(value, "author");
    let artist = str_field(value, "artist");
    let status = value
        .get("status")
        .and_then(Value::as_i64)
        .map(status_from_index)
        .unwrap_or_default();
    // The id mirrors what the MangaYomi app stores: the extension's raw
    // `link`, passed back verbatim to `getDetail`/`getChapterList`. Some
    // extensions (e.g. MangaDex) return relative paths that they join with
    // their own `apiUrl`, so the id must not be absolutised. The resolved
    // URL is kept separately for display purposes.
    let id = if link.is_empty() {
        str_field(value, "url")
    } else {
        link
    };
    Manga {
        source_id: source_id.to_string(),
        id,
        title: Some(str_field(value, "name")).filter(|s| !s.is_empty()),
        author: Some(author).filter(|s| !s.is_empty()),
        artist: Some(artist).filter(|s| !s.is_empty()),
        description: Some(str_field(value, "description")).filter(|s| !s.is_empty()),
        tags: value.get("genre").and_then(Value::as_array).map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_string())
                .collect()
        }),
        cover_url: resolve_url(base_url, &str_field(value, "imageUrl")),
        url,
        status,
        nsfw: MangaContentRating::Safe,
        viewer: MangaViewer::DefaultViewer,
        last_updated: None,
        last_opened: None,
        last_read: None,
        date_added: None,
    }
}

/// Converts the `chapters` array of a `getDetail` result to rakuyomi
/// chapters. Each chapter's URL is its id.
pub fn chapters_from_value(
    source_id: &str,
    manga_id: &str,
    base_url: &str,
    value: &Value,
) -> Vec<Chapter> {
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    list.iter()
        .enumerate()
        .map(|(index, chapter)| {
            let raw_url = str_field(chapter, "url");
            let url = resolve_url(base_url, &raw_url);
            Chapter {
                source_id: source_id.to_string(),
                // Raw chapter URL as the id, mirroring the MangaYomi app: the
                // downloader hands `chapter.id` straight to `getPageList`,
                // which extensions may join with their `apiUrl` (e.g. MangaDex
                // uses bare ids, madara sources absolute URLs).
                id: raw_url,
                manga_id: manga_id.to_string(),
                title: Some(str_field(chapter, "name")).filter(|s| !s.is_empty()),
                scanlator: Some(str_field(chapter, "scanlator")).filter(|s| !s.is_empty()),
                url,
                lang: None,
                chapter_num: None,
                volume_num: None,
                date_uploaded: None,
                source_order: index,
                thumbnail: resolve_url(base_url, &str_field(chapter, "thumbnailUrl")),
                locked: Some(false),
            }
        })
        .collect()
}

/// Converts a `getPageList` result (list of image URL strings or of
/// `{url: ...}` maps) to rakuyomi pages.
pub fn pages_from_value(
    source_id: &str,
    chapter_id: &str,
    base_url: &str,
    value: &Value,
) -> Vec<Page> {
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    list.iter()
        .enumerate()
        .map(|(index, page)| {
            let raw = match page {
                Value::String(s) => s.clone(),
                _ => str_field(page, "url"),
            };
            let image_url = resolve_url(base_url, &raw);
            Page {
                source_id: source_id.to_string(),
                chapter_id: chapter_id.to_string(),
                index,
                image_url,
                base64: None,
                text: None,
                ctx: None,
            }
        })
        .collect()
}

/// Builds the single text page holding the raw chapter HTML produced by
/// `getHtmlContent` (light novel extensions).
pub fn page_from_chapter_html(source_id: &str, chapter_id: &str, index: usize, html: &str) -> Page {
    Page {
        source_id: source_id.to_string(),
        chapter_id: chapter_id.to_string(),
        index,
        image_url: None,
        base64: None,
        text: Some(format!(
            "{}{html}",
            crate::source::lnreader::convert::HTML_MARKER
        )),
        ctx: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extension_meta_from_index_entry() {
        let meta = ExtensionMeta::from_value(&json!({
            "id": 638504049,
            "name": "1st Kiss-Manga",
            "lang": "en",
            "baseUrl": "https://1stkissmanga.org",
            "version": "0.1.3",
            "sourceCodeUrl": "https://example.com/madara.dart"
        }));
        assert_eq!(meta.id, "638504049");
        assert_eq!(meta.name, "1st Kiss-Manga");
        assert_eq!(meta.lang, "en");
        assert_eq!(meta.base_url, "https://1stkissmanga.org");
        assert_eq!(
            meta.source_code_url.as_deref(),
            Some("https://example.com/madara.dart")
        );
        assert_eq!(meta.item_type, 0);
    }

    #[test]
    fn test_extension_meta_item_type() {
        let novel = ExtensionMeta::from_value(&json!({
            "id": 1021112861,
            "name": "RoyalRoad",
            "lang": "en",
            "baseUrl": "https://www.royalroad.com",
            "version": "1.0.0",
            "itemType": 2
        }));
        assert_eq!(novel.item_type, 2);

        let anime = ExtensionMeta::from_value(&json!({
            "id": 1,
            "name": "AniList",
            "lang": "en",
            "baseUrl": "https://anilist.co",
            "version": "1.0.0",
            "itemType": 1
        }));
        assert_eq!(anime.item_type, 1);

        let missing = ExtensionMeta::from_value(&json!({ "id": 2 }));
        assert_eq!(missing.item_type, 0);
    }

    #[test]
    fn test_manga_from_value() {
        let manga = manga_from_value(
            "123",
            "https://example.com",
            &json!({
                "name": "One Piece",
                "link": "/manga/one",
                "imageUrl": "/uploads/one.jpg",
                "description": "Pirate story",
                "genre": ["Action", "Adventure"],
                "status": 0
            }),
        );
        assert_eq!(manga.id, "/manga/one");
        assert_eq!(manga.title.as_deref(), Some("One Piece"));
        assert_eq!(
            manga.cover_url.map(|u| u.to_string()),
            Some("https://example.com/uploads/one.jpg".to_string())
        );
        assert_eq!(manga.status, PublishingStatus::Ongoing);
        assert_eq!(
            manga.tags.as_deref(),
            Some(&["Action".to_string(), "Adventure".to_string()][..])
        );
    }

    #[test]
    fn test_chapters_and_pages() {
        let chapters = chapters_from_value(
            "123",
            "/manga/one",
            "https://example.com",
            &json!([{"name": "Chapter 1", "url": "/manga/one/ch/1"}]),
        );
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].id, "/manga/one/ch/1");
        assert_eq!(chapters[0].source_order, 0);

        let pages = pages_from_value(
            "123",
            "/manga/one/ch/1",
            "https://example.com",
            &json!(["https://cdn.example.com/p1.jpg", {"url": "/img/p2.jpg"}]),
        );
        assert_eq!(pages.len(), 2);
        assert_eq!(
            pages[0].image_url.clone().map(|u| u.to_string()),
            Some("https://cdn.example.com/p1.jpg".to_string())
        );
        assert_eq!(
            pages[1].image_url.clone().map(|u| u.to_string()),
            Some("https://example.com/img/p2.jpg".to_string())
        );
    }
}
