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
            None => match js_runtime::new(request.settings_snapshot, &request.main_js) {
                Ok(rt) => runtime.insert(rt),
                Err(e) => {
                    // Same treatment as an `execute` failure below: report it
                    // for this request and keep the worker alive for the
                    // next one, rather than `?`-propagating out of `run()`
                    // and exiting the process on the first malformed plugin.
                    write_response(
                        &mut writer,
                        &WorkerResponse {
                            ok: false,
                            error: Some(format!("failed to initialize plugin runtime: {e:#}")),
                            ..Default::default()
                        },
                    )?;
                    continue;
                }
            },
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
    // Apply this call's recombined `{key}__include`/`{key}__exclude`
    // settings pairs onto `plugin.filters` immediately before the plugin's
    // own `searchNovels` runs (see
    // `js_runtime::JsRuntime::apply_settings_filters`). LNReader's app hands
    // the filters object to `popularNovels`, which Rakuyomi never calls —
    // `searchNovels` is the only search entry point here, and real sources
    // read `this.filters.<key>.value` from it (see
    // `docs/lnreader/REFERENCE.md` §11.3), so this is the one place the
    // user's saved filter selections can reach a search.
    runtime.apply_settings_filters()?;

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
///
/// Pagination: `parseNovel()`'s `SourceNovel` only carries page 1's chapters
/// plus a `totalPages` count; the rest of the list is fetched with
/// `parsePage(manga_id, page)` for every page in 2..=totalPages (the
/// LNReader plugin contract — confirmed broken live on `ranobes`, whose
/// `parseNovel()` reports page 1/25 but whose `parsePage` Rakuyomi never
/// called, silently loading only the first page of chapters). Each page's
/// raw `ChapterItem`s are converted immediately (raw `JsValue`s can't be
/// held across the `call_plugin_method` calls), concatenated in page order,
/// and the whole list is reversed exactly **once** at the end — not per
/// page — so the global newest-first order is preserved (same ordering
/// rationale that used to live on `convert::chapters_from_source_novel`, now
/// documented on `convert::chapters_from_chapter_items`). A failing page
/// propagates as an `Err` for the whole call, so a partial list is never
/// returned or cached.
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
    let manga = {
        let context = runtime.context();
        convert::manga_from_source_novel(&novel, source_id, manga_id, context)?
    };

    // Page 1's chapters ship inside `SourceNovel.chapters`; convert them
    // immediately — the raw `JsValue` they came from must not outlive this
    // call (see `JsRuntime::call_plugin_method`'s doc comment).
    let mut chapters = {
        let context = runtime.context();
        let chapters_value = convert::get_prop(&novel, "chapters", context)?;
        let items = convert::js_array_to_vec(&chapters_value, context)?;
        convert::chapters_from_chapter_items(&items, source_id, manga_id, 0, context)?
    };

    // Pages 2..=totalPages come from `parsePage(manga_id, page)`. Each
    // page's raw items are converted immediately and appended in order,
    // keeping `source_order` continuous across pages; any page failure
    // fails the whole call (no partial list is ever returned/cached).
    let total_pages = {
        let context = runtime.context();
        convert::source_novel_total_pages(&novel, context)?
    };
    for page in 2..=total_pages {
        let page_arg = JsValue::from(page as f64);
        let page_items = {
            let manga_id_js = JsValue::from(boa_engine::js_string!(manga_id));
            let page_novel = runtime.call_plugin_method("parsePage", &[manga_id_js, page_arg])?;
            let context = runtime.context();
            convert::js_array_to_vec(&page_novel, context)?
        };
        let page_chapters = {
            let context = runtime.context();
            convert::chapters_from_chapter_items(
                &page_items,
                source_id,
                manga_id,
                chapters.len(),
                context,
            )?
        };
        chapters.extend(page_chapters);
    }

    // Single reversal of the fully concatenated list: preserving the global
    // newest-first order across pages (reversing per page would interleave
    // the pages' orders instead).
    chapters.reverse();

    runtime.cache_novel(manga_id.to_string(), manga.clone(), chapters.clone());
    Ok((manga, chapters))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic plugin whose `searchNovels` reports the current
    /// `plugin.filters.genres` include/exclude state through the returned
    /// title — no network, no real source. Mirrors the corpus-observed
    /// `ExcludableCheckboxGroup` value shape (`docs/lnreader/REFERENCE.md`
    /// §11.2) and the real `searchNovels(searchTerm, page)` argument count
    /// (§11.3).
    const SEARCH_REPORTS_FILTERS_MAIN_JS: &str = r#"
        var filters = {
            genres: {
                label: 'Genres',
                value: { include: [], exclude: [] },
                options: [],
                type: 'ExcludableCheckboxGroup',
            },
        };
        module.exports.default = {
            filters: filters,
            searchNovels: async function (query, page) {
                var v = this.filters.genres.value;
                return [{
                    name: query + '|' + (v.include || []).join('+') + '|' + (v.exclude || []).join('+'),
                    path: 'novel/' + query + '/',
                }];
            },
        };
    "#;

    fn vec_settings(pairs: &[(&str, Vec<&str>)]) -> HashMap<String, SourceSettingValue> {
        pairs
            .iter()
            .map(|(key, values)| {
                (
                    key.to_string(),
                    SourceSettingValue::Vec(values.iter().map(|s| s.to_string()).collect()),
                )
            })
            .collect()
    }

    /// Synthetic plugin whose `parseNovel` reports `totalPages: 3` (page 1
    /// of 3, the exact shape that broke `ranobes` — see
    /// `parse_and_convert_novel`'s doc comment) and whose `parsePage(mangaId,
    /// page)` returns the remaining pages' chapters. No network, no real
    /// source — mirrors the LNReader `SourceNovel.totalPages` +
    /// `parsePage(novelId, page)` contract (§11.x of REFERENCE.md).
    const PAGINATED_NOVEL_MAIN_JS: &str = r#"
        module.exports.default = {
            parseNovel: async function (mangaId) {
                return {
                    name: 'Paged Test Novel',
                    path: mangaId,
                    totalPages: 3,
                    chapters: [
                        { path: 'p1-c1', name: 'Chapter 1' },
                        { path: 'p1-c2', name: 'Chapter 2' },
                    ],
                };
            },
            parsePage: async function (mangaId, page) {
                if (page === 2) {
                    return [
                        { path: 'p2-c3', name: 'Chapter 3' },
                        { path: 'p2-c4', name: 'Chapter 4' },
                    ];
                }
                if (page === 3) {
                    return [
                        { path: 'p3-c5', name: 'Chapter 5' },
                        { path: 'p3-c6', name: 'Chapter 6' },
                    ];
                }
                throw new Error('unexpected page ' + page);
            },
        };
    "#;

    /// Same fixture, but `parsePage` rejects on page 3: the whole call must
    /// fail rather than return/cache a partial list (pages 1+2 only).
    const PAGINATED_NOVEL_FAILING_PAGE_MAIN_JS: &str = r#"
        module.exports.default = {
            parseNovel: async function (mangaId) {
                return {
                    path: mangaId,
                    totalPages: 3,
                    chapters: [{ path: 'p1-c1', name: 'Chapter 1' }],
                };
            },
            parsePage: async function (mangaId, page) {
                if (page === 2) {
                    return [{ path: 'p2-c3', name: 'Chapter 3' }];
                }
                throw new Error('page 3 exploded');
            },
        };
    "#;

    /// Synthetic paginated plugin mirroring `ranobes.js`'s `parsePage` date
    /// handling (see the `Date` shim in `js_runtime.rs`): the site's raw
    /// chapter dates are space-separated (`window.__DATA__.chapters[].date`,
    /// e.g. `"2021-06-27 02:06:47"`), and the plugin converts each one with
    /// `new Date(date).toISOString()` before putting it into `releaseTime`.
    /// Before the shim, boa rejected the space separator, `.toISOString()`
    /// threw `RangeError: Invalid time value`, `parsePage` rejected, and
    /// pagination failed with a 500.
    const RANOBES_STYLE_PAGINATED_MAIN_JS: &str = r#"
        module.exports.default = {
            parseNovel: async function (mangaId) {
                return {
                    path: mangaId,
                    totalPages: 2,
                    chapters: [{ path: 'p1-c1', name: 'Chapter 1' }],
                };
            },
            parsePage: async function (mangaId, page) {
                if (page !== 2) throw new Error('unexpected page ' + page);
                var iso = new Date('2021-06-27 02:06:47').toISOString();
                return [{
                    path: 'p2-c2',
                    name: 'Chapter 2 (' + iso + ')',
                    releaseTime: iso,
                }];
            },
        };
    "#;

    fn novel_runtime(main_js: &str) -> js_runtime::JsRuntime {
        js_runtime::new(HashMap::new(), main_js).expect("runtime construction should not fail")
    }

    /// Regression test for the `ranobes` bug: `parseNovel` returns page 1 of
    /// N with `totalPages`, and `parsePage(manga_id, page)` supplies the rest
    /// — but Rakuyomi never called it, so only the first page's chapters were
    /// ever loaded. Exercises `parse_and_convert_novel` directly (same call
    /// `execute_get_chapter_list`/`execute_get_manga_details` both route
    /// through).
    #[test]
    fn parse_and_convert_novel_loads_all_pages_in_global_order() {
        let mut runtime = novel_runtime(PAGINATED_NOVEL_MAIN_JS);

        let (manga, chapters) = parse_and_convert_novel(&mut runtime, "synthetic", "manga-x")
            .expect("paginated parseNovel should not fail");
        assert_eq!(manga.title.as_deref(), Some("Paged Test Novel"));

        // 2 (page 1) + 2 (page 2) + 2 (page 3) = 6 chapters, all loaded.
        assert_eq!(chapters.len(), 6, "all three pages must be concatenated");

        // Raw concatenation order is oldest-first (p1-c1..p3-c6, in page
        // order); the whole list is reversed exactly once, so the final order
        // is newest-first GLOBALLY (p3-c6 first), not per-page (which would
        // interleave p2/p1 back into the middle).
        let ids: Vec<&str> = chapters.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["p3-c6", "p3-c5", "p2-c4", "p2-c3", "p1-c2", "p1-c1"]
        );

        // `source_order` stays the continuous raw-list index (0-based over
        // the concatenation, before the reversal) — page 2 must not restart
        // at 0.
        let orders: Vec<usize> = chapters.iter().map(|c| c.source_order).collect();
        assert_eq!(orders, vec![5, 4, 3, 2, 1, 0]);

        // The existing one-shot novel cache is preserved and now holds the
        // FULL paginated list (previously it would have held page 1 only).
        let cached = runtime
            .take_cached_novel("manga-x")
            .expect("cached novel should be present after parse_and_convert_novel");
        assert_eq!(cached.1.len(), 6);
    }

    #[test]
    fn parse_and_convert_novel_fails_entirely_when_a_page_errors() {
        let mut runtime = novel_runtime(PAGINATED_NOVEL_FAILING_PAGE_MAIN_JS);

        let err = parse_and_convert_novel(&mut runtime, "synthetic", "manga-x")
            .expect_err("a failing parsePage must fail the whole call, not return a partial list");
        assert!(
            err.to_string().contains("parsePage"),
            "expected the parsePage failure to surface, got: {err}"
        );

        // Nothing cached: the paired get_manga_details/get_chapter_list call
        // must not reuse a partial result.
        assert!(
            runtime.take_cached_novel("manga-x").is_none(),
            "a failed pagination must not be cached"
        );
    }

    /// Regression test for the Ranobes date bug: page 2's `parsePage` runs
    /// `new Date('2021-06-27 02:06:47').toISOString()` (the space-separated
    /// raw site date) and puts the result in `releaseTime`. Before the
    /// `Date` shim, that threw `RangeError: Invalid time value` and failed
    /// the whole pagination; now it must succeed, the page-2 chapter must be
    /// present, and its `releaseTime` (now RFC3339) must have been parsed
    /// into `date_uploaded` by `convert::parse_release_time`.
    #[test]
    fn parse_and_convert_novel_normalizes_space_separated_dates_ranobes_style() {
        let mut runtime = novel_runtime(RANOBES_STYLE_PAGINATED_MAIN_JS);

        let (_, chapters) = parse_and_convert_novel(&mut runtime, "synthetic", "manga-x")
            .expect("ranobes-style pagination must not fail: the Date shim must accept the space-separated date");

        // 1 (page 1) + 1 (page 2) = 2 chapters, both pages loaded.
        assert_eq!(chapters.len(), 2, "page 2 must be loaded, not skipped");

        let p2 = chapters
            .iter()
            .find(|c| c.id == "p2-c2")
            .expect("page-2 chapter must be present in the paginated list");

        // `new Date('2021-06-27 02:06:47').toISOString()` produced a real ISO
        // timestamp (T separator, Z suffix) instead of throwing — the very
        // behavior the shim restores. The exact instant is not asserted (boa
        // parses the T-form as local time, so `.toISOString()`'s output
        // depends on the host timezone).
        let name = p2
            .title
            .as_deref()
            .expect("page-2 chapter should have a name");
        let iso = name
            .strip_prefix("Chapter 2 (")
            .and_then(|s| s.strip_suffix(')'))
            .expect("page-2 chapter name should embed the toISOString() output");
        assert!(
            iso.contains('T') && iso.ends_with('Z'),
            "expected the plugin's toISOString() output in the chapter name, got: {iso}"
        );

        // And convert.rs stored that RFC3339 string as a real timestamp.
        assert!(
            p2.date_uploaded.is_some(),
            "page-2 releaseTime (RFC3339 ISO) must be parsed into date_uploaded"
        );
    }

    /// The worker's `JsRuntime` is persistent across calls (see [`run`]'s doc
    /// comment): only the settings snapshot is refreshed between calls, and
    /// the recombined `{key}__include`/`{key}__exclude` pairs must reach
    /// `searchNovels` on *every* call — including after a refresh.
    #[test]
    fn search_applies_recombined_filters_after_snapshot_refresh() {
        let main_js = SEARCH_REPORTS_FILTERS_MAIN_JS.to_string();
        let mut runtime = js_runtime::new(
            vec_settings(&[("genres__include", vec!["Fantasy"])]),
            &main_js,
        )
        .expect("runtime construction should not fail");

        // First call on the freshly built runtime: the include pair is
        // recombined and visible to `searchNovels`.
        let first = execute_search_mangas(&mut runtime, "synthetic", "q".into(), 1)
            .expect("search should not fail");
        let first_title = first.mangas.expect("mangas present").remove(0).title;
        assert_eq!(first_title.as_deref(), Some("q|Fantasy|"));

        // The runtime persists; only the settings snapshot is refreshed. The
        // next search must see the new filter values.
        runtime.update_settings_snapshot(vec_settings(&[
            ("genres__include", vec!["Sci-Fi", "Adventure"]),
            ("genres__exclude", vec!["Horror"]),
        ]));
        let second = execute_search_mangas(&mut runtime, "synthetic", "q".into(), 1)
            .expect("search should not fail");
        let second_title = second.mangas.expect("mangas present").remove(0).title;
        assert_eq!(second_title.as_deref(), Some("q|Sci-Fi+Adventure|Horror"));

        // A refresh with the pair explicitly emptied resets the filter to its
        // default state.
        runtime.update_settings_snapshot(vec_settings(&[
            ("genres__include", vec![]),
            ("genres__exclude", vec![]),
        ]));
        let third = execute_search_mangas(&mut runtime, "synthetic", "q".into(), 1)
            .expect("search should not fail");
        let third_title = third.mangas.expect("mangas present").remove(0).title;
        assert_eq!(third_title.as_deref(), Some("q||"));
    }
}
