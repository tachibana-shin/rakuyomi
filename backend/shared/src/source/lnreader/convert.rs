use aidoku::{Chapter, ContentRating, Manga, MangaStatus, Viewer};
use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::source::model::Page;

/// Returns an error if the JSON value does not have the given field.
fn get<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .ok_or_else(|| anyhow!("missing field `{}` in {}", key, value))
}

fn as_string(value: &Value, key: &str) -> Result<Option<String>> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => bail!("field `{}` is not a string: {}", key, other),
    }
}

fn as_f64(value: &Value, key: &str) -> Result<Option<f64>> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| anyhow!("field `{}` is not a number", key)),
        Some(other) => bail!("field `{}` is not a number: {}", key, other),
    }
}

fn parse_timestamp(s: &str) -> Option<i64> {
    // ISO 8601 (e.g. `2017-10-02T05:48:45.000Z`)
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    None
}

/// Converts the JSON array produced by the JS runtime (`search` / `popular`
/// methods) into a list of mangas.
pub fn mangas_from_search(value: &Value) -> Result<Vec<Manga>> {
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("expected search result to be an array, got {}", value))?;
    items
        .iter()
        .map(|item| {
            Ok(Manga {
                key: get(item, "path")?
                    .as_str()
                    .ok_or_else(|| anyhow!("chapter path is not a string"))?
                    .to_string(),
                title: get(item, "name")?.as_str().unwrap_or_default().to_string(),
                cover: as_string(item, "cover")?,
                artists: None,
                authors: None,
                description: None,
                url: None,
                tags: None,
                status: MangaStatus::Unknown,
                content_rating: ContentRating::Safe,
                viewer: Viewer::LeftToRight,
                update_strategy: aidoku::UpdateStrategy::Always,
                next_update_time: None,
                chapters: Some(Vec::new()),
            })
        })
        .collect()
}

/// Converts the JSON object produced by the `novel` method into a manga.
pub fn manga_from_novel(value: &Value) -> Result<Manga> {
    let title = get(value, "name")?.as_str().unwrap_or_default().to_string();
    let key = value
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let authors = as_string(value, "author")?
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());
    let artists = as_string(value, "artist")?
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());
    let tags = as_string(value, "genres")?
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        });
    let status = as_string(value, "status")?
        .as_deref()
        .map(manga_status)
        .unwrap_or(MangaStatus::Unknown);

    Ok(Manga {
        key,
        title,
        cover: as_string(value, "cover")?,
        artists,
        authors,
        description: as_string(value, "summary")?,
        url: None,
        tags,
        status,
        content_rating: ContentRating::Safe,
        viewer: Viewer::LeftToRight,
        update_strategy: aidoku::UpdateStrategy::Always,
        next_update_time: None,
        chapters: Some(Vec::new()),
    })
}

/// Converts the JSON array of chapter objects (`novel` / `page` methods) into
/// a list of chapters.
pub fn chapters(value: &Value) -> Result<Vec<Chapter>> {
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("expected chapters to be an array, got {}", value))?;
    items
        .iter()
        .map(|item| {
            let key = get(item, "path")?
                .as_str()
                .ok_or_else(|| anyhow!("chapter path is not a string"))?
                .to_string();
            let title = get(item, "name")?
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_default();
            Ok(Chapter {
                key,
                title: Some(title),
                chapter_number: as_f64(item, "chapterNumber")?.map(|n| n as f32),
                volume_number: None,
                date_uploaded: as_string(item, "releaseTime")?.and_then(|s| parse_timestamp(&s)),
                scanlators: as_string(item, "scanlator")?
                    .filter(|s| !s.is_empty())
                    .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                url: None,
                language: None,
                thumbnail: None,
                locked: false,
            })
        })
        .collect()
}

/// The HTML marker used by the downloader to distinguish raw HTML from
/// markdown. See [`crate::util::into_html`].
pub const HTML_MARKER: &str = "<!-- html -->\n";

/// Builds the single page holding the raw chapter HTML produced by the
/// `chapter` method.
pub fn page_from_chapter_html(index: usize, html: &str, chapter_id: String) -> Page {
    Page {
        source_id: String::new(),
        chapter_id,
        index,
        image_url: None,
        base64: None,
        text: Some(format!("{HTML_MARKER}{html}")),
        ctx: None,
    }
}

/// Maps an LNReader novel status string onto an Aidoku [`MangaStatus`].
pub fn manga_status(status: &str) -> MangaStatus {
    let status = status.to_lowercase();
    if status.contains("ongoing") || status.contains("releasing") {
        MangaStatus::Ongoing
    } else if status.contains("complete") {
        MangaStatus::Completed
    } else if status.contains("cancelled") || status.contains("canceled") {
        MangaStatus::Cancelled
    } else if status.contains("hiatus") {
        MangaStatus::Hiatus
    } else {
        MangaStatus::Unknown
    }
}
