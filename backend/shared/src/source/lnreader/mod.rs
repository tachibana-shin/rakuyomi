//! LNReader plugin source backend.
//!
//! Loads a single LNReader plugin file (`*.lnreader.js`), runs it inside the
//! embedded QuickJS runtime and exposes it through the same [`Source`]
//! interface as WASM sources.

pub mod convert;
pub mod manifest;
pub mod runtime;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{header::HeaderMap, Method, Request};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    resource_usage::ResourceRegistry,
    settings::SourceSettingValue,
    source::{
        model::{Manga, Page, SettingDefinition},
        source_settings::SourceSettings,
        SourceConfig, SourceFeatures, SourceInfo, SourceManifest,
    },
    source_manager::SourceManager,
    util::DEFAULT_USER_AGENT,
};

use self::{
    convert::{
        chapters, manga_from_novel, mangas_from_search, page_from_chapter_html, HTML_MARKER,
    },
    manifest::{manifest_from_props, parse_props, setting_definitions, PluginProps},
    runtime::{LnReaderRuntime, DEFAULT_INVOKE_TIMEOUT},
};

/// The suffix of installed plugin files (`<id>.lnreader.js`).
pub(crate) const LNREADER_FILE_SUFFIX: &str = ".lnreader.js";

/// The suffix of the probe cache sidecar (`<id>.lnreader.probe.json`),
/// which records the `props` result of evaluating the plugin JS once at
/// install time so later loads skip the JS evaluation entirely.
pub(crate) const LNREADER_PROBE_SUFFIX: &str = ".lnreader.probe.json";

/// On-disk probe cache (`<id>.lnreader.probe.json`). The plugin fingerprint
/// (length + mtime) guards against stale metadata when the plugin is
/// updated or replaced outside the install pipeline.
#[derive(Serialize, Deserialize)]
struct ProbeCache {
    plugin_len: u64,
    plugin_mtime_ns: u128,
    props_json: String,
}

fn probe_cache_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let stem = name.strip_suffix(LNREADER_FILE_SUFFIX).unwrap_or(name);
    path.with_file_name(format!("{stem}{LNREADER_PROBE_SUFFIX}"))
}

/// Returns the cached `props` JSON when it matches the plugin fingerprint.
fn read_probe_cache(path: &Path, plugin_len: u64, plugin_mtime_ns: u128) -> Option<String> {
    let contents = fs::read_to_string(probe_cache_path(path)).ok()?;
    let cache: ProbeCache = serde_json::from_str(&contents).ok()?;
    (cache.plugin_len == plugin_len && cache.plugin_mtime_ns == plugin_mtime_ns)
        .then_some(cache.props_json)
}

/// Persists the `props` result next to the plugin. Failures are logged,
/// never fatal: the next load simply re-evaluates the plugin.
fn write_probe_cache(path: &Path, plugin_len: u64, plugin_mtime_ns: u128, props_json: &str) {
    let cache = ProbeCache {
        plugin_len,
        plugin_mtime_ns,
        props_json: props_json.to_string(),
    };
    let result = serde_json::to_vec(&cache)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| fs::write(probe_cache_path(path), bytes).map_err(anyhow::Error::from));
    if let Err(err) = result {
        log::warn!("failed to write probe cache for {}: {err}", path.display());
    }
}

/// A single LNReader plugin exposed through the RakuYomi source API.
///
/// All method calls block on the underlying JS runtime; the async wrappers
/// used by the rest of the codebase run them inside `spawn_blocking`.
///
/// The plugin JS is only evaluated lazily: a valid probe cache is parsed at
/// load time (cheap), and a missing or stale cache defers the JS evaluation
/// to the first actual method call (`ensure_probed`), so loading a plugin
/// collection never boots an interpreter.
pub struct LnReaderSource {
    /// Canonical source id, derived from the file name (`<id>.lnreader.js`).
    /// It is stable whether or not the plugin has been probed yet, which is
    /// what the settings and library entries are keyed by.
    pub id: String,
    pub features: SourceFeatures,
    /// Runtime usage registry this source reports its JS memory to.
    pub(crate) usage: ResourceRegistry,
    path: PathBuf,
    plugin_code: String,
    source_of_source: Option<String>,
    /// Snapshot of the stored settings at load time, applied when the probe
    /// runs (the settings container needs them up front).
    stored_settings: HashMap<String, SourceSettingValue>,
    /// Manager handle the settings container persists changes through.
    arc_manager: Arc<tokio::sync::Mutex<SourceManager>>,
    /// Lazy probe state. `None` until probed, then the cached outcome (both
    /// success and failure) so a plugin that fails to probe is not
    /// re-evaluated on every call.
    probe: Mutex<Option<Result<Arc<ProbedLnReader>, String>>>,
}

/// Everything derived from evaluating the plugin `props`: either parsed from
/// the probe cache at load time, or built by evaluating the JS on first use.
pub(crate) struct ProbedLnReader {
    manifest: SourceManifest,
    setting_definitions: Vec<SettingDefinition>,
    props: PluginProps,
    /// Merged source settings (stored values overlaid on the definition
    /// defaults), used to build the filter/settings JSON for each call. The
    /// mutex mirrors the wasm backend's `Arc<Mutex<BlockingSource>>`
    /// wrapper: [`SourceSettings`] is `RefCell`-based and single-threaded,
    /// the lock makes the source shareable across `spawn_blocking`.
    settings: Arc<Mutex<SourceSettings>>,
    runtime: LnReaderRuntime,
}

impl std::fmt::Debug for LnReaderSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LnReaderSource")
            .field("id", &self.id)
            .field("probed", &self.probe.try_lock().map(|p| p.is_some()))
            .finish()
    }
}

/// Derives the plugin id from the file name (`<id>.lnreader.js`).
pub(crate) fn plugin_id_from_path(path: &Path) -> Result<String> {
    let stem = path
        .file_stem()
        .ok_or_else(|| anyhow!("plugin file has no filename stem"))?
        .to_str()
        .ok_or_else(|| anyhow!("plugin filename is not valid UTF-8"))?;
    let id = stem.strip_suffix(".lnreader").unwrap_or(stem).to_string();
    if id.is_empty() {
        bail!("plugin filename is empty");
    }
    Ok(id)
}

impl LnReaderSource {
    /// Loads a plugin from a `*.lnreader.js` file. The plugin JS is only
    /// evaluated when the probe cache is missing or stale; with a valid
    /// cache the source starts fully probed (parsing the cached props is
    /// cheap), without one it stays pending until the first method call.
    pub fn from_lnreader_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Self> {
        let plugin_code = fs::read_to_string(path)
            .with_context(|| format!("failed to read plugin file {}", path.display()))?;

        // The plugin id is derived from the file name so it is known before
        // the props are evaluated and stays stable across the probe; the
        // runtime still uses the plugin-declared id for storage namespacing.
        let id = plugin_id_from_path(path)?;
        let source_of_source = Self::read_source_of_source(path)?;
        let stored_settings = manager
            .settings
            .source_settings
            .get(&id)
            .cloned()
            .unwrap_or_default();

        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to stat plugin file {}", path.display()))?;
        let plugin_len = metadata.len();
        let plugin_mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());
        let usage = ResourceRegistry::default();
        let probe =
            match plugin_mtime_ns.and_then(|mtime| read_probe_cache(path, plugin_len, mtime)) {
                Some(props_json) => {
                    match Self::build_probed(
                        &id,
                        &plugin_code,
                        &source_of_source,
                        &stored_settings,
                        arc_manager,
                        &props_json,
                        usage.clone(),
                    ) {
                        Ok(state) => Some(Ok(Arc::new(state))),
                        Err(e) => {
                            log::warn!("failed to parse probe cache for {}: {e}", path.display());
                            // Remove the invalid cache so run_probe re-evaluates
                            // the plugin instead of reading the same bad data.
                            let _ = fs::remove_file(probe_cache_path(path));
                            None
                        }
                    }
                }
                None => None,
            };

        Ok(Self {
            id,
            features: SourceFeatures {
                process_page_image: false,
            },
            usage,
            path: path.to_path_buf(),
            plugin_code,
            source_of_source,
            stored_settings,
            arc_manager: arc_manager.clone(),
            probe: Mutex::new(probe),
        })
    }

    /// Builds the probed state from the plugin `props` JSON (either read
    /// from the probe cache or produced by evaluating the JS).
    fn build_probed(
        id: &str,
        plugin_code: &str,
        source_of_source: &Option<String>,
        stored_settings: &HashMap<String, SourceSettingValue>,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
        props_json: &str,
        usage: ResourceRegistry,
    ) -> Result<ProbedLnReader> {
        let props = parse_props(props_json)?;
        let mut manifest = manifest_from_props(&props, source_of_source.clone());
        // The registered id (from the file name) is the stable identity; the
        // plugin-declared id only namespaces the JS storage.
        manifest.info.id = id.to_string();

        let setting_definitions = setting_definitions(&props.plugin_settings)?;

        let settings = Arc::new(Mutex::new(SourceSettings::new(
            id.to_string(),
            &setting_definitions,
            stored_settings,
            arc_manager,
        )?));

        // The final runtime starts its worker lazily on the first call,
        // using the source's usage registry to track its memory.
        let runtime = LnReaderRuntime::new(
            props.id.clone(),
            plugin_code.to_string(),
            String::new(),
            DEFAULT_USER_AGENT.to_string(),
            DEFAULT_INVOKE_TIMEOUT,
            usage,
        )?;

        // Seed the plugin's `@libs/storage` with the pluginSettings values,
        // mirroring how the LNReader app stores them (`pluginId_DB_key`).
        runtime.seed_storage(Self::storage_seed(
            &props.id,
            &props.plugin_settings,
            &settings,
        )?);

        Ok(ProbedLnReader {
            manifest,
            setting_definitions,
            props,
            settings,
            runtime,
        })
    }

    /// The manifest reported while the plugin is not probed yet. The id
    /// (from the file name) and `source_of_source` are known up front; the
    /// name falls back to the id until the props are evaluated.
    fn placeholder_manifest(&self) -> SourceManifest {
        SourceManifest {
            info: SourceInfo {
                id: self.id.clone(),
                lang: None,
                languages: None,
                #[cfg(not(feature = "all"))]
                content_rating: None,
                name: self.id.clone(),
                version: Value::String(String::new()),
                url: None,
                urls: None,
                min_app_version: None,
            },
            config: Some(SourceConfig {
                allows_base_url_select: Some(false),
            }),
            source_of_source: self.source_of_source.clone(),
        }
    }

    /// The probed state, if the probe has run (or the plugin was loaded from
    /// a valid cache). Never triggers the probe; while another thread is
    /// mid-probe the lock is contended, so `try_lock` yields the placeholder
    /// instead of blocking the caller.
    fn probed(&self) -> Option<Arc<ProbedLnReader>> {
        self.probe
            .try_lock()
            .ok()?
            .as_ref()
            .and_then(|outcome| outcome.as_ref().ok())
            .cloned()
    }

    /// The source manifest: the probed one once available, a placeholder
    /// derived from the file name before that.
    pub fn manifest(&self) -> SourceManifest {
        self.probed()
            .map(|state| state.manifest.clone())
            .unwrap_or_else(|| self.placeholder_manifest())
    }

    /// The setting definitions: empty until the plugin is probed.
    pub fn setting_definitions(&self) -> Vec<SettingDefinition> {
        self.probed()
            .map(|state| state.setting_definitions.clone())
            .unwrap_or_default()
    }

    /// Runs the probe on first use: re-reads the probe cache (it may have
    /// appeared since load), otherwise evaluates the plugin JS, writes the
    /// cache and builds the full probed state. The outcome is cached, so a
    /// plugin that fails to probe errors fast on every later call.
    pub(crate) fn ensure_probed(&self) -> Result<Arc<ProbedLnReader>> {
        let mut guard = self.probe.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = guard.as_ref() {
            return cached.clone().map_err(|error| anyhow!("{error}"));
        }
        let outcome = self
            .run_probe()
            .map(Arc::new)
            .map_err(|error| format!("{error:#}"));
        if let Err(error) = &outcome {
            log::warn!("failed to probe LNReader plugin {}: {error}", self.id);
        }
        *guard = Some(outcome.clone());
        outcome.map_err(|error| anyhow!("{error}"))
    }

    /// Evaluates the plugin `props` (or reads them from the probe cache) and
    /// builds the probed state from the result.
    fn run_probe(&self) -> Result<ProbedLnReader> {
        let metadata = fs::metadata(&self.path)
            .with_context(|| format!("failed to stat plugin file {}", self.path.display()))?;
        let plugin_len = metadata.len();
        let plugin_mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());
        let props_json = match plugin_mtime_ns
            .and_then(|mtime| read_probe_cache(&self.path, plugin_len, mtime))
        {
            Some(cached) => cached,
            None => {
                let probe_runtime = LnReaderRuntime::new(
                    self.id.clone(),
                    self.plugin_code.clone(),
                    String::new(),
                    DEFAULT_USER_AGENT.to_string(),
                    DEFAULT_INVOKE_TIMEOUT,
                    ResourceRegistry::default(),
                )?;
                let result = probe_runtime
                    .invoke("props", "[]")
                    .context("plugin `props` failed")?;
                probe_runtime.stop_worker();
                if let Some(mtime) = plugin_mtime_ns {
                    write_probe_cache(&self.path, plugin_len, mtime, &result);
                }
                result
            }
        };
        Self::build_probed(
            &self.id,
            &self.plugin_code,
            &self.source_of_source,
            &self.stored_settings,
            &self.arc_manager,
            &props_json,
            self.usage.clone(),
        )
    }

    /// Reads the `source_of_source` from the sidecar meta file, if any.
    fn read_source_of_source(path: &Path) -> Result<Option<String>> {
        let meta_path = crate::source::BlockingSource::meta_source_path(path)?;
        if !meta_path.exists() {
            return Ok(None);
        }
        let meta: crate::source::SourceMeta = serde_json::from_str(
            &fs::read_to_string(&meta_path)
                .with_context(|| format!("failed to read meta file {:?}", meta_path))?,
        )?;
        Ok(meta.source_of_source)
    }

    /// Builds the `pluginId_DB_key` storage entries from the plugin settings.
    fn storage_seed(
        plugin_id: &str,
        plugin_settings: &Value,
        settings: &Arc<Mutex<SourceSettings>>,
    ) -> Result<Vec<(String, String)>> {
        let Value::Object(map) = plugin_settings else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for (key, definition) in map {
            let Some(declared) = definition.get("value") else {
                continue;
            };
            let value = settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(key)
                .map(|v| setting_to_json(&v))
                .unwrap_or_else(|| declared.clone());
            let item = json!({
                "created": chrono::Utc::now().to_rfc3339(),
                "value": value,
            });
            entries.push((format!("{}_DB_{}", plugin_id, key), item.to_string()));
        }
        Ok(entries)
    }

    /// Serializes the current settings as a JSON object for the JS side.
    /// Returns an empty object while the plugin is not probed yet.
    pub fn settings_json(&self) -> Value {
        match self.ensure_probed() {
            Ok(state) => Self::settings_json_from(&state),
            Err(e) => {
                log::warn!("failed to probe plugin {} for settings: {e:#}", self.id);
                Value::Object(serde_json::Map::new())
            }
        }
    }

    fn settings_json_from(state: &ProbedLnReader) -> Value {
        let mut out = serde_json::Map::new();
        for (key, value) in state
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .all()
        {
            out.insert(key.clone(), setting_to_json(&value));
        }
        Value::Object(out)
    }

    /// Invokes a plugin method with JSON args and parses the JSON response.
    pub fn invoke(&self, method: &str, args: Value) -> Result<Value> {
        let state = self.ensure_probed()?;
        Self::invoke_with(&state, method, &args)
    }

    fn invoke_with(state: &ProbedLnReader, method: &str, args: &Value) -> Result<Value> {
        let output = state
            .runtime
            .invoke(method, &args.to_string())
            .with_context(|| format!("plugin method `{}` failed", method))?;
        Ok(serde_json::from_str(&output)?)
    }

    /// Resolves a path to an absolute URL the way the app does
    /// (`resolveUrl`, falling back to `site + path`).
    fn resolve_url(&self, state: &ProbedLnReader, path: &str, is_novel: bool) -> Option<Url> {
        let out = Self::invoke_with(state, "resolveUrl", &json!([path, is_novel]))
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{}{}", state.props.site, path));
        Url::parse(&out).ok()
    }

    /// Resolves a manga cover URL. Absolute URLs are kept as-is; relative
    /// paths (e.g. `/uploads/cover.jpg`) are resolved against the plugin's
    /// site, mirroring how the app builds the cover URL.
    fn resolve_manga_cover(&self, state: &ProbedLnReader, cover: Option<String>) -> Option<String> {
        let cover = cover?;
        if Url::parse(&cover).is_ok() {
            return Some(cover);
        }
        let path = cover.trim_start_matches('/');
        Url::parse(&format!("{}{}", state.props.site, path))
            .ok()
            .map(|u| u.to_string())
    }

    /// Converts a plugin manga, resolving its cover URL.
    fn manga_from(&self, state: &ProbedLnReader, manga: aidoku::Manga) -> Manga {
        let mut manga = manga;
        manga.cover = self.resolve_manga_cover(state, manga.cover);
        Manga::from(manga, self.id.clone())
    }

    /// Implements `get_manga_list`: the popular novels list (page 1).
    pub fn get_manga_list(
        &self,
        _cancellation_token: CancellationToken,
        listing: aidoku::Listing,
    ) -> Result<Vec<Manga>> {
        let state = self.ensure_probed()?;
        let show_latest = listing.name.eq_ignore_ascii_case("latest");
        let settings = Self::settings_json_from(&state);
        let value = Self::invoke_with(&state, "popular", &json!([1, settings, show_latest]))?;
        Ok(mangas_from_search(&value)?
            .into_iter()
            .map(|manga| self.manga_from(&state, manga))
            .collect())
    }

    /// Implements `search_mangas`. An empty query means "browse", which the
    /// LNReader plugins expose through `popularNovels`; otherwise the plugin's
    /// `searchNovels` is used. Pagination is not supported by the LNReader API
    /// shape, so `has_next` is always false.
    pub fn search_mangas(
        &self,
        _cancellation_token: CancellationToken,
        query: String,
        page: i32,
    ) -> Result<(Vec<Manga>, bool)> {
        let state = self.ensure_probed()?;
        let value = if query.trim().is_empty() {
            let settings = Self::settings_json_from(&state);
            Self::invoke_with(&state, "popular", &json!([page.max(1), settings, false]))?
        } else {
            Self::invoke_with(&state, "search", &json!([query, page]))?
        };
        let mangas = mangas_from_search(&value)?
            .into_iter()
            .map(|manga| self.manga_from(&state, manga))
            .collect();
        Ok((mangas, false))
    }

    /// Implements `get_manga_details`: `parseNovel` + the chapter list.
    pub fn get_manga_details(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Manga> {
        let state = self.ensure_probed()?;
        let value = Self::invoke_with(&state, "novel", &json!([manga_id]))?;
        Ok(self.manga_from(&state, manga_from_novel(&value)?))
    }

    /// Implements `get_chapter_list`. Plugins with `parsePage` paginate their
    /// chapter list, so all pages are fetched here.
    pub fn get_chapter_list(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<crate::source::model::Chapter>> {
        let state = self.ensure_probed()?;
        let novel = Self::invoke_with(&state, "novel", &json!([manga_id]))?;
        let mut out = chapters(novel.get("chapters").unwrap_or(&Value::Null))?;

        let total_pages = novel.get("totalPages").and_then(Value::as_u64).unwrap_or(1);
        if state.props.has_parse_page && total_pages > 1 {
            for page in 2..=total_pages {
                let value = Self::invoke_with(&state, "page", &json!([manga_id, page]))?;
                out.extend(chapters(&value)?);
            }
        }

        // Resolve each chapter path to an absolute URL; the downloader uses it
        // as the base URL for the images inside the chapter HTML. This mirrors
        // the app's `chapterPathToUrl`: `resolveUrl` when the plugin defines
        // it, `site + path` otherwise.
        for chapter in out.iter_mut() {
            let url = if state.props.has_resolve_url {
                self.resolve_url(&state, &chapter.key, false)
            } else {
                Url::parse(&format!("{}{}", state.props.site, chapter.key)).ok()
            };
            chapter.url = url.map(|u| u.to_string());
        }

        let mut out = out
            .into_iter()
            .map(|chapter| {
                crate::source::model::Chapter::from(chapter, self.id.clone(), manga_id.clone())
            })
            .collect::<Vec<_>>();
        crate::source::model::normalize_chapter_order(&mut out);
        Ok(out)
    }

    /// Implements `get_page_list`: the chapter HTML becomes a single text page.
    pub fn get_page_list(
        &self,
        _cancellation_token: CancellationToken,
        _manga_id: String,
        chapter_id: String,
        _chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        let state = self.ensure_probed()?;
        let value = Self::invoke_with(&state, "chapter", &json!([chapter_id]))?;
        let html = value.as_str().unwrap_or_default();
        Ok(vec![page_from_chapter_html(0, html, chapter_id)])
    }

    /// Implements `get_image_request` using the plugin's `imageRequestInit`.
    pub fn get_image_request(
        &self,
        url: Url,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        let state = self.ensure_probed()?;
        let init = state.props.image_request_init.as_ref();
        let method = init
            .and_then(|init| init.method.as_deref())
            .and_then(|m| Method::from_bytes(m.as_bytes()).ok())
            .unwrap_or(Method::GET);
        let mut builder = crate::tls::client_builder()
            .build()
            .context("failed to build HTTP client")?
            .request(method, url.clone());

        if let Some(init) = init {
            let mut headers = HeaderMap::new();
            for (key, value) in &init.headers {
                if let (Ok(name), Ok(v)) = (
                    reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                    reqwest::header::HeaderValue::from_str(value),
                ) {
                    headers.insert(name, v);
                }
            }
            builder = builder.headers(headers);
            if let Some(body) = &init.body {
                builder = builder.body(body.clone());
            }
        }

        let request = builder
            .build()
            .with_context(|| format!("failed to build image request for {}", url))?;
        Ok(request)
    }

    /// LNReader plugins have no image processing step.
    pub fn process_page_image(
        &self,
        _cancellation_token: CancellationToken,
        _request: (Url, HeaderMap),
        _response: (reqwest::StatusCode, HeaderMap),
        _bytes: tokio_util::bytes::Bytes,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Vec<u8>> {
        bail!("process_page_image is not supported by LNReader plugins")
    }

    /// Notifications are not supported by LNReader plugins.
    pub fn handle_notification_next(
        &self,
        _cancellation_token: CancellationToken,
        _key: String,
    ) -> Result<()> {
        Ok(())
    }

    // next-SDK shaped helpers. Nothing in the codebase calls them for LNReader
    // sources (the server only uses the primary methods), they exist so the
    // `Source` dispatcher stays uniform.

    pub fn get_manga_list_next(
        &self,
        cancellation_token: CancellationToken,
        listing: aidoku::Listing,
        _page: i32,
    ) -> Result<crate::source::NextMangaPageResult> {
        let _ = cancellation_token;
        let state = self.ensure_probed()?;
        let show_latest = listing.name.eq_ignore_ascii_case("latest");
        let settings = Self::settings_json_from(&state);
        let value = Self::invoke_with(&state, "popular", &json!([1, settings, show_latest]))?;
        Ok(crate::source::NextMangaPageResult {
            entries: mangas_from_search(&value)?,
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
        let _ = cancellation_token;
        let state = self.ensure_probed()?;
        let value = Self::invoke_with(&state, "search", &json!([query, page]))?;
        Ok(crate::source::NextMangaPageResult {
            entries: mangas_from_search(&value)?,
            has_next_page: false,
        })
    }

    pub fn get_manga_update_next(
        &self,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<aidoku::Manga> {
        let _ = cancellation_token;
        let state = self.ensure_probed()?;
        let mut manga = if needs_details {
            let value = Self::invoke_with(&state, "novel", &json!([manga.key]))?;
            manga_from_novel(&value)?
        } else {
            manga
        };
        if !needs_chapters {
            manga.chapters = None;
        }
        // if needs_chapters {
        //     let novel = self.invoke("novel", json!([manga.key]))?;
        //     manga.chapters = Some(chapters(novel.get("chapters").unwrap_or(&Value::Null))?);
        // }
        Ok(manga)
    }

    pub fn get_page_list_next(
        &self,
        cancellation_token: CancellationToken,
        _manga: aidoku::Manga,
        chapter: aidoku::Chapter,
    ) -> Result<Vec<aidoku::Page>> {
        let _ = cancellation_token;
        let state = self.ensure_probed()?;
        let value = Self::invoke_with(&state, "chapter", &json!([chapter.key]))?;
        let html = value.as_str().unwrap_or_default();
        Ok(vec![aidoku::Page {
            content: aidoku::PageContent::Text(format!("{HTML_MARKER}{html}")),
            thumbnail: None,
            has_description: false,
            description: None,
        }])
    }

    pub fn get_image_request_next(
        &self,
        url: Url,
        ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        self.get_image_request(url, ctx)
    }
}

/// Serializes a setting value the way the JS side expects it.
fn setting_to_json(value: &SourceSettingValue) -> Value {
    match value {
        SourceSettingValue::String(s) => Value::String(s.clone()),
        SourceSettingValue::Int(i) => json!(i),
        SourceSettingValue::Float(f) => json!(f),
        SourceSettingValue::Bool(b) => json!(b),
        SourceSettingValue::Vec(v) => json!(v),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_cache_path_uses_lnreader_suffix() {
        let path = Path::new("/tmp/mangapill.lnreader.js");
        assert_eq!(
            probe_cache_path(path),
            Path::new("/tmp/mangapill.lnreader.probe.json")
        );
    }

    #[test]
    fn test_probe_cache_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("rakuyomi-lnreader-probe-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let plugin = dir.join("mangapill.lnreader.js");
        fs::write(&plugin, "export function props() { return {}; }").unwrap();
        write_probe_cache(&plugin, 42, 12345, r#"{"id":"mangapill"}"#);
        let cached = read_probe_cache(&plugin, 42, 12345).expect("cache should hit");
        assert_eq!(cached, r#"{"id":"mangapill"}"#);
        assert!(
            read_probe_cache(&plugin, 43, 12345).is_none(),
            "length mismatch"
        );
        assert!(
            read_probe_cache(&plugin, 42, 12346).is_none(),
            "mtime mismatch"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
