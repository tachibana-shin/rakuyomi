//! Entry point for the LNReader "worker" subprocess: a persistent, isolated
//! process — one per loaded LNReader source, spawned once when that source
//! is loaded and reused for every subsequent call on it (same lifecycle as a
//! WASM Aidoku instance living inside `Store`/`instance` for the source's
//! whole lifetime, see `WasmBlockingSource`) — that reads one
//! newline-delimited JSON [`WorkerRequest`] from stdin, executes it against
//! its (also persistent) [`js_runtime::JsRuntime`], writes one
//! newline-delimited JSON [`WorkerResponse`] to stdout, and loops back to
//! read the next request. Exits cleanly on EOF (the parent closed its stdin,
//! meaning the source was unloaded or the backend is shutting down).
//!
//! Why a whole subprocess: a source with a large catalog (tested: a 22-volume
//! series) crashes the process running its JS — confirmed via debugger to be
//! a native memory issue inside `boa_engine` itself (not something
//! `Context`'s own `recursion_limit` catches, since that only bounds JS-level
//! call frames within one `Context::run()`, not the native Rust reentrancy
//! between chained Promise continuations). Since a native crash takes down
//! the whole OS process it runs in, the only real containment is a separate
//! process: if this worker crashes or hangs, [`super::LnReaderSource`]'s
//! caller (`mod.rs`) detects it (dead pipe, or a read timeout for a hang),
//! returns a normal, catchable `Err` for that one call, and transparently
//! respawns a fresh worker for the next call — instead of losing the entire
//! backend. A worker thread would **not** achieve the crash-containment half
//! of this — a `SIGSEGV` kills the process regardless of which thread
//! triggered it.
//!
//! [`run`] is called from the standalone `lnreader_worker` binary's `main()`
//! (see that crate) — a small, separate process deployed alongside `server`,
//! same pattern as `uds_http_request`/`cbz_metadata_reader`. Nothing inside
//! the `shared` crate itself calls [`run`]; it's exposed crate-externally
//! only via `source::lnreader_worker_main`, specifically for that binary.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use anyhow::{Context as _, Result};
use boa_engine::JsValue;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::settings::SourceSettingValue;
use crate::source::model::{Chapter, Manga, PublishingStatus};

use super::{convert, js_runtime};

#[derive(Serialize, Deserialize)]
pub(super) struct WorkerRequest {
    pub(super) main_js: String,
    pub(super) settings_snapshot: HashMap<String, SourceSettingValue>,
    pub(super) source_id: String,
    pub(super) operation: Operation,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
pub(super) enum Operation {
    SearchMangas { query: String, page: i32 },
    GetMangaDetails { manga_id: String },
    GetChapterList { manga_id: String },
    GetPageList { chapter_id: String },
    GetImageRequestInitHeaders,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub(super) struct WorkerResponse {
    pub(super) ok: bool,
    pub(super) error: Option<String>,
    pub(super) mangas: Option<Vec<MangaDto>>,
    pub(super) has_next_page: Option<bool>,
    pub(super) chapters: Option<Vec<ChapterDto>>,
    pub(super) page_html: Option<String>,
    pub(super) image_request_init_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(super) storage_writes: Vec<(String, SourceSettingValue)>,
}

/// Plain-data mirror of [`Manga`], kept local to this module rather than
/// adding `Deserialize` (and, transitively, to `PublishingStatus` etc.) to
/// the shared model type just for this one IPC boundary.
#[derive(Serialize, Deserialize, Default, Debug)]
pub(super) struct MangaDto {
    source_id: String,
    id: String,
    title: Option<String>,
    author: Option<String>,
    artist: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
    cover_url: Option<String>,
    url: Option<String>,
    status: String,
}

impl From<&Manga> for MangaDto {
    fn from(m: &Manga) -> Self {
        Self {
            source_id: m.source_id.clone(),
            id: m.id.clone(),
            title: m.title.clone(),
            author: m.author.clone(),
            artist: m.artist.clone(),
            description: m.description.clone(),
            tags: m.tags.clone(),
            cover_url: m.cover_url.as_ref().map(ToString::to_string),
            url: m.url.as_ref().map(ToString::to_string),
            status: format!("{:?}", m.status),
        }
    }
}

impl MangaDto {
    pub(super) fn into_manga(self) -> Manga {
        Manga {
            source_id: self.source_id,
            id: self.id,
            title: self.title,
            author: self.author,
            artist: self.artist,
            description: self.description,
            tags: self.tags,
            cover_url: self.cover_url.and_then(|u| Url::parse(&u).ok()),
            url: self.url.and_then(|u| Url::parse(&u).ok()),
            status: status_from_debug_str(&self.status),
            ..Default::default()
        }
    }
}

fn status_from_debug_str(s: &str) -> PublishingStatus {
    match s {
        "Ongoing" => PublishingStatus::Ongoing,
        "Completed" => PublishingStatus::Completed,
        "Cancelled" => PublishingStatus::Cancelled,
        "Hiatus" => PublishingStatus::Hiatus,
        "NotPublished" => PublishingStatus::NotPublished,
        _ => PublishingStatus::Unknown,
    }
}

/// Plain-data mirror of [`Chapter`], same reasoning as [`MangaDto`].
#[derive(Serialize, Deserialize, Default, Debug)]
pub(super) struct ChapterDto {
    source_id: String,
    id: String,
    manga_id: String,
    title: Option<String>,
    chapter_num: Option<f32>,
    date_uploaded: Option<String>,
    source_order: usize,
}

impl From<&Chapter> for ChapterDto {
    fn from(c: &Chapter) -> Self {
        Self {
            source_id: c.source_id.clone(),
            id: c.id.clone(),
            manga_id: c.manga_id.clone(),
            title: c.title.clone(),
            chapter_num: c.chapter_num,
            date_uploaded: c.date_uploaded.map(|d| d.to_rfc3339()),
            source_order: c.source_order,
        }
    }
}

impl ChapterDto {
    pub(super) fn into_chapter(self) -> Chapter {
        Chapter {
            source_id: self.source_id,
            id: self.id,
            manga_id: self.manga_id,
            title: self.title,
            chapter_num: self.chapter_num,
            date_uploaded: self.date_uploaded.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&chrono_tz::UTC))
            }),
            source_order: self.source_order,
            ..Default::default()
        }
    }
}

/// Reads one newline-delimited JSON request from stdin, executes it, writes
/// one newline-delimited JSON response to stdout, and loops back for the
/// next request — until stdin hits EOF (parent closed it), which is a clean
/// exit, not an error. The `JsRuntime` is built lazily from the *first*
/// request's `main_js`/`settings_snapshot` and then kept alive across every
/// later iteration of the loop (mirrors a WASM `Store`/instance living for a
/// source's whole lifetime); later requests only refresh the settings
/// snapshot ([`js_runtime::JsRuntime::update_settings_snapshot`]) — their own
/// `main_js` field is redundant at that point (every request still carries
/// it, since [`WorkerRequest`]'s shape doesn't distinguish "first" from
/// "later" requests) and is simply ignored.
///
/// Never returns an `Err` that would prevent writing *a* response for a
/// request already read: any failure while executing one operation becomes
/// `WorkerResponse { ok: false, error: Some(..), .. }` on stdout instead,
/// and the loop continues — only a genuine crash (caught by the parent as a
/// closed pipe/dead process) or a hang (caught by the parent's read timeout)
/// ends this process without a response for that specific call.
pub fn run() -> Result<()> {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let mut runtime: Option<js_runtime::JsRuntime> = None;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader
            .read_line(&mut line)
            .context("failed to read worker request line from stdin")?;
        if bytes_read == 0 {
            return Ok(()); // EOF: parent closed stdin, clean shutdown.
        }

        let request: WorkerRequest = match serde_json::from_str(line.trim()) {
            Ok(request) => request,
            Err(e) => {
                write_response(
                    &mut writer,
                    &WorkerResponse {
                        ok: false,
                        error: Some(format!("failed to parse worker request: {e}")),
                        ..Default::default()
                    },
                )?;
                continue;
            }
        };

        let rt = match &mut runtime {
            Some(rt) => {
                rt.update_settings_snapshot(request.settings_snapshot);
                rt
            }
            None => runtime.insert(js_runtime::new(
                request.settings_snapshot,
                &request.main_js,
            )?),
        };

        let response = match execute(rt, &request.source_id, request.operation) {
            Ok(response) => response,
            Err(e) => WorkerResponse {
                ok: false,
                error: Some(format!("{e:#}")),
                ..Default::default()
            },
        };
        write_response(&mut writer, &response)?;
    }
}

fn write_response(writer: &mut impl Write, response: &WorkerResponse) -> Result<()> {
    let mut json =
        serde_json::to_string(response).context("failed to serialize worker response")?;
    json.push('\n');
    writer
        .write_all(json.as_bytes())
        .context("failed to write worker response")?;
    writer.flush().context("failed to flush worker response")?;
    Ok(())
}

fn execute(
    runtime: &mut js_runtime::JsRuntime,
    source_id: &str,
    operation: Operation,
) -> Result<WorkerResponse> {
    let mut response = match operation {
        Operation::SearchMangas { query, page } => {
            execute_search_mangas(runtime, source_id, query, page)?
        }
        Operation::GetMangaDetails { manga_id } => {
            execute_get_manga_details(runtime, source_id, manga_id)?
        }
        Operation::GetChapterList { manga_id } => {
            execute_get_chapter_list(runtime, source_id, manga_id)?
        }
        Operation::GetPageList { chapter_id } => execute_get_page_list(runtime, chapter_id)?,
        Operation::GetImageRequestInitHeaders => execute_get_image_request_init_headers(runtime)?,
    };

    response.ok = true;
    response.storage_writes = runtime.take_pending_writes();
    Ok(response)
}

fn execute_search_mangas(
    runtime: &mut js_runtime::JsRuntime,
    source_id: &str,
    query: String,
    page: i32,
) -> Result<WorkerResponse> {
    let query_js = JsValue::from(boa_engine::js_string!(query.as_str()));
    let page_js = JsValue::from(page as f64);
    let result = runtime.call_plugin_method("searchNovels", &[query_js, page_js])?;

    let context = runtime.context();
    let items = convert::js_array_to_vec(&result, context)?;
    let mut mangas = Vec::with_capacity(items.len());
    for item in &items {
        mangas.push(MangaDto::from(&convert::manga_from_novel_item(
            item, source_id, context,
        )?));
    }
    let has_next_page = !mangas.is_empty();

    Ok(WorkerResponse {
        mangas: Some(mangas),
        has_next_page: Some(has_next_page),
        ..Default::default()
    })
}

fn execute_get_manga_details(
    runtime: &mut js_runtime::JsRuntime,
    source_id: &str,
    manga_id: String,
) -> Result<WorkerResponse> {
    let (manga, _chapters) = parse_and_convert_novel(runtime, source_id, &manga_id)?;
    Ok(WorkerResponse {
        mangas: Some(vec![MangaDto::from(&manga)]),
        ..Default::default()
    })
}

fn execute_get_chapter_list(
    runtime: &mut js_runtime::JsRuntime,
    source_id: &str,
    manga_id: String,
) -> Result<WorkerResponse> {
    let (_manga, chapters) = parse_and_convert_novel(runtime, source_id, &manga_id)?;
    Ok(WorkerResponse {
        chapters: Some(chapters.iter().map(ChapterDto::from).collect()),
        ..Default::default()
    })
}

fn execute_get_page_list(
    runtime: &mut js_runtime::JsRuntime,
    chapter_id: String,
) -> Result<WorkerResponse> {
    let chapter_id_js = JsValue::from(boa_engine::js_string!(chapter_id.as_str()));
    let result = runtime.call_plugin_method("parseChapter", &[chapter_id_js])?;
    let context = runtime.context();
    let html = result
        .to_string(context)
        .map_err(|e| anyhow::anyhow!("parseChapter did not return a string: {e}"))?
        .to_std_string_escaped();
    Ok(WorkerResponse {
        page_html: Some(html),
        ..Default::default()
    })
}

fn execute_get_image_request_init_headers(
    runtime: &mut js_runtime::JsRuntime,
) -> Result<WorkerResponse> {
    let image_request_init = runtime.plugin_property("imageRequestInit")?;
    let context = runtime.context();
    let headers = if image_request_init.is_undefined() || image_request_init.is_null() {
        HashMap::new()
    } else {
        let headers_value = convert::get_prop(&image_request_init, "headers", context)?;
        convert::js_object_to_string_map(&headers_value, context)?
    };
    Ok(WorkerResponse {
        image_request_init_headers: Some(headers),
        ..Default::default()
    })
}

/// Calls `parseNovel()` and converts the result to `Manga`/`Vec<Chapter>`
/// immediately, rather than returning the raw `JsValue` — both because a
/// `JsValue` can't safely be held across calls (see
/// `JsRuntime::call_plugin_method`'s doc comment on why the plugin instance
/// itself is re-fetched every call rather than cached, for the same
/// use-after-free reason), and because `execute_get_manga_details`/
/// `execute_get_chapter_list` need to share one converted result instead of
/// each calling into JS independently.
///
/// `parseNovel()` is the one LNReader plugin method that returns both a
/// novel's metadata and its chapter list, but the real UI calls
/// `get_manga_details` and `get_chapter_list` back-to-back when a novel is
/// opened — for a source whose `parseNovel()` fans out over the network per
/// volume (e.g. LNori), that doubled the HTTP work for what's one logical
/// "open a novel" action. `JsRuntime::take_cached_novel` lets the second of
/// the two calls reuse the first's result instead of re-running the plugin;
/// deliberately short-lived and single-use, not a general metadata cache —
/// see `js_runtime::NOVEL_CACHE_TTL`'s doc comment.
fn parse_and_convert_novel(
    runtime: &mut js_runtime::JsRuntime,
    source_id: &str,
    manga_id: &str,
) -> Result<(Manga, Vec<Chapter>)> {
    if let Some(cached) = runtime.take_cached_novel(manga_id) {
        return Ok(cached);
    }

    let manga_id_js = JsValue::from(boa_engine::js_string!(manga_id));
    let novel = runtime.call_plugin_method("parseNovel", &[manga_id_js])?;
    let context = runtime.context();
    let manga = convert::manga_from_source_novel(&novel, source_id, manga_id, context)?;
    let chapters = convert::chapters_from_source_novel(&novel, source_id, manga_id, context)?;

    runtime.cache_novel(manga_id.to_string(), manga.clone(), chapters.clone());
    Ok((manga, chapters))
}
