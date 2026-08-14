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
//! Method calls are synchronous from the outside: each call boots a fresh
//! [`Keiyoushi`] engine (the VM is `!Send`, so it is created and used inside
//! the `spawn_blocking` thread the caller already runs on), wires its HTTP
//! callback to the RakuYomi cookie/UA store through a blocking reqwest
//! client, then executes the requested method. Engines are cheap to create
//! (~100 ms release), which keeps the runtime self-healing: no worker
//! threads, no stuck-runtime restarts.

pub mod model;

use std::{
    cell::RefCell,
    fs,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Context as _, Result};
use dexvm::context::SettingValue;
use dexvm::keiyoushi::{HttpData, HttpResp, Keiyoushi};
use dexvm::vm::error::JvmError;
use reqwest::{header::HeaderMap, Method, Request};
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

/// The suffix of the persisted SharedPreferences file of an installed APK
/// (`<pkg>.keiyoushi.prefs`).
pub(crate) const KEIYOUSHI_PREFS_SUFFIX: &str = ".keiyoushi.prefs";

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
    /// The extension APK bytes, shared by every source bundled in it.
    apk_bytes: Arc<Vec<u8>>,
    /// Which source of the APK (`createSources()` index) this instance is.
    source_index: usize,
    /// Merged source settings (stored values overlaid on the extension
    /// preference defaults), applied to the engine before every call. The
    /// mutex mirrors the `Arc<Mutex<BlockingSource>>` wrapper of the wasm
    /// backend: [`SourceSettings`] is `RefCell`-based and single-threaded,
    /// the lock makes the source shareable across `spawn_blocking`.
    settings: Arc<Mutex<SourceSettings>>,
    /// Persistence file for the extension's `SharedPreferences`.
    prefs_path: std::path::PathBuf,
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
            .finish()
    }
}

/// The result of booting an extension APK once: the manifest package id,
/// the sources it bundles and the preference definitions it materialises.
struct ApkProbe {
    /// Canonical `manifest` package id (e.g.
    /// `eu.kanade.tachiyomi.extension.vi.cuutruyenmoe`).
    package_id: String,
    sources: Vec<(String, String, bool)>,
    setting_definitions: Vec<SettingDefinition>,
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
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read extension file {}", path.display()))?;
        let probe = probe_apk(&bytes)?;

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

        let prefs_path = path.with_extension("keiyoushi.prefs");
        let single = probe.sources.len() == 1;
        let apk_bytes = Arc::new(bytes);
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
                    version: Value::String("1".to_string()),
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
                apk_bytes: apk_bytes.clone(),
                source_index: index,
                settings,
                prefs_path: prefs_path.clone(),
            });
        }
        Ok(out)
    }

    /// Runs `f` on a fresh engine configured with the RakuYomi HTTP
    /// infrastructure and the stored source settings.
    ///
    /// Returns the call result together with the URL of the last request the
    /// extension made, which is used to absolutise relative manga/page URLs.
    fn with_engine<T>(
        &self,
        f: impl FnOnce(&mut Keiyoushi, dexvm::keiyoushi::Source) -> Result<T, JvmError>,
    ) -> Result<(T, Option<Url>)> {
        let mut ext = Keiyoushi::new(&self.apk_bytes)
            .map_err(|e| anyhow!("failed to boot keiyoushi extension: {e}"))?;

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
        ext.set_shared_preferences_path(&self.prefs_path);

        let sources = ext.sources().map_err(|e| {
            anyhow!(
                "keiyoushi extension has no sources: {}",
                ext.describe_error(&e)
            )
        })?;
        let source = *sources.get(self.source_index).ok_or_else(|| {
            anyhow!(
                "keiyoushi extension source index {} is out of bounds ({} sources)",
                self.source_index,
                sources.len()
            )
        })?;

        // Stored source settings are applied on every call: each engine
        // starts from the extension defaults, so the persisted values are
        // written back into the preference file the extension reads them
        // from (mihon's `preferenceKey() = "source_<id>"`; "config" would
        // be a file the extension never requests).
        let prefs_file = ext
            .preference_file(&source)
            .unwrap_or_else(|_| "config".to_string());
        let settings = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .all();
        for (key, value) in settings.iter() {
            if let Some(pref) = setting_value_to_pref(value) {
                if let Err(err) = ext.update_setting(&prefs_file, key, pref) {
                    log::warn!("keiyoushi: failed to persist setting `{key}`: {err}");
                }
            }
        }
        drop(settings);

        let result = f(&mut ext, source).map_err(|e| {
            anyhow!(
                "keiyoushi extension call failed: {}",
                ext.describe_error(&e)
            )
        })?;
        let base = last_url
            .borrow()
            .as_deref()
            .and_then(|url| Url::parse(url).ok());
        Ok((result, base))
    }

    /// Implements `get_manga_list`: `popular` (or `latest` for the "latest"
    /// listing), page 1.
    pub fn get_manga_list(
        &self,
        _cancellation_token: CancellationToken,
        listing: aidoku::Listing,
    ) -> Result<Vec<Manga>> {
        let use_latest = listing.name.eq_ignore_ascii_case("latest") && self.supports_latest;
        let (pages, base) = self.with_engine(|ext, src| {
            if use_latest {
                call_fallback(
                    "getLatestUpdates",
                    ext,
                    &src,
                    |ext, src, page| ext.latest_coro(src, page),
                    |ext, src, page| ext.latest(src, page),
                    1,
                )
            } else {
                call_fallback(
                    "getPopularManga",
                    ext,
                    &src,
                    |ext, src, page| ext.popular_coro(src, page),
                    |ext, src, page| ext.popular(src, page),
                    1,
                )
            }
        })?;
        Ok(model::mangas_from_page(
            &self.id,
            base.as_ref(),
            pages.mangas,
        ))
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
        let (pages, base) = self.with_engine(|ext, src| {
            if query.is_empty() {
                // An empty query means "browse", which the extensions expose
                // through the popular listing.
                call_fallback(
                    "getPopularManga",
                    ext,
                    &src,
                    |ext, src, _| ext.popular_coro(src, page.max(1)),
                    |ext, src, _| ext.popular(src, page.max(1)),
                    0,
                )
            } else {
                call_fallback(
                    "getSearchManga",
                    ext,
                    &src,
                    |ext, src, q: &str| ext.search_coro(src, page.max(1), q, &[]),
                    |ext, src, q: &str| ext.search(src, page.max(1), q, &[]),
                    query.as_str(),
                )
            }
        })?;
        Ok((
            model::mangas_from_page(&self.id, base.as_ref(), pages.mangas),
            pages.has_next,
        ))
    }

    /// Implements `get_manga_details` from a raw manga URL.
    pub fn get_manga_details(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Manga> {
        let manga = dexvm::keiyoushi::Manga {
            url: manga_id.clone(),
            title: manga_id.clone(),
            ..Default::default()
        };
        let (manga, base) = self.with_engine(|ext, src| {
            call_fallback(
                "getMangaUpdate",
                ext,
                &src,
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.manga_update_details(src, m),
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.manga_details(src, m),
                &manga,
            )
        })?;
        Ok(model::manga_from_keiyoushi(&self.id, base.as_ref(), manga))
    }

    /// Implements `get_chapter_list` from a raw manga URL.
    pub fn get_chapter_list(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<Chapter>> {
        let manga = dexvm::keiyoushi::Manga {
            url: manga_id.clone(),
            title: manga_id.clone(),
            ..Default::default()
        };
        let (chapters, base) = self.with_engine(|ext, src| {
            call_fallback(
                "getMangaUpdate",
                ext,
                &src,
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.manga_update_chapters(src, m),
                |ext, src, m: &dexvm::keiyoushi::Manga| ext.chapters(src, m),
                &manga,
            )
        })?;
        let mut out = model::chapters_from_keiyoushi(&self.id, &manga_id, base.as_ref(), chapters);
        crate::source::model::normalize_chapter_order(&mut out);
        Ok(out)
    }

    /// Implements `get_page_list` from a raw chapter URL.
    pub fn get_page_list(
        &self,
        _cancellation_token: CancellationToken,
        _manga_id: String,
        chapter_id: String,
        _chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        let chapter = dexvm::keiyoushi::Chapter {
            url: chapter_id.clone(),
            name: chapter_id.clone(),
            ..Default::default()
        };
        let (pages, base) = self.with_engine(|ext, src| {
            call_fallback(
                "getPageList",
                ext,
                &src,
                |ext, src, c: &dexvm::keiyoushi::Chapter| ext.pages_coro(src, c),
                |ext, src, c: &dexvm::keiyoushi::Chapter| ext.pages(src, c),
                &chapter,
            )
        })?;
        if std::env::var("DEXVM_TRACE").is_ok() {
            eprintln!("DEXVM_TRACE keiyoushi get_page_list: raw={}", pages.len());
        }
        Ok(model::pages_from_keiyoushi(
            &self.id,
            &chapter_id,
            base.as_ref(),
            pages,
        ))
    }

    /// Implements `get_image_request`: image URLs carry their own
    /// authentication parameters, so a plain GET with the shared
    /// user-agent and the per-domain cookie store is enough.
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
    let package_id = ext.manifest().map(|m| m.package_id).unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        let converted = setting_definition_from_dexvm(def).unwrap();
        assert!(matches!(
            converted,
            SettingDefinition::Switch { key, default, .. } if key == "nsfw" && default
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
            }],
        };
        let converted = setting_definition_from_dexvm(group).unwrap();
        assert!(matches!(converted, SettingDefinition::Group { items, .. } if items.len() == 1));
    }

    /// Boots a real engine from the fixture APK so `call_fallback` is
    /// exercised with a genuine `dexvm::keiyoushi::Source`. Skips when no
    /// fixture is available (same convention as the other fixture tests).
    fn test_engine() -> Option<(Keiyoushi, dexvm::keiyoushi::Source)> {
        let apk = std::env::var("DEXVM_APK").ok().or_else(|| {
            let fallback =
                Path::new("/home/shin/dex_runtime/fixtures/tachiyomi-en.mangapill-v1.4.9.apk");
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
}
