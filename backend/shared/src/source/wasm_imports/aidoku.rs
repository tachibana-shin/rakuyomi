#![allow(clippy::too_many_arguments)]
use anyhow::{Context, Result};
use chrono::DateTime;
use num_enum::FromPrimitive;
use url::Url;
use wasm_macros::{aidoku_wasm_function, register_wasm_function};
use wasm_shared::{
    get_memory,
    memory_reader::{read_string, read_values},
};
use wasmi::{Caller, Linker};

use crate::source::{
    model::{
        Chapter, DeepLink, Manga, MangaContentRating, MangaPageResult, MangaViewer, Page,
        PublishingStatus,
    },
    wasm_store::{ObjectValue, OperationContextObject, Value, WasmStore},
};

pub fn register_aidoku_imports(linker: &mut Linker<WasmStore>) -> Result<()> {
    register_wasm_function!(linker, "aidoku", "create_manga_result", create_manga_result)?;
    register_wasm_function!(linker, "aidoku", "create_manga", create_manga)?;
    register_wasm_function!(linker, "aidoku", "create_chapter", create_chapter)?;
    register_wasm_function!(linker, "aidoku", "create_page", create_page)?;
    register_wasm_function!(linker, "aidoku", "create_deeplink", create_deeplink)?;

    Ok(())
}

#[aidoku_wasm_function]
fn create_manga(
    mut caller: Caller<'_, WasmStore>,
    id: Option<String>,
    cover_url: Option<String>,
    title: Option<String>,
    author: Option<String>,
    artist: Option<String>,
    description: Option<String>,
    url: Option<String>,
    tags_i32: i32,
    tag_str_lens_i32: i32,
    tag_count_i32: i32,
    status_i32: i32,
    nsfw_i32: i32,
    viewer_i32: i32,
) -> Result<i32> {
    let id = id.context("id is required for create_manga")?;

    let tags = offset_from_i32(tags_i32);
    let tag_str_lens = offset_from_i32(tag_str_lens_i32);
    let tag_count = length_from_i32(tag_count_i32);
    let status = status_i32
        .try_into()
        .ok()
        .map(PublishingStatus::from_primitive)
        .context("invalid status")?;
    let nsfw = nsfw_i32
        .try_into()
        .ok()
        .map(MangaContentRating::from_primitive)
        .context("invalid nsfw rating")?;
    let viewer = viewer_i32
        .try_into()
        .ok()
        .map(MangaViewer::from_primitive)
        .context("invalid viewer type")?;

    let memory = get_memory(&mut caller).context("failed to get memory")?;
    let tags_array = if let (Some(tags), Some(tag_str_lens), Some(tag_count)) =
        (tags, tag_str_lens, tag_count)
    {
        let tag_strings: Vec<usize> = read_values::<i32>(&memory, &caller, tags, tag_count)
            .context("failed to read tag strings")?
            .iter()
            .map(|offset_i32| offset_from_i32(*offset_i32))
            .collect::<Option<_>>()
            .context("failed to parse tag strings")?;

        let tag_string_lengths: Vec<usize> = read_values(&memory, &caller, tag_str_lens, tag_count)
            .context("failed to read tag string lengths")?
            .iter()
            .map(|length_i32| length_from_i32(*length_i32))
            .collect::<Option<_>>()
            .context("failed to parse tag string lengths")?;

        let tags = (0..tag_count)
            .map(|i| {
                maybe_read_sized_string(
                    &mut caller,
                    Some(tag_strings[i]),
                    Some(tag_string_lengths[i]),
                )
            })
            .collect::<Option<Vec<String>>>()
            .context("failed to read tags")?;

        Some(tags)
    } else {
        None
    };

    let wasm_store = caller.data_mut();
    let manga = Manga {
        source_id: wasm_store.id.clone(),
        id,
        title,
        author,
        artist,
        description,
        tags: tags_array,
        cover_url: cover_url.and_then(|url| Url::parse(&url).ok()),
        url: url.and_then(|url| Url::parse(&url).ok()),
        status,
        nsfw,
        viewer,
        ..Manga::default()
    };

    Ok(wasm_store.store_std_value(Value::Object(ObjectValue::Manga(manga)).into(), None) as i32)
}

#[aidoku_wasm_function]
fn create_manga_result(
    mut caller: Caller<'_, WasmStore>,
    manga_array_i32: i32,
    has_more_i32: i32,
) -> Result<i32> {
    let manga_array =
        descriptor_from_i32(manga_array_i32).context("invalid manga array descriptor")?;
    let has_more = has_more_i32 != 0;

    let wasm_store = caller.data_mut();
    let array = match wasm_store
        .get_std_value(manga_array)
        .context("couldn't read manga array from store")?
        .as_ref()
    {
        Value::Array(arr) => Some(arr.clone()),
        _ => None,
    }
    .context("expected an array value")?;

    let manga_array = array
        .into_iter()
        .map(|value| match value {
            Value::Object(ObjectValue::Manga(manga)) => Some(manga),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .context("failed to parse manga array")?;

    let manga_page_result = MangaPageResult {
        manga: manga_array,
        has_next_page: has_more,
    };

    Ok(wasm_store.store_std_value(
        Value::Object(ObjectValue::MangaPageResult(manga_page_result)).into(),
        None,
    ) as i32)
}

#[aidoku_wasm_function]
fn create_chapter(
    mut caller: Caller<'_, WasmStore>,
    id: Option<String>,
    title: Option<String>,
    volume: f32,
    chapter: f32,
    date_uploaded: Option<DateTime<chrono_tz::Tz>>,
    scanlator: Option<String>,
    url: Option<String>,
    lang: Option<String>,
) -> Result<i32> {
    let wasm_store = caller.data_mut();
    let chapter = Chapter {
        source_id: wasm_store.id.clone(),
        id: id.context("id is required for create_chapter")?,
        manga_id: match &wasm_store.context.current_object {
            OperationContextObject::Manga { id } => id.clone(),
            other => anyhow::bail!("unexpected `create_chapter` call under {:?} context", other),
        },
        title,
        scanlator,
        url: url.and_then(|url| Url::parse(&url).ok()),
        lang,
        chapter_num: if chapter > 0.0 { Some(chapter) } else { None },
        volume_num: if volume > 0.0 { Some(volume) } else { None },
        date_uploaded,
        source_order: 123,
        thumbnail: None,
    };

    Ok(
        wasm_store.store_std_value(Value::Object(ObjectValue::Chapter(chapter)).into(), None)
            as i32,
    )
}

#[aidoku_wasm_function]
pub fn create_page(
    mut caller: Caller<'_, WasmStore>,
    index: i32,
    image_url: Option<String>,
    base64: Option<String>,
    text: Option<String>,
) -> Result<i32> {
    let wasm_store = caller.data_mut();
    let page = Page {
        source_id: wasm_store.id.clone(),
        chapter_id: match &wasm_store.context.current_object {
            OperationContextObject::Chapter { id, .. } => id.clone(),
            other => anyhow::bail!("unexpected `create_page` call under {:?} context", other),
        },
        index: index as usize,
        image_url: image_url.and_then(|url| Url::parse(&url).ok()),
        base64,
        text,
        ctx: None,
    };

    Ok(wasm_store.store_std_value(Value::Object(ObjectValue::Page(page)).into(), None) as i32)
}

#[aidoku_wasm_function]
pub fn create_deeplink(mut caller: Caller<'_, WasmStore>, manga: i32, chapter: i32) -> Result<i32> {
    let manga: usize = manga.try_into().context("invalid manga descriptor")?;
    let chapter: usize = chapter.try_into().context("invalid chapter descriptor")?;

    let wasm_store = caller.data_mut();
    let manga = match wasm_store
        .get_std_value(manga)
        .context("couldn't read manga from store")?
        .as_ref()
    {
        Value::Object(ObjectValue::Manga(manga)) => Some(manga.clone()),
        _ => None,
    }
    .context("expected a Manga object")?;

    let chapter = match wasm_store
        .get_std_value(chapter)
        .context("couldn't read chapter from store")?
        .as_ref()
    {
        Value::Object(ObjectValue::Chapter(chapter)) => Some(chapter.clone()),
        _ => None,
    }
    .context("expected a Chapter object")?;

    let deeplink = DeepLink {
        manga: Some(manga),
        chapter: Some(chapter),
    };

    Ok(
        wasm_store.store_std_value(Value::Object(ObjectValue::DeepLink(deeplink)).into(), None)
            as i32,
    )
}

fn descriptor_from_i32(descriptor_i32: i32) -> Option<usize> {
    descriptor_i32.try_into().ok()
}

fn offset_from_i32(offset_i32: i32) -> Option<usize> {
    offset_i32.try_into().ok()
}

fn length_from_i32(len_i32: i32) -> Option<usize> {
    len_i32
        .try_into()
        .ok()
        .and_then(|len| if len > 0 { Some(len) } else { None })
}

fn maybe_read_sized_string(
    caller: &mut Caller<'_, WasmStore>,
    offset: Option<usize>,
    length: Option<usize>,
) -> Option<String> {
    let memory = get_memory(caller)?;

    match (offset, length) {
        (Some(offset), Some(length)) => read_string(&memory, &caller, offset, length),
        _ => None,
    }
}
