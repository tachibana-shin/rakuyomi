//! Keiyoushi (mihon/Tachiyomi) extension backend.
//!
//! Loads a single `*.keiyoushi.apk` extension (as published by the
//! keiyoushi/extensions repo), runs its DEX bytecode inside the embedded
//! `dexvm` interpreter and exposes every source bundled in the APK through
//! the same [`Source`](crate::source::Source) interface as WASM, LNReader
//! and MangaYomi sources.
//!
//! One APK usually bundles a single source (its [`SourceId`] is then the
//! extension package name); multi-source "all" APKs register one source per
//! bundled language as `<pkg>:<lang>`.
//!
//! Method calls are synchronous from the outside: the extension VM is booted
//! once per source and driven by a dedicated worker thread, like the
//! LNReader runtime and the MangaYomi provider. The engine is `!Send`
//! (dexvm uses `Rc` internally), so it never leaves the worker thread;
//! callers send typed requests through a channel and block on the reply
//! with a timeout, and a wedged worker is restarted on the next call.

pub mod model;

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context as _, Result};
use dexvm::context::SettingValue;
use dexvm::keiyoushi::{HttpData, HttpResp, Keiyoushi};
use dexvm::vm::error::JvmError;
use reqwest::{header::HeaderMap, Method, Request};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    settings::SourceSettingValue,
    source::{
        model::{Chapter, Manga, Page, SettingDefinition},
        source_settings::SourceSettings,
        BlockingSource, SourceFeatures, SourceInfo, SourceManifest, SourceMeta,
    },
    source_manager::SourceManager,
    util::DEFAULT_USER_AGENT,
};

/// The suffix of installed extension APKs (`<pkg>.keiyoushi.apk`).
pub(crate) const KEIYOUSHI_FILE_SUFFIX: &str = ".keiyoushi.apk";

/// The suffix of the probe cache sidecar (`<pkg>.keiyoushi.probe.json`),
/// which records the metadata collected by booting the APK once at install
/// time so later loads can skip the boot entirely.
pub(crate) const KEIYOUSHI_PROBE_SUFFIX: &str = ".keiyoushi.probe.json";

/// How long a single HTTP request issued through the extension may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);

/// A single source bundled inside a keiyoushi extension APK, exposed through
/// the RakuYomi source API.
pub struct KeiyoushiSource {
    pub id: String,
    pub manifest: SourceManifest,
    pub setting_definitions: Vec<SettingDefinition>,
    pub features: SourceFeatures,
    pub base_url: String,
    pub name: String,
    pub lang: String,
    pub supports_latest: bool,
    /// Path of the extension APK on disk. The APK is only read from disk
    /// when the engine is booted on first use, so installed extensions do
    /// not hold their (potentially tens of MB) bytes in memory.
    apk_path: PathBuf,
    /// Which source of the APK (`createSources()` index) this instance is.
    source_index: usize,
    /// Merged source settings (stored values overlaid on the extension
    /// preference defaults), seeded into the engine once at boot. The mutex
    /// mirrors the `Arc<Mutex<BlockingSource>>` wrapper of the wasm backend:
    /// [`SourceSettings`] is `RefCell`-based and single-threaded, the lock
    /// makes the source shareable across `spawn_blocking`.
    settings: Arc<Mutex<SourceSettings>>,
    /// The worker driving the engine, spawned lazily on the first call and
    /// restarted when it wedges or dies, like the LNReader/MangaYomi
    /// runtimes. The engine itself never leaves the worker thread.
    worker: Mutex<Option<WorkerHandle>>,
}

impl std::fmt::Debug for KeiyoushiSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeiyoushiSource")
            .field("id", &self.id)
            .field("manifest", &self.manifest)
            .field("setting_definitions", &self.setting_definitions)
            .field("name", &self.name)
            .field("lang", &self.lang)
            .field("base_url", &self.base_url)
            .field("supports_latest", &self.supports_latest)
            .field(
                "worker_alive",
                &self
                    .worker
                    .lock()
                    .is_ok_and(|w| w.as_ref().is_some_and(|h| !h.thread.is_finished())),
            )
            .finish()
    }
}

/// The engine of one source: the dexvm VM plus the per-call bookkeeping it
/// needs. Lives on the worker thread only (dexvm uses `Rc` internally and
/// is `!Send`).
struct KeiyoushiEngine {
    ext: Keiyoushi,
    /// The bundled source this instance maps to (arena id, stable for the
    /// context lifetime).
    source: dexvm::keiyoushi::Source,
    /// URL of the last request the extension made, used to absolutise
    /// relative manga/page URLs. Reset before every call.
    last_url: Rc<RefCell<Option<String>>>,
    /// Chapter id whose `getPageList` decrypt grants are currently stashed
    /// in the VM. Image fetches skip the re-parse while it matches, so a
    /// chapter parses its page list exactly once.
    page_list_chapter: Option<String>,
}

/// Request dispatched to the engine worker thread.
struct WorkerRequest {
    kind: RequestKind,
    reply: Sender<Result<KeiyoushiReply, String>>,
}

/// The typed engine call to execute on the worker.
#[derive(Clone)]
enum RequestKind {
    MangaList { use_latest: bool },
    Search { query: String, page: i32 },
    MangaDetails { manga_id: String },
    ChapterList { manga_id: String },
    PageList { chapter_id: String },
    Image { chapter_id: String, url: String },
}

/// Typed result of an engine call, carried back across the channel.
enum KeiyoushiReply {
    Mangas { mangas: Vec<Manga>, has_next: bool },
    Manga(Box<Manga>),
    Chapters(Vec<Chapter>),
    Pages(Vec<Page>),
    Image(Vec<u8>),
}

/// Handle of one engine worker thread.
struct WorkerHandle {
    tx: Sender<WorkerRequest>,
    thread: JoinHandle<()>,
}

/// How long a single engine call may run before it is aborted.
const DEFAULT_INVOKE_TIMEOUT: Duration = Duration::from_secs(60);

/// The result of booting an extension APK once: the manifest package id,
/// the sources it bundles and the preference definitions it materialises.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct ApkProbe {
    /// Canonical `manifest` package id (e.g.
    /// `eu.kanade.tachiyomi.extension.vi.cuutruyenmoe`).
    package_id: String,
    /// The `AndroidManifest` `versionName` (e.g. `1.6.8`), when declared.
    /// `serde(default)` keeps probe caches written before this field was
    /// introduced readable.
    #[serde(default)]
    version_name: Option<String>,
    sources: Vec<(String, String, bool)>,
    setting_definitions: Vec<SettingDefinition>,
}

/// On-disk probe cache (`<pkg>.keiyoushi.probe.json`). The APK fingerprint
/// (length + mtime) guards against stale metadata when the extension is
/// updated or replaced outside the install pipeline.
#[derive(Serialize, Deserialize)]
struct ProbeCache {
    apk_len: u64,
    apk_mtime_ns: u128,
    probe: ApkProbe,
}

fn probe_cache_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let stem = name.strip_suffix(KEIYOUSHI_FILE_SUFFIX).unwrap_or(name);
    path.with_file_name(format!("{stem}{KEIYOUSHI_PROBE_SUFFIX}"))
}

/// Returns the cached probe when it matches the APK fingerprint on disk.
fn read_probe_cache(path: &Path, apk_len: u64, apk_mtime_ns: u128) -> Option<ApkProbe> {
    let contents = fs::read_to_string(probe_cache_path(path)).ok()?;
    let cache: ProbeCache = serde_json::from_str(&contents).ok()?;
    (cache.apk_len == apk_len && cache.apk_mtime_ns == apk_mtime_ns).then_some(cache.probe)
}

/// Persists the probe next to the APK. Failures are logged, never fatal:
/// the next load simply re-boots.
fn write_probe_cache(path: &Path, apk_len: u64, apk_mtime_ns: u128, probe: &ApkProbe) {
    let cache = ProbeCache {
        apk_len,
        apk_mtime_ns,
        probe: probe.clone(),
    };
    let result = serde_json::to_vec(&cache)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| fs::write(probe_cache_path(path), bytes).map_err(anyhow::Error::from));
    if let Err(err) = result {
        log::warn!("failed to write probe cache for {}: {err}", path.display());
    }
}

/// Boots the extension APK once, wiring its HTTP callback to the RakuYomi
/// cookie/UA store through a blocking reqwest client, and seeds the stored
/// settings into the extension's in-memory preferences.
fn boot_engine(
    apk_path: &Path,
    source_index: usize,
    settings: &Arc<Mutex<SourceSettings>>,
) -> Result<KeiyoushiEngine> {
    let bytes = fs::read(apk_path)
        .with_context(|| format!("failed to read extension file {}", apk_path.display()))?;
    let mut ext =
        Keiyoushi::new(&bytes).map_err(|e| anyhow!("failed to boot keiyoushi extension: {e}"))?;

    let client = crate::tls::blocking_client_builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .context("failed to build HTTP client for keiyoushi extension")?;

    let last_url = Rc::new(RefCell::new(None::<String>));
    let recorded = last_url.clone();
    let http_client = client.clone();
    ext.set_http(move |req: &HttpData| -> HttpResp {
        execute_request(&http_client, req, &recorded)
    });
    ext.set_host_headers(crate::cookie_store::get_user_agent_and_cookie_header);

    let sources = ext.sources().map_err(|e| {
        anyhow!(
            "keiyoushi extension has no sources: {}",
            ext.describe_error(&e)
        )
    })?;
    let source = *sources.get(source_index).ok_or_else(|| {
        anyhow!(
            "keiyoushi extension source index {} is out of bounds ({} sources)",
            source_index,
            sources.len()
        )
    })?;

    // The stored settings are seeded once into the extension's in-memory
    // preferences (mihon's `preferenceKey() = "source_<id>"`). Changes the
    // extension makes through `SharedPreferences$Editor` are mirrored back
    // into RakuYomi's settings store via `on_update_settings`, so later
    // boots seed the updated values.
    let all_settings = { settings.lock().unwrap_or_else(|e| e.into_inner()).all() };
    let mut preferences = HashMap::new();
    for (key, value) in all_settings.iter() {
        if let Some(pref) = setting_value_to_pref(value) {
            preferences.insert(key.clone(), pref);
        }
    }
    let settings_store = settings.clone();
    ext.on_update_settings(move |key, value| {
        if let Some(value) = pref_to_setting_value(value) {
            if let Err(err) = settings_store
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .save(key, value)
            {
                log::warn!("keiyoushi: failed to persist setting `{key}`: {err}");
            }
        }
    });
    let prefs_file = ext.preference_file(&source).map_err(|e| {
        anyhow!(
            "keiyoushi: failed to resolve source preference file: {}",
            ext.describe_error(&e)
        )
    })?;
    ext.seed_preferences(&prefs_file, &preferences);

    Ok(KeiyoushiEngine {
        ext,
        source,
        last_url,
        page_list_chapter: None,
    })
}

/// The engine worker: boots the VM once and executes every queued request
/// on this thread. Exits when the channel closes (the source was dropped
/// or the worker handle was replaced after a timeout).
fn worker_loop(
    rx: Receiver<WorkerRequest>,
    apk_path: &Path,
    source_index: usize,
    settings: &Arc<Mutex<SourceSettings>>,
    source_id: &str,
) -> Result<()> {
    let mut engine = boot_engine(apk_path, source_index, settings)?;
    log::debug!("keiyoushi worker started for {source_id}");
    while let Ok(request) = rx.recv() {
        *engine.last_url.borrow_mut() = None;
        let result = handle_request(&mut engine, source_id, request.kind);
        let _ = request.reply.send(result);
    }
    Ok(())
}

/// Executes one typed engine call, absolutising the extension's results
/// against the URL of its last request.
fn handle_request(
    engine: &mut KeiyoushiEngine,
    source_id: &str,
    kind: RequestKind,
) -> Result<KeiyoushiReply, String> {
    let result = match kind {
        RequestKind::MangaList { use_latest } => if use_latest {
            call_fallback(
                "getLatestUpdates",
                &mut engine.ext,
                &engine.source,
                |ext, src, page| ext.latest_coro(src, page),
                |ext, src, page| ext.latest(src, page),
                1,
            )
        } else {
            call_fallback(
                "getPopularManga",
                &mut engine.ext,
                &engine.source,
                |ext, src, page| ext.popular_coro(src, page),
                |ext, src, page| ext.popular(src, page),
                1,
            )
        }
        .map(|pages| KeiyoushiReply::Mangas {
            mangas: model::mangas_from_page(source_id, base(engine).as_ref(), pages.mangas),
            has_next: false,
        }),
        RequestKind::Search { query, page } => {
            if query.is_empty() {
                // An empty query means "browse", which the extensions expose
                // through the popular listing.
                call_fallback(
                    "getPopularManga",
                    &mut engine.ext,
                    &engine.source,
                    |ext, src, _| ext.popular_coro(src, page.max(1)),
                    |ext, src, _| ext.popular(src, page.max(1)),
                    0,
                )
            } else {
                call_fallback(
                    "getSearchManga",
                    &mut engine.ext,
                    &engine.source,
                    |ext, src, q: &str| ext.search_coro(src, page.max(1), q, &[]),
                    |ext, src, q: &str| ext.search(src, page.max(1), q, &[]),
                    query.as_str(),
                )
            }
            .map(|pages| KeiyoushiReply::Mangas {
                mangas: model::mangas_from_page(source_id, base(engine).as_ref(), pages.mangas),
                has_next: pages.has_next,
            })
        }
        RequestKind::MangaDetails { manga_id } => {
            let manga = dexvm::keiyoushi::Manga {
                url: manga_id.clone(),
                title: manga_id.clone(),
                ..Default::default()
            };
            call_fallback(
                "getMangaUpdate",
                &mut engine.ext,
                &engine.source,
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.manga_update_details(src, m),
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.manga_details(src, m),
                &manga,
            )
            .map(|manga| {
                KeiyoushiReply::Manga(Box::new(model::manga_from_keiyoushi(
                    source_id,
                    base(engine).as_ref(),
                    manga,
                )))
            })
        }
        RequestKind::ChapterList { manga_id } => {
            let manga = dexvm::keiyoushi::Manga {
                url: manga_id.clone(),
                title: manga_id.clone(),
                ..Default::default()
            };
            call_fallback(
                "getMangaUpdate",
                &mut engine.ext,
                &engine.source,
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.manga_update_chapters(src, m),
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.chapters(src, m),
                &manga,
            )
            .map(|chapters| {
                let mut out = model::chapters_from_keiyoushi(
                    source_id,
                    &manga_id,
                    base(engine).as_ref(),
                    chapters,
                );
                crate::source::model::normalize_chapter_order(&mut out);
                KeiyoushiReply::Chapters(out)
            })
        }
        RequestKind::PageList { chapter_id } => {
            let chapter = dexvm::keiyoushi::Chapter {
                url: chapter_id.clone(),
                name: chapter_id.clone(),
                ..Default::default()
            };
            let pages = call_fallback(
                "getPageList",
                &mut engine.ext,
                &engine.source,
                |ext, src, c: &dexvm::keiyoushi::Chapter| ext.pages_coro(src, c),
                |ext, src, c: &dexvm::keiyoushi::Chapter| ext.pages(src, c),
                &chapter,
            );
            if std::env::var("DEXVM_TRACE").is_ok() {
                eprintln!(
                    "DEXVM_TRACE keiyoushi get_page_list: raw={}",
                    pages.as_ref().map_or(0, |p| p.len())
                );
            }
            pages.map(|pages| {
                // The decrypt grants stashed during this parse survive in the
                // persistent VM, so image fetches of this chapter skip the
                // re-parse.
                engine.page_list_chapter = Some(chapter_id.clone());
                KeiyoushiReply::Pages(model::pages_from_keiyoushi(
                    source_id,
                    &chapter_id,
                    base(engine).as_ref(),
                    pages,
                ))
            })
        }
        RequestKind::Image { chapter_id, url } => {
            let chapter = dexvm::keiyoushi::Chapter {
                url: chapter_id.clone(),
                name: chapter_id.clone(),
                ..Default::default()
            };
            if engine.page_list_chapter.as_deref() != Some(chapter_id.as_str()) {
                call_fallback(
                    "getPageList",
                    &mut engine.ext,
                    &engine.source,
                    |ext, src, c: &dexvm::keiyoushi::Chapter| ext.pages_coro(src, c),
                    |ext, src, c: &dexvm::keiyoushi::Chapter| ext.pages(src, c),
                    &chapter,
                )
                .map(|_| engine.page_list_chapter = Some(chapter_id))
            } else {
                Ok(())
            }
            .and_then(|()| {
                engine
                    .ext
                    .image_data(&engine.source, &url)
                    .map(KeiyoushiReply::Image)
            })
        }
    };
    result.map_err(|e| {
        format!(
            "keiyoushi extension call failed: {}",
            engine.ext.describe_error(&e)
        )
    })
}

/// Parses the URL of the last request the extension made, used to
/// absolutise relative manga/page URLs.
fn base(engine: &KeiyoushiEngine) -> Option<Url> {
    engine
        .last_url
        .borrow()
        .as_deref()
        .and_then(|url| Url::parse(url).ok())
}

impl KeiyoushiSource {
    /// Boots the APK at `path` and returns one source per bundled `Source`.
    ///
    /// The SourceId is the extension package name when the APK bundles a
    /// single source, or `<pkg>:<lang>` per source otherwise; the id scheme
    /// is stable so stored settings, library entries and downloaded chapters
    /// keep working across reloads.
    pub fn from_keiyoushi_apk(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Vec<Self>> {
        // The APK is only booted when the probe cache is missing or stale;
        // booting happens again on the first actual call (`with_engine`),
        // so load time never runs the VM.
        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to stat extension file {}", path.display()))?;
        let apk_len = metadata.len();
        let apk_mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());
        let probe = match apk_mtime_ns.and_then(|mtime| read_probe_cache(path, apk_len, mtime)) {
            Some(probe) => probe,
            None => {
                let bytes = fs::read(path)
                    .with_context(|| format!("failed to read extension file {}", path.display()))?;
                let probe = probe_apk(&bytes)?;
                if let Some(mtime) = apk_mtime_ns {
                    write_probe_cache(path, apk_len, mtime, &probe);
                }
                probe
            }
        };

        // The canonical package id comes from the manifest; the file name is
        // only a fallback for containers without one.
        let pkg = if probe.package_id.is_empty() {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(KEIYOUSHI_FILE_SUFFIX))
                .filter(|name| !name.is_empty())
                .context("keiyoushi APK file name is missing the package name")?
                .to_string()
        } else {
            probe.package_id.clone()
        };

        let source_of_source = {
            let meta_file = BlockingSource::meta_source_path(path)?;
            let mut source_of_source = None;
            if meta_file.exists() {
                let meta: SourceMeta = serde_json::from_str(
                    &fs::read_to_string(&meta_file)
                        .with_context(|| format!("failed to read meta file {:?}", meta_file))?,
                )?;
                source_of_source = meta.source_of_source;
            }
            source_of_source
        };

        let single = probe.sources.len() == 1;
        let mut out = Vec::with_capacity(probe.sources.len());
        for (index, (name, lang, supports_latest)) in probe.sources.iter().enumerate() {
            let id = if single {
                pkg.to_string()
            } else {
                format!("{pkg}:{lang}")
            };
            let stored_settings = manager
                .settings
                .source_settings
                .get(&id)
                .cloned()
                .unwrap_or_default();
            let settings = Arc::new(Mutex::new(SourceSettings::new(
                id.clone(),
                &probe.setting_definitions,
                &stored_settings,
                arc_manager,
            )?));
            let manifest = SourceManifest {
                info: SourceInfo {
                    id: id.clone(),
                    lang: Some(lang.clone()),
                    languages: None,
                    #[cfg(not(feature = "all"))]
                    content_rating: None,
                    name: name.clone(),
                    version: Value::String(
                        probe
                            .version_name
                            .clone()
                            .unwrap_or_else(|| "1".to_string()),
                    ),
                    url: None,
                    urls: None,
                    min_app_version: None,
                },
                config: None,
                source_of_source: source_of_source.clone(),
            };
            out.push(Self {
                id,
                manifest,
                setting_definitions: probe.setting_definitions.clone(),
                features: SourceFeatures {
                    process_page_image: false,
                },
                base_url: String::new(),
                name: name.clone(),
                lang: lang.clone(),
                supports_latest: *supports_latest,
                apk_path: path.to_path_buf(),
                source_index: index,
                settings,
                worker: Mutex::new(None),
            });
        }
        Ok(out)
    }

    /// Runs one engine call on the worker thread, starting (or restarting)
    /// the worker on demand. Blocks until the worker replies or the call
    /// times out; a wedged worker is restarted for the next call.
    fn invoke(&self, kind: RequestKind) -> Result<KeiyoushiReply> {
        let mut attempts = 0;
        loop {
            let reply_rx = {
                let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
                let needs_restart = worker
                    .as_ref()
                    .map(|w| w.thread.is_finished())
                    .unwrap_or(true);
                if needs_restart {
                    *worker = None;
                    drop(worker);
                    self.start_worker()?;
                    continue;
                }
                let (reply_tx, reply_rx) = channel();
                worker
                    .as_ref()
                    .unwrap()
                    .tx
                    .send(WorkerRequest {
                        kind: kind.clone(),
                        reply: reply_tx,
                    })
                    .context("failed to send request to keiyoushi worker")?;
                reply_rx
            };

            match reply_rx.recv_timeout(DEFAULT_INVOKE_TIMEOUT) {
                Ok(result) => return result.map_err(anyhow::Error::msg),
                Err(_) => {
                    // The worker is stuck (e.g. a dexvm bug or a pathological
                    // extension). Restart it so the next call works.
                    log::warn!("keiyoushi extension call timed out; restarting the worker");
                    let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
                    *worker = None;
                    attempts += 1;
                    if attempts >= 2 {
                        bail!("keiyoushi extension call timed out twice");
                    }
                }
            }
        }
    }

    /// Spawns the engine worker thread. The engine boots inside it (the
    /// dexvm VM is `!Send`), so the APK bytes, VM state and per-call
    /// bookkeeping never leave this thread.
    fn start_worker(&self) -> Result<()> {
        let apk_path = self.apk_path.clone();
        let source_index = self.source_index;
        let settings = self.settings.clone();
        let source_id = self.id.clone();

        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name(format!("keiyoushi-worker-{source_id}"))
            .spawn(move || {
                if let Err(err) = worker_loop(rx, &apk_path, source_index, &settings, &source_id) {
                    log::warn!("keiyoushi worker exited: {:#}", err);
                }
            })
            .context("failed to spawn keiyoushi worker thread")?;
        *self.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(WorkerHandle { tx, thread });
        Ok(())
    }

    /// Implements `get_manga_list`: `popular` (or `latest` for the "latest"
    /// listing), page 1.
    pub fn get_manga_list(
        &self,
        _cancellation_token: CancellationToken,
        listing: aidoku::Listing,
    ) -> Result<Vec<Manga>> {
        let use_latest = listing.name.eq_ignore_ascii_case("latest") && self.supports_latest;
        match self.invoke(RequestKind::MangaList { use_latest })? {
            KeiyoushiReply::Mangas { mangas, .. } => Ok(mangas),
            _ => unreachable!("manga list request must reply with mangas"),
        }
    }

    /// Implements `search_mangas`. The keiyoushi filter list is not exposed
    /// through the RakuYomi search UI, so searches run with default filter
    /// states (which is what a first browse does in the original app too).
    pub fn search_mangas(
        &self,
        _cancellation_token: CancellationToken,
        query: String,
        page: i32,
    ) -> Result<(Vec<Manga>, bool)> {
        let query = query.trim().to_string();
        match self.invoke(RequestKind::Search { query, page })? {
            KeiyoushiReply::Mangas { mangas, has_next } => Ok((mangas, has_next)),
            _ => unreachable!("search request must reply with mangas"),
        }
    }

    /// Implements `get_manga_details` from a raw manga URL.
    pub fn get_manga_details(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Manga> {
        match self.invoke(RequestKind::MangaDetails { manga_id })? {
            KeiyoushiReply::Manga(manga) => Ok(*manga),
            _ => unreachable!("manga details request must reply with a manga"),
        }
    }

    /// Implements `get_chapter_list` from a raw manga URL.
    pub fn get_chapter_list(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<Chapter>> {
        match self.invoke(RequestKind::ChapterList { manga_id })? {
            KeiyoushiReply::Chapters(chapters) => Ok(chapters),
            _ => unreachable!("chapter list request must reply with chapters"),
        }
    }

    /// Implements `get_page_list` from a raw chapter URL.
    pub fn get_page_list(
        &self,
        _cancellation_token: CancellationToken,
        _manga_id: String,
        chapter_id: String,
        _chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        match self.invoke(RequestKind::PageList { chapter_id })? {
            KeiyoushiReply::Pages(pages) => Ok(pages),
            _ => unreachable!("page list request must reply with pages"),
        }
    }

    /// Fetches a page image through the extension's own OkHttpClient so the
    /// extension's client-side interceptors (IMGX-style decryption, per-host
    /// auth) run, exactly like the `getClient()` -> `newCall()` ->
    /// `execute()` path mihon uses. Extensions stash per-image decrypt
    /// grants during `getPageList`; the persistent engine keeps them, so
    /// the page list is only parsed once per chapter (tracked by
    /// [`KeiyoushiEngine::page_list_chapter`]) and every image of the
    /// chapter reuses those grants.
    pub fn fetch_page_image(&self, chapter_id: &str, url: &str) -> Result<Vec<u8>> {
        match self.invoke(RequestKind::Image {
            chapter_id: chapter_id.to_string(),
            url: url.to_string(),
        })? {
            KeiyoushiReply::Image(bytes) => Ok(bytes),
            _ => unreachable!("image request must reply with bytes"),
        }
    }

    /// Implements `get_image_request`: image URLs carry their own
    /// authentication parameters, so a plain GET with the shared
    /// user-agent and the per-domain cookie store is enough.
    ///
    /// # TODO
    /// maybe is deadcode because matches in chapter_downloader only call fetch_page_image
    pub fn get_image_request(
        &self,
        url: Url,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        let mut builder = crate::tls::client_builder()
            .build()
            .context("failed to build HTTP client")?
            .request(Method::GET, url.clone());
        let mut header_map = HeaderMap::new();
        header_map.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(DEFAULT_USER_AGENT),
        );
        if let Some(host) = url.host_str() {
            let (_override_ua, cookie_value) =
                crate::cookie_store::get_user_agent_and_cookie_header(host);
            if let Some(cookies) = cookie_value {
                if let Ok(header) = reqwest::header::HeaderValue::from_str(&cookies) {
                    header_map.insert(reqwest::header::COOKIE, header);
                }
            }
        }
        builder = builder.headers(header_map);
        let request = builder
            .build()
            .with_context(|| format!("failed to build image request for {}", url))?;
        Ok(request)
    }

    /// Keiyoushi extensions have no image processing step.
    pub fn process_page_image(
        &self,
        _cancellation_token: CancellationToken,
        _request: (Url, HeaderMap),
        _response: (reqwest::StatusCode, HeaderMap),
        _bytes: tokio_util::bytes::Bytes,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Vec<u8>> {
        bail!("process_page_image is not supported by Keiyoushi extensions")
    }

    // next-SDK shaped helpers, uniform with the other backends.

    pub fn get_manga_list_next(
        &self,
        cancellation_token: CancellationToken,
        listing: aidoku::Listing,
        _page: i32,
    ) -> Result<crate::source::NextMangaPageResult> {
        let mangas = self.get_manga_list(cancellation_token, listing)?;
        Ok(crate::source::NextMangaPageResult {
            entries: mangas.into_iter().map(manga_to_aidoku).collect(),
            has_next_page: false,
        })
    }

    pub fn get_search_manga_list_next(
        &self,
        cancellation_token: CancellationToken,
        query: String,
        page: i32,
        _filters: Vec<aidoku::FilterValue>,
    ) -> Result<crate::source::NextMangaPageResult> {
        let (mangas, has_next_page) = self.search_mangas(cancellation_token, query, page)?;
        Ok(crate::source::NextMangaPageResult {
            entries: mangas.into_iter().map(manga_to_aidoku).collect(),
            has_next_page,
        })
    }

    pub fn get_manga_update_next(
        &self,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        needs_details: bool,
        _needs_chapters: bool,
    ) -> Result<aidoku::Manga> {
        if needs_details {
            let updated = self.get_manga_details(cancellation_token, manga.key)?;
            Ok(manga_to_aidoku(updated))
        } else {
            Ok(manga)
        }
    }

    pub fn get_page_list_next(
        &self,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        chapter: aidoku::Chapter,
    ) -> Result<Vec<aidoku::Page>> {
        let pages = self.get_page_list(cancellation_token, manga.key, chapter.key, None)?;
        Ok(pages
            .into_iter()
            .map(|page| aidoku::Page {
                content: match page.image_url {
                    Some(url) => aidoku::PageContent::Url(url.to_string(), page.ctx),
                    None => aidoku::PageContent::Text(page.text.unwrap_or_default()),
                },
                thumbnail: None,
                has_description: false,
                description: None,
            })
            .collect())
    }

    pub fn get_image_request_next(
        &self,
        url: Url,
        ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        self.get_image_request(url, ctx)
    }

    /// Notifications are not supported by Keiyoushi extensions.
    pub fn handle_notification_next(
        &self,
        _cancellation_token: CancellationToken,
        _key: String,
    ) -> Result<()> {
        Ok(())
    }
}

/// Converts a rakuyomi manga to the aidoku representation used by the
/// next-SDK shaped results.
fn manga_to_aidoku(manga: Manga) -> aidoku::Manga {
    aidoku::Manga {
        key: manga.id.clone(),
        title: manga.title.unwrap_or_default(),
        cover: manga.cover_url.map(|u| u.to_string()),
        artists: manga.artist.map(|a| vec![a]),
        authors: manga.author.map(|a| vec![a]),
        description: manga.description,
        url: manga.url.map(|u| u.to_string()),
        tags: manga.tags,
        status: aidoku::MangaStatus::Unknown,
        content_rating: aidoku::ContentRating::Unknown,
        viewer: aidoku::Viewer::Unknown,
        update_strategy: aidoku::UpdateStrategy::Never,
        next_update_time: None,
        chapters: None,
    }
}

/// Boots an extension APK once and collects the metadata shared by all of
/// its bundled sources: the manifest package id, the source names/languages
/// and the preference definitions.
fn probe_apk(bytes: &[u8]) -> Result<ApkProbe> {
    // No shared-preferences path is set, so the boot stays read-only and
    // preferences remain in memory.
    let mut ext = Keiyoushi::new(bytes).map_err(|e| anyhow!("failed to boot extension: {e}"))?;
    let manifest = ext.manifest();
    let package_id = manifest
        .as_ref()
        .map(|m| m.package_id.clone())
        .unwrap_or_default();
    let version_name = manifest.ok().and_then(|m| m.version_name);
    let sources = ext.sources()?;
    if sources.is_empty() {
        bail!("keiyoushi extension bundles no sources");
    }
    let mut out = Vec::with_capacity(sources.len());
    for source in &sources {
        let name = ext.source_name(source)?;
        let lang = ext.source_lang(source)?;
        let supports_latest = ext.supports_latest(source)?;
        out.push((name, lang, supports_latest));
    }
    let defs = ext
        .preference_definitions(&sources[0])
        .unwrap_or_default()
        .into_iter()
        .filter_map(setting_definition_from_dexvm)
        .collect();
    Ok(ApkProbe {
        package_id,
        version_name,
        sources: out,
        setting_definitions: defs,
    })
}

/// Runs a coroutine-style (`getPopularManga` & friends) keiyoushi call,
/// falling back to the classic request/parse pair when the extension only
/// implements the legacy interfaces. The fallback triggers on the resolution
/// error naming the missing coroutine method; genuine runtime failures
/// propagate as-is.
///
/// `ext` and `src` are passed as arguments to the closures (instead of being
/// captured) so the two fallback implementations can share the engine.
fn call_fallback<T, A>(
    coro_name: &str,
    ext: &mut Keiyoushi,
    src: &dexvm::keiyoushi::Source,
    coro: impl FnOnce(&mut Keiyoushi, &dexvm::keiyoushi::Source, A) -> Result<T, JvmError>,
    classic: impl FnOnce(&mut Keiyoushi, &dexvm::keiyoushi::Source, A) -> Result<T, JvmError>,
    arg: A,
) -> Result<T, JvmError>
where
    A: Clone,
{
    match coro(ext, src, arg.clone()) {
        Ok(result) => {
            if std::env::var("DEXVM_TRACE").is_ok() {
                eprintln!("DEXVM_TRACE call_fallback {coro_name}: coro Ok");
            }
            Ok(result)
        }
        Err(JvmError::Resolution(message)) if message.contains(coro_name) => {
            if std::env::var("DEXVM_TRACE").is_ok() {
                eprintln!(
                    "DEXVM_TRACE call_fallback {coro_name}: coro failed ({message}) -> classic"
                );
            }
            classic(ext, src, arg)
        }
        Err(other) => Err(other),
    }
}

/// Executes a request captured by the extension with the blocking reqwest
/// client and maps the response back onto the okhttp representation.
fn execute_request(
    client: &reqwest::blocking::Client,
    req: &HttpData,
    recorded: &Rc<RefCell<Option<String>>>,
) -> HttpResp {
    *recorded.borrow_mut() = Some(req.url.clone());

    // Mangadex API regression: `includeEmptyPages=0` on the chapter feed
    // currently returns an empty result instead of the default exclusion of
    // empty-paged chapters. Dropping the (default-valued) parameter preserves
    // the extension's intent while keeping the request functional.
    let mut url = req.url.clone();
    if url.contains("includeEmptyPages=0") {
        url = url.replace("includeEmptyPages=0", "");
        url = url.replace("&&", "&");
        if url.ends_with('?') {
            url.pop();
        }
    }

    let method = req.method.to_uppercase();
    let mut builder = match method.as_str() {
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "PATCH" => client.patch(&url),
        "DELETE" => client.delete(&url),
        "HEAD" => client.head(&url),
        _ => client.get(&url),
    };
    for (name, value) in &req.headers {
        let lower = name.to_ascii_lowercase();
        // Hop-by-hop headers are managed by reqwest itself.
        if matches!(
            lower.as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding" | "accept-encoding"
        ) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            builder = builder.header(name, value);
        }
    }
    if let Some(body) = req.body.as_deref() {
        if !matches!(method.as_str(), "GET" | "HEAD") && !body.is_empty() {
            builder = builder.body(body.to_string());
        }
    }
    let request = match builder.build() {
        Ok(request) => request,
        Err(_) => {
            return HttpResp {
                code: 0,
                message: "invalid request built by extension".to_string(),
                headers: Vec::new(),
                body: None,
            }
        }
    };
    match client.execute(request) {
        Ok(response) => {
            // After redirects the final URL is the document base the
            // extension parses relative URLs against.
            *recorded.borrow_mut() = Some(response.url().to_string());
            // `Set-Cookie` headers land in the shared RakuYomi store (the
            // single cookie source, like reqwest's cookie jar): the
            // "enter-secret" gate flow depends on the session cookie set by
            // the gate response being sent on the retried request, which
            // the per-request host header resolver picks up from there.
            crate::cookie_store::record_response_cookies(&response);
            let code = response.status().as_u16() as i32;
            let message = response
                .status()
                .canonical_reason()
                .unwrap_or_default()
                .to_string();
            let headers: Vec<(String, String)> = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        value.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect();
            let body = response.bytes().ok().map(|bytes| bytes.to_vec());
            HttpResp {
                code,
                message,
                headers,
                body,
            }
        }
        Err(_) => HttpResp {
            code: 0,
            message: "network error".to_string(),
            headers: Vec::new(),
            body: None,
        },
    }
}

/// Converts a stored RakuYomi source setting into the dexvm preference
/// representation. Unmappable values (blobs, vectors) are skipped.
fn setting_value_to_pref(value: &SourceSettingValue) -> Option<SettingValue> {
    Some(match value {
        SourceSettingValue::Bool(value) => SettingValue::Bool(*value),
        SourceSettingValue::Int(value) => SettingValue::Long(*value),
        SourceSettingValue::Float(value) => SettingValue::Float(*value as f32),
        SourceSettingValue::String(value) => SettingValue::String(value.clone()),
        _ => return None,
    })
}

/// Converts a dexvm preference value changed by the extension back into
/// the RakuYomi setting model, so `on_update_settings` changes can be
/// persisted into `settings.source_settings`.
fn pref_to_setting_value(value: &SettingValue) -> Option<SourceSettingValue> {
    Some(match value {
        SettingValue::Bool(value) => SourceSettingValue::Bool(*value),
        SettingValue::Long(value) => SourceSettingValue::Int(*value),
        SettingValue::Int(value) => SourceSettingValue::Int(i64::from(*value)),
        SettingValue::Float(value) => SourceSettingValue::Float(f64::from(*value)),
        SettingValue::String(value) => SourceSettingValue::String(value.clone()),
    })
}

/// Converts a dexvm preference definition into the RakuYomi setting model.
/// Preference screens become groups; plain preferences become switches
/// (their default value decides the initialState).
fn setting_definition_from_dexvm(
    definition: dexvm::context::SettingDefinition,
) -> Option<SettingDefinition> {
    let title = definition.title.filter(|s| !s.is_empty());
    if !definition.children.is_empty() {
        let items: Vec<SettingDefinition> = definition
            .children
            .iter()
            .filter_map(|child| setting_definition_from_dexvm(child.clone()))
            .collect();
        return Some(SettingDefinition::Group {
            title,
            items,
            footer: None,
        });
    }
    let key = definition.key?;
    if key.is_empty() {
        return None;
    }
    let title = title.unwrap_or_else(|| key.clone());
    let values = definition.entry_values.clone();
    let titles = (!definition.entries.is_empty()).then(|| definition.entries.clone());
    let short = definition
        .kind
        .as_deref()
        .and_then(|kind| kind.rsplit('.').next())
        .unwrap_or_default();
    match short {
        "ListPreference" => Some(SettingDefinition::Select {
            title,
            key,
            values,
            titles,
            default: definition.default_text,
        }),
        "MultiSelectListPreference" => Some(SettingDefinition::MultiSelect {
            title,
            key,
            values,
            titles,
            default: definition.default_values,
        }),
        "EditTextPreference" | "TextPreference" => Some(SettingDefinition::Text {
            placeholder: None,
            title: Some(title),
            key,
            default: definition.default_text,
        }),
        _ => {
            let default = match definition.default_value {
                dexvm::vm::value::JValue::Int(value) => value != 0,
                _ => false,
            };
            Some(SettingDefinition::Switch {
                title,
                key,
                default,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_cache_path_uses_keiyoushi_suffix() {
        let path = Path::new("/tmp/eu.kanade.tachiyomi.extension.en.mangapill.keiyoushi.apk");
        assert_eq!(
            probe_cache_path(path),
            Path::new("/tmp/eu.kanade.tachiyomi.extension.en.mangapill.keiyoushi.probe.json")
        );
    }

    #[test]
    fn test_probe_cache_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("rakuyomi-keiyoushi-probe-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let apk = dir.join("eu.kanade.tachiyomi.extension.en.mangapill.keiyoushi.apk");
        fs::write(&apk, b"fake apk bytes").unwrap();
        let probe = ApkProbe {
            package_id: "eu.kanade.tachiyomi.extension.en.mangapill".to_string(),
            version_name: Some("1.4.9".to_string()),
            sources: vec![("MangaPill".to_string(), "en".to_string(), true)],
            setting_definitions: vec![],
        };
        write_probe_cache(&apk, 14, 12345, &probe);
        let cached = read_probe_cache(&apk, 14, 12345).expect("cache should hit");
        assert_eq!(cached.package_id, probe.package_id);
        assert_eq!(cached.sources, probe.sources);
        assert!(
            read_probe_cache(&apk, 15, 12345).is_none(),
            "length mismatch"
        );
        assert!(
            read_probe_cache(&apk, 14, 12346).is_none(),
            "mtime mismatch"
        );
        assert!(
            read_probe_cache(&apk, 14, 12345).is_some(),
            "cached value survives reads"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_setting_value_to_pref() {
        assert!(matches!(
            setting_value_to_pref(&SourceSettingValue::Bool(true)),
            Some(SettingValue::Bool(true))
        ));
        assert!(matches!(
            setting_value_to_pref(&SourceSettingValue::String("x".to_string())),
            Some(SettingValue::String(s)) if s == "x"
        ));
        assert!(setting_value_to_pref(&SourceSettingValue::Vec(vec!["a".to_string()])).is_none());
        assert!(setting_value_to_pref(&SourceSettingValue::Null).is_none());
    }

    #[test]
    fn test_setting_definition_from_dexvm() {
        let def = dexvm::context::SettingDefinition {
            key: Some("nsfw".to_string()),
            title: Some("Show NSFW".to_string()),
            summary: None,
            default_value: dexvm::vm::value::JValue::Int(1),
            enabled: true,
            visible: true,
            children: Vec::new(),
            kind: Some("androidx.preference.SwitchPreferenceCompat".to_string()),
            entries: Vec::new(),
            entry_values: Vec::new(),
            default_text: None,
            default_values: Vec::new(),
        };
        let converted = setting_definition_from_dexvm(def).unwrap();
        assert!(matches!(
            converted,
            SettingDefinition::Switch { key, default, .. } if key == "nsfw" && default
        ));

        let list = dexvm::context::SettingDefinition {
            key: Some("domain".to_string()),
            title: Some("Chọn tên miền".to_string()),
            summary: None,
            default_value: dexvm::vm::value::JValue::Null,
            enabled: true,
            visible: true,
            children: Vec::new(),
            kind: Some("androidx.preference.ListPreference".to_string()),
            entries: vec!["Domain A".to_string(), "Domain B".to_string()],
            entry_values: vec!["a.net".to_string(), "b.net".to_string()],
            default_text: Some("a.net".to_string()),
            default_values: Vec::new(),
        };
        let converted = setting_definition_from_dexvm(list).unwrap();
        assert!(matches!(
            converted,
            SettingDefinition::Select { key, values, titles, default, .. }
                if key == "domain"
                    && values == vec!["a.net".to_string(), "b.net".to_string()]
                    && titles == Some(vec!["Domain A".to_string(), "Domain B".to_string()])
                    && default == Some("a.net".to_string())
        ));

        let multi = dexvm::context::SettingDefinition {
            key: Some("genres".to_string()),
            title: Some("Genres".to_string()),
            summary: None,
            default_value: dexvm::vm::value::JValue::Null,
            enabled: true,
            visible: true,
            children: Vec::new(),
            kind: Some("androidx.preference.MultiSelectListPreference".to_string()),
            entries: Vec::new(),
            entry_values: vec!["action".to_string(), "drama".to_string()],
            default_text: None,
            default_values: vec!["action".to_string()],
        };
        let converted = setting_definition_from_dexvm(multi).unwrap();
        assert!(matches!(
            converted,
            SettingDefinition::MultiSelect { key, values, default, .. }
                if key == "genres" && values.len() == 2 && default == vec!["action".to_string()]
        ));

        let text = dexvm::context::SettingDefinition {
            key: Some("alt".to_string()),
            title: Some("Alt domain".to_string()),
            summary: None,
            default_value: dexvm::vm::value::JValue::Null,
            enabled: true,
            visible: true,
            children: Vec::new(),
            kind: Some("androidx.preference.EditTextPreference".to_string()),
            entries: Vec::new(),
            entry_values: Vec::new(),
            default_text: Some("x.net".to_string()),
            default_values: Vec::new(),
        };
        let converted = setting_definition_from_dexvm(text).unwrap();
        assert!(matches!(
            converted,
            SettingDefinition::Text { key, title, default, .. }
                if key == "alt"
                    && title == Some("Alt domain".to_string())
                    && default == Some("x.net".to_string())
        ));

        let group = dexvm::context::SettingDefinition {
            key: None,
            title: Some("Group".to_string()),
            summary: None,
            default_value: dexvm::vm::value::JValue::Null,
            enabled: true,
            visible: true,
            children: vec![dexvm::context::SettingDefinition {
                key: Some("child".to_string()),
                title: None,
                summary: None,
                default_value: dexvm::vm::value::JValue::Int(0),
                enabled: true,
                visible: true,
                children: Vec::new(),
                kind: Some("androidx.preference.SwitchPreferenceCompat".to_string()),
                entries: Vec::new(),
                entry_values: Vec::new(),
                default_text: None,
                default_values: Vec::new(),
            }],
            kind: None,
            entries: Vec::new(),
            entry_values: Vec::new(),
            default_text: None,
            default_values: Vec::new(),
        };
        let converted = setting_definition_from_dexvm(group).unwrap();
        assert!(matches!(converted, SettingDefinition::Group { items, .. } if items.len() == 1));
    }

    /// Boots a real engine from the fixture APK so `call_fallback` is
    /// exercised with a genuine `dexvm::keiyoushi::Source`. Skips when no
    /// fixture is available (same convention as the other fixture tests).
    fn test_engine() -> Option<(Keiyoushi, dexvm::keiyoushi::Source)> {
        let apk = std::env::var("DEXVM_APK").ok().or_else(|| {
            let fallback = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/tachiyomi-en.mangapill-v1.4.9.apk");
            fallback
                .exists()
                .then(|| fallback.to_string_lossy().into_owned())
        })?;
        let bytes = fs::read(apk).ok()?;
        let mut ext = Keiyoushi::new(&bytes).ok()?;
        let src = ext.sources().ok()?.into_iter().next()?;
        Some((ext, src))
    }

    #[test]
    fn test_probe_reads_version_name_from_manifest() {
        let Some(apk) = std::env::var("DEXVM_APK").ok().or_else(|| {
            let fallback = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/tachiyomi-en.mangapill-v1.4.9.apk");
            fallback
                .exists()
                .then(|| fallback.to_string_lossy().into_owned())
        }) else {
            eprintln!("skipping: no keiyoushi fixture available");
            return;
        };
        let bytes = fs::read(apk).expect("fixture should be readable");
        let probe = probe_apk(&bytes).expect("fixture should probe cleanly");
        assert_eq!(
            probe.version_name.as_deref(),
            Some("1.4.9"),
            "fixture AndroidManifest versionName should surface in the probe"
        );
    }

    #[test]
    fn test_call_fallback_falls_back_on_missing_method() {
        let Some((mut ext, src)) = test_engine() else {
            eprintln!("skipping: no keiyoushi fixture available");
            return;
        };
        let result = call_fallback(
            "getPopularManga",
            &mut ext,
            &src,
            |_ext, _src, _| {
                Err(JvmError::Resolution(
                    "invoke_on getPopularManga (ILkotlin/coroutines/Continuation;)Ljava/lang/Object;: no method getPopularManga ... found".to_string(),
                ))
            },
            |_ext, _src, _| Ok(42),
            1,
        );
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_call_fallback_propagates_other_errors() {
        let Some((mut ext, src)) = test_engine() else {
            eprintln!("skipping: no keiyoushi fixture available");
            return;
        };
        let result = call_fallback(
            "getPopularManga",
            &mut ext,
            &src,
            |_ext, _src, _| {
                Err(JvmError::Resolution(
                    "no method isPopupMenuTouchInterceptor found".to_string(),
                ))
            },
            |_ext, _src, _| Ok(42),
            1,
        );
        assert!(result.is_err());

        let result = call_fallback(
            "getPopularManga",
            &mut ext,
            &src,
            |_ext, _src, _| Err(JvmError::BudgetExceeded),
            |_ext, _src, _| Ok(42),
            1,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_boot_engine_seeds_settings_and_mirrors_changes_back() {
        let apk = std::env::var("DEXVM_APK").ok().or_else(|| {
            let fallback = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/tachiyomi-en.mangapill-v1.4.9.apk");
            fallback
                .exists()
                .then(|| fallback.to_string_lossy().into_owned())
        });
        let Some(apk) = apk else {
            eprintln!("skipping: no keiyoushi fixture available");
            return;
        };

        // Load a real source so `with_engine` runs against a genuine engine.
        let dir = std::env::temp_dir().join(format!(
            "rakuyomi-keiyoushi-writeback-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let apk_name = format!("tachiyomi-en.mangapill-v1.4.9{}", KEIYOUSHI_FILE_SUFFIX);
        let apk_path = dir.join(&apk_name);
        fs::copy(&apk, &apk_path).unwrap();

        let manager = SourceManager::new(dir, HashMap::new(), crate::settings::Settings::default());
        let arc_manager = Arc::new(tokio::sync::Mutex::new(manager));
        let manager_guard = arc_manager.blocking_lock();
        let sources =
            KeiyoushiSource::from_keiyoushi_apk(&apk_path, &manager_guard, &arc_manager).unwrap();
        let Some(first) = sources.into_iter().next() else {
            panic!("fixture APK exposes no sources");
        };
        let source_id = first.id.clone();
        drop(manager_guard);

        // Seed a stored setting, then reload the source the same way
        // `update_source_setting` does; the reloaded instance must pick the
        // stored settings up from `settings.source_settings`.
        {
            let mut manager_guard = arc_manager.blocking_lock();
            manager_guard.settings.source_settings.insert(
                source_id.clone(),
                HashMap::from([("enabled".to_string(), SourceSettingValue::Bool(true))]),
            );
        }
        let manager_guard = arc_manager.blocking_lock();
        let sources =
            KeiyoushiSource::from_keiyoushi_apk(&apk_path, &manager_guard, &arc_manager).unwrap();
        let Some(source) = sources.into_iter().next() else {
            panic!("fixture APK exposes no sources");
        };
        drop(manager_guard);

        // Boot the engine the way the worker thread does, then run one engine
        // call that reads the seeded preference and writes a new one through
        // the extension API.
        let mut engine = boot_engine(&source.apk_path, source.source_index, &source.settings)
            .expect("fixture should boot cleanly");
        let prefs_file = engine
            .ext
            .preference_file(&engine.source)
            .expect("preference file should resolve");
        let settings = engine.ext.get_settings(&prefs_file);
        assert_eq!(
            settings.get("enabled"),
            Some(&SettingValue::Bool(true)),
            "seeded setting must be visible to the extension"
        );
        engine
            .ext
            .update_setting(
                &prefs_file,
                "display_mode",
                SettingValue::String("list".to_string()),
            )
            .expect("host-side settings write must succeed");

        // The engine-side write must be mirrored back into
        // `settings.source_settings` through `on_update_settings`.
        let manager_guard = arc_manager.blocking_lock();
        let stored = manager_guard
            .settings
            .source_settings
            .get(&source_id)
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            stored.get("display_mode"),
            Some(&SourceSettingValue::String("list".to_string())),
            "extension write must be persisted into settings.source_settings"
        );
        assert_eq!(
            stored.get("enabled"),
            Some(&SourceSettingValue::Bool(true)),
            "seeded setting must survive the write-back"
        );
    }

    #[test]
    #[ignore = "needs RAKUYOMI_APK"]
    fn dump_probe_definitions() {
        let apk = std::env::var("RAKUYOMI_APK").unwrap();
        let probe = probe_apk(&std::fs::read(&apk).unwrap()).unwrap();
        println!(
            "{}",
            serde_json::to_string_pretty(&probe.setting_definitions).unwrap()
        );
    }

    #[test]
    #[ignore = "needs RAKUYOMI_APK + network"]
    fn search_with_shrunk_regex_option() {
        let apk = std::env::var("RAKUYOMI_APK").unwrap();
        let dir = std::env::temp_dir().join(format!(
            "rakuyomi-keiyoushi-regexopt-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let apk_name = format!("tachiyomi-regexopt{}", KEIYOUSHI_FILE_SUFFIX);
        let apk_path = dir.join(&apk_name);
        fs::copy(&apk, &apk_path).unwrap();

        let manager = SourceManager::new(dir, HashMap::new(), crate::settings::Settings::default());
        let arc_manager = Arc::new(tokio::sync::Mutex::new(manager));
        let manager_guard = arc_manager.blocking_lock();
        let sources =
            KeiyoushiSource::from_keiyoushi_apk(&apk_path, &manager_guard, &arc_manager).unwrap();
        let Some(first) = sources.into_iter().next() else {
            panic!("fixture APK exposes no sources");
        };
        drop(manager_guard);

        match first.search_mangas(CancellationToken::new(), String::new(), 1) {
            Ok((mangas, has_next)) => {
                assert!(!mangas.is_empty());
                assert!(has_next);
                println!(
                    "fourkhd search returned {} mangas (has_next={})",
                    mangas.len(),
                    has_next
                );
            }
            Err(err) => {
                let msg = format!("{err:#}");
                assert!(
                    !msg.contains("class not found"),
                    "search must not fail on missing RegexOption class: {msg}"
                );
                eprintln!("fourkhd search blocked by network ({msg}); RegexOption resolved fine");
            }
        }
    }
}
