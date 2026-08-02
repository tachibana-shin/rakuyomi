//! `JsValue` <-> Rust conversions between the LNReader plugin contract
//! (`NovelItem`/`SourceNovel`/`ChapterItem`, see `docs/docs.md` in
//! `lnreader-plugins`) and Rakuyomi's own `Manga`/`Chapter`/`Page` (see
//! `source::model`), following the mapping in the v2 feasibility report §3.

use std::collections::HashMap;

use anyhow::{Context as _, Result};
use boa_engine::{js_string, property::PropertyKey, Context, JsValue};

use crate::source::model::{Chapter, Manga, Page, PublishingStatus};

pub(super) fn get_prop(object: &JsValue, key: &str, context: &mut Context) -> Result<JsValue> {
    let obj = object
        .as_object()
        .with_context(|| format!("expected an object while reading `{key}`"))?;
    let prop_key: PropertyKey = js_string!(key).into();
    obj.get(prop_key, context)
        .map_err(|e| anyhow::anyhow!("failed to read `{key}`: {e}"))
}

fn opt_string(object: &JsValue, key: &str, context: &mut Context) -> Result<Option<String>> {
    let value = get_prop(object, key, context)?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(
        value
            .to_string(context)
            .map_err(|e| anyhow::anyhow!("`{key}` is not a string: {e}"))?
            .to_std_string_escaped(),
    ))
}

fn req_string(object: &JsValue, key: &str, context: &mut Context) -> Result<String> {
    opt_string(object, key, context)?.with_context(|| format!("required field `{key}` is missing"))
}

fn opt_f32(object: &JsValue, key: &str, context: &mut Context) -> Result<Option<f32>> {
    let value = get_prop(object, key, context)?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(
        value
            .to_number(context)
            .map_err(|e| anyhow::anyhow!("`{key}` is not a number: {e}"))? as f32,
    ))
}

/// Reads an array-like `JsValue` (a real `Array`, or anything with a numeric
/// `.length`) into a `Vec<JsValue>`.
pub(super) fn js_array_to_vec(value: &JsValue, context: &mut Context) -> Result<Vec<JsValue>> {
    if value.is_undefined() || value.is_null() {
        return Ok(Vec::new());
    }
    let length = get_prop(value, "length", context)?
        .to_number(context)
        .map_err(|e| anyhow::anyhow!("array has no numeric `length`: {e}"))?
        as usize;

    let obj = value.as_object().context("expected an array-like object")?;
    let mut items = Vec::with_capacity(length);
    for i in 0..length {
        let key: PropertyKey = i.into();
        let item = obj
            .get(key, context)
            .map_err(|e| anyhow::anyhow!("failed to read array element {i}: {e}"))?;
        items.push(item);
    }
    Ok(items)
}

/// Best-effort mapping of LNReader's `NovelStatus` (`@libs/novelStatus`,
/// itself just string constants — see `docs.md`'s `SourceNovel::status`) to
/// Rakuyomi's `PublishingStatus`. Plugins may also set an arbitrary raw
/// string, so unrecognized values fall back to `Unknown` rather than erroring.
fn status_from_str(value: &str) -> PublishingStatus {
    match value {
        "Ongoing" => PublishingStatus::Ongoing,
        "Completed" | "Publishing Finished" => PublishingStatus::Completed,
        "Cancelled" => PublishingStatus::Cancelled,
        "On Hiatus" => PublishingStatus::Hiatus,
        // "Licensed": the source stopped distributing it (licensing), closest
        // fit in Rakuyomi's own status set.
        "Licensed" => PublishingStatus::NotPublished,
        _ => PublishingStatus::Unknown,
    }
}

/// Converts one `Plugin.NovelItem` (`{path, name, cover?}`, see `docs.md`)
/// into a `Manga`, as returned by `popularNovels`/`searchNovels`.
pub(super) fn manga_from_novel_item(
    item: &JsValue,
    source_id: &str,
    context: &mut Context,
) -> Result<Manga> {
    let id = req_string(item, "path", context)?;
    let title = opt_string(item, "name", context)?;
    let cover_url = opt_string(item, "cover", context)?.and_then(|u| url::Url::parse(&u).ok());

    Ok(Manga {
        source_id: source_id.to_string(),
        id,
        title,
        cover_url,
        ..Default::default()
    })
}

/// Converts one `Plugin.SourceNovel` (see `docs.md`) into a `Manga`, as
/// returned by `parseNovel`. `manga_id` is the id we asked for (== the
/// `path` we called `parseNovel` with) — used as-is rather than re-read from
/// `SourceNovel::path`, since the plugin contract requires them to match
/// anyway (`docs.md`: "SourceNovel::path should be the same value as
/// NovelItem::path provided as parameter").
pub(super) fn manga_from_source_novel(
    novel: &JsValue,
    source_id: &str,
    manga_id: &str,
    context: &mut Context,
) -> Result<Manga> {
    let title = opt_string(novel, "name", context)?;
    let cover_url = opt_string(novel, "cover", context)?.and_then(|u| url::Url::parse(&u).ok());
    let author = opt_string(novel, "author", context)?;
    let artist = opt_string(novel, "artist", context)?;
    let description = opt_string(novel, "summary", context)?;
    let tags = opt_string(novel, "genres", context)?
        .map(|genres| genres.split(',').map(|s| s.trim().to_string()).collect());
    let status = opt_string(novel, "status", context)?
        .map(|s| status_from_str(&s))
        .unwrap_or_default();

    Ok(Manga {
        source_id: source_id.to_string(),
        id: manga_id.to_string(),
        title,
        author,
        artist,
        description,
        tags,
        cover_url,
        status,
        ..Default::default()
    })
}

/// Reads `SourceNovel.chapters` (`Plugin.ChapterItem[]`, see `docs.md`) into
/// `Chapter`s, one-to-one — not the `vi.hakovn` "one Aidoku chapter = one
/// volume" pattern (report v2 §2).
pub(super) fn chapters_from_source_novel(
    novel: &JsValue,
    source_id: &str,
    manga_id: &str,
    context: &mut Context,
) -> Result<Vec<Chapter>> {
    let chapters_value = get_prop(novel, "chapters", context)?;
    let items = js_array_to_vec(&chapters_value, context)?;

    let mut chapters = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        chapters.push(chapter_from_chapter_item(
            item, source_id, manga_id, index, context,
        )?);
    }
    Ok(chapters)
}

fn chapter_from_chapter_item(
    item: &JsValue,
    source_id: &str,
    manga_id: &str,
    source_order: usize,
    context: &mut Context,
) -> Result<Chapter> {
    let id = req_string(item, "path", context)?;
    let title = opt_string(item, "name", context)?;
    let chapter_num = opt_f32(item, "chapterNumber", context)?;
    let date_uploaded =
        opt_string(item, "releaseTime", context)?.and_then(|s| parse_release_time(&s));

    Ok(Chapter {
        source_id: source_id.to_string(),
        id,
        manga_id: manga_id.to_string(),
        title,
        chapter_num,
        date_uploaded,
        source_order,
        ..Default::default()
    })
}

/// `ChapterItem::releaseTime` is documented as `YYYY-MM-DD` but plugins vary
/// in practice; only the documented format is parsed here (no dependency on
/// the `dayjs` shim, which isn't wired up for native use yet — see the plan's
/// "Suite" section).
fn parse_release_time(value: &str) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().with_timezone(&chrono_tz::UTC))
}

/// Reads a plain `{key: value}` JS object (e.g. `PluginBase.imageRequestInit.headers`)
/// into a `HashMap<String, String>`. Non-string keys (symbols) are skipped;
/// non-string values are coerced via `.to_string()`.
pub(super) fn js_object_to_string_map(
    value: &JsValue,
    context: &mut Context,
) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    if value.is_undefined() || value.is_null() {
        return Ok(map);
    }
    let obj = value.as_object().context("expected an object")?;
    for key in obj
        .own_property_keys(context)
        .map_err(|e| anyhow::anyhow!("failed to enumerate object keys: {e}"))?
    {
        let PropertyKey::String(key_str) = &key else {
            continue;
        };
        let value = obj.get(key.clone(), context).map_err(|e| {
            anyhow::anyhow!("failed to read `{}`: {e}", key_str.to_std_string_escaped())
        })?;
        let value_str = value
            .to_string(context)
            .map_err(|e| anyhow::anyhow!("failed to stringify value: {e}"))?
            .to_std_string_escaped();
        map.insert(key_str.to_std_string_escaped(), value_str);
    }
    Ok(map)
}

/// Builds the single `Page` for a chapter's HTML content, per the "1 chapter
/// = 1 Page" strategy (report v2 §2) — `chapter_downloader.rs` treats a page
/// with `.text` set as a novel chapter and assembles it into the EPUB
/// automatically, no changes needed there.
pub(super) fn page_from_chapter_html(
    html: String,
    source_id: &str,
    chapter_id: &str,
    title: Option<String>,
) -> Page {
    Page {
        source_id: source_id.to_string(),
        chapter_id: chapter_id.to_string(),
        index: 0,
        image_url: None,
        base64: title,
        text: Some(html),
        ctx: None,
    }
}
