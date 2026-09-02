use aidoku::FilterValue;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::{header::HeaderMap, Method, Request, StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio_util::bytes::Bytes;
use tokio_util::sync::CancellationToken;
use url::Url;
use wasmi::*;
use zip::ZipArchive;

use crate::{
    settings::{Settings, SourceSettingValue},
    source::{
        next_reader::read_next,
        wasm_imports::next as sdk_next,
        wasm_store::{ImageRef, ImageRequest, ImageResponse},
    },
    source_manager::SourceManager,
};

use self::{
    keiyoushi::KeiyoushiSource,
    lnreader::LnReaderSource,
    mangayomi::MangayomiSource,
    model::{Chapter, Filter, Manga, MangaPageResult, Page, SettingDefinition},
    source_settings::SourceSettings,
    wasm_imports::{
        aidoku::register_aidoku_imports,
        defaults::register_defaults_imports,
        env::register_env_imports,
        html::register_html_imports,
        json::register_json_imports,
        net::{register_net_imports, DEFAULT_USER_AGENT},
        std::register_std_imports,
    },
    wasm_store::{
        ObjectValue, OperationContext, OperationContextObject, RequestBuildingState, RequestState,
        Value, ValueMap, WasmStore,
    },
};
use crate::resource_usage::ResourceRegistry;

pub(crate) mod decode_image;
pub mod keiyoushi;
pub mod lnreader;
pub mod mangayomi;

#[cfg(not(feature = "all"))]
pub mod html_element;
#[cfg(feature = "all")]
mod html_element;
pub mod model;
#[cfg(not(feature = "all"))]
pub mod next_reader;
#[cfg(feature = "all")]
mod next_reader;
pub mod source_settings;
#[cfg(any(feature = "ffi", feature = "all"))]
pub mod wasm_imports;
#[cfg(not(any(feature = "ffi", feature = "all")))]
mod wasm_imports;
#[cfg(any(feature = "ffi", feature = "all"))]
pub mod wasm_store;
#[cfg(not(any(feature = "ffi", feature = "all")))]
mod wasm_store;

/*
 * params need mark encode
 * handle_notification
 * handle_deep_link
 * handle_basic_login
 * handle_web_login
 * handle_key_migration
 */

/// The kinds of sources RakuYomi can run: WASM (Aidoku), LNReader
/// (JavaScript), MangaYomi (Dart) and Keiyoushi (mihon extension DEX)
/// plugins. All of them are kept behind an `Arc` so `Source` is cheap to
/// clone; blocking work is always moved to a `spawn_blocking` thread.
#[derive(Clone)]
pub enum SourceBackend {
    /// A WASM source, mirroring the legacy tuple layout.
    Aidoku(Arc<Mutex<BlockingSource>>),
    /// An LNReader plugin running inside an embedded QuickJS runtime.
    LnReader(Arc<LnReaderSource>),
    /// A MangaYomi extension running inside the embedded d4rt_rs interpreter.
    Mangayomi(Arc<MangayomiSource>),
    /// One source of a keiyoushi extension APK running inside dexvm.
    Keiyoushi(Arc<KeiyoushiSource>),
}

#[derive(Clone)]
pub struct Source {
    pub backend: SourceBackend,
    pub features: SourceFeatures,
    pub usage: ResourceRegistry,
}

/// Like [`wrap_blocking_source_fn!`], but dispatches between the WASM and
/// LNReader backends.
#[macro_export]
macro_rules! wrap_blocking_source_fn {
    ($fn_name:ident, $return_type:ty, $($param:ident : $type:ty),*) => {
        pub async fn $fn_name(&self, $($param: $type),*) -> $return_type {
            let started_at = std::time::Instant::now();
            let usage = self.usage.clone();
            let result: ::std::result::Result<($return_type, String), _> = match &self.backend {
                SourceBackend::Aidoku(blocking_source) => {
                    let blocking_source = blocking_source.clone();
                    let usage = usage.clone();
                    ::tokio::task::spawn_blocking(move || {
                        let mut guard = blocking_source
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        let source_id = guard.manifest.info.id.clone();
                        let result = guard.$fn_name($($param),*);
                        if result.is_ok() && usage.is_active() {
                            if let (Ok(memory), Some(store)) =
                                (guard.get_memory(), guard.store.as_ref())
                            {
                                usage.record_wasm_memory(
                                    &source_id,
                                    memory.data_size(store) as u64,
                                );
                            }
                        }
                        (result, source_id)
                    }).await
                }
                SourceBackend::LnReader(lnreader) => {
                    let lnreader = lnreader.clone();
                    ::tokio::task::spawn_blocking(move || {
                        let result = lnreader.$fn_name($($param),*);
                        (result, lnreader.manifest().info.id.clone())
                    }).await
                }
                SourceBackend::Mangayomi(mangayomi) => {
                    let mangayomi = mangayomi.clone();
                    ::tokio::task::spawn_blocking(move || {
                        let result = mangayomi.$fn_name($($param),*);
                        (result, mangayomi.manifest().info.id.clone())
                    }).await
                }
                SourceBackend::Keiyoushi(keiyoushi) => {
                    let keiyoushi = keiyoushi.clone();
                    ::tokio::task::spawn_blocking(move || {
                        let result = keiyoushi.$fn_name($($param),*);
                        (result, keiyoushi.manifest.info.id.clone())
                    }).await
                }
            };
            let (result, source_id) = match result {
                Ok(pair) => pair,
                Err(e) => {
                    // The worker panicked before returning its id; read the
                    // manifest on the async path only in this exceptional
                    // case so the failure still lands in the usage log.
                    let source_id = match &self.backend {
                        SourceBackend::Aidoku(blocking_source) => blocking_source
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .manifest
                            .info
                            .id
                            .clone(),
                        SourceBackend::LnReader(lnreader) => lnreader.manifest().info.id.clone(),
                        SourceBackend::Mangayomi(mangayomi) => mangayomi.manifest().info.id.clone(),
                        SourceBackend::Keiyoushi(keiyoushi) => keiyoushi.manifest.info.id.clone(),
                    };
                    usage.record(
                        &source_id,
                        Err(format!("worker task failed: {e}")),
                        started_at.elapsed(),
                    );
                    return Err(::anyhow::anyhow!("worker task failed: {e}"));
                }
            };
            usage.record(
                &source_id,
                match &result {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("{e:#}")),
                },
                started_at.elapsed(),
            );
            result
        }
    };
}

#[macro_export]
macro_rules! call_cleanup {
    (
        blocking = $blocking:expr,
        func = $func:expr,
        args = ($($args:expr),*),
        free = [$($descriptor:expr),*],
        as $result_ty:ty,
        parse = $parse_fn:expr
    ) => {{
        let call_result = $func.call(
            $blocking.engine_store_mut()?,
            ($($args),*),
        );

        match call_result {
            Ok(result_descriptor) => {
                let instance = $blocking.engine_instance()?;
                let parsed: Result<$result_ty> = {
                    let store: &mut Store<WasmStore> = $blocking.engine_store_mut()?;
                    $parse_fn(result_descriptor, store, instance)
                };

                {
                    let store_mut = $blocking.engine_store_mut()?.data_mut();
                    $(store_mut.take_std_value($descriptor as usize);)*
                    $blocking.free_result(result_descriptor);
                }

                parsed
            }
            Err(e) => {
                {
                    let store_mut = $blocking.engine_store_mut()?.data_mut();
                    $(store_mut.take_std_value($descriptor as usize);)*
                }

                Err(anyhow::anyhow!("wasm call failed: {}", e))
            }
        }
    }};
}

impl Source {
    pub fn from_aix_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Self> {
        #[cfg(feature = "all")]
        let blocking_source = BlockingSource::from_aix_file(path, manager, arc_manager)?;

        #[cfg(not(feature = "all"))]
        let blocking_source = BlockingSource::from_aix_file(path, manager, arc_manager)?;

        let features = { blocking_source.features.clone() };

        Ok(Self {
            backend: SourceBackend::Aidoku(Arc::new(Mutex::new(blocking_source))),
            features,
            usage: ResourceRegistry::default(),
        })
    }

    /// Loads an LNReader plugin (`*.lnreader.js`) from disk.
    pub fn from_lnreader_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Self> {
        let mut source = LnReaderSource::from_lnreader_file(path, manager, arc_manager)?;
        let features = source.features.clone();
        let usage = ResourceRegistry::default();
        // The worker records the QuickJS memory through the source's own
        // registry, so both sides share one handle.
        source.usage = usage.clone();
        Ok(Self {
            backend: SourceBackend::LnReader(Arc::new(source)),
            features,
            usage,
        })
    }

    /// Loads a MangaYomi extension (`*.mangayomi.dart`) from disk.
    pub fn from_mangayomi_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Self> {
        let mut source = MangayomiSource::from_mangayomi_file(path, manager, arc_manager)?;
        let features = source.features.clone();
        let usage = ResourceRegistry::default();
        // The worker records the runtime memory through the source's own
        // registry, so both sides share one handle.
        source.usage = usage.clone();
        Ok(Self {
            backend: SourceBackend::Mangayomi(Arc::new(source)),
            features,
            usage,
        })
    }

    /// Loads the sources bundled in a keiyoushi extension APK
    /// (`*.keiyoushi.apk`) from disk. One APK can bundle several sources
    /// (usually one per language), each of which becomes its own [`Source`].
    pub fn from_keiyoushi_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Vec<Self>> {
        let sources = KeiyoushiSource::from_keiyoushi_apk(path, manager, arc_manager)?;
        Ok(sources
            .into_iter()
            .map(|mut source| {
                let features = source.features.clone();
                let usage = ResourceRegistry::default();
                // The worker records the VM memory estimate through the
                // source's own registry, so both sides share one handle.
                source.usage = usage.clone();
                Source {
                    backend: SourceBackend::Keiyoushi(Arc::new(source)),
                    features,
                    usage,
                }
            })
            .collect())
    }

    pub fn manifest(&self) -> SourceManifest {
        // FIXME we dont actually need to clone here but yeah it's easier
        match &self.backend {
            SourceBackend::Aidoku(blocking_source) => blocking_source
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .manifest
                .clone(),
            SourceBackend::LnReader(lnreader) => lnreader.manifest(),
            SourceBackend::Mangayomi(mangayomi) => mangayomi.manifest(),
            SourceBackend::Keiyoushi(keiyoushi) => keiyoushi.manifest.clone(),
        }
    }

    pub fn setting_definitions(&self) -> Vec<SettingDefinition> {
        match &self.backend {
            SourceBackend::Aidoku(blocking_source) => blocking_source
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .setting_definitions
                .clone(),
            SourceBackend::LnReader(lnreader) => lnreader.setting_definitions(),
            SourceBackend::Mangayomi(mangayomi) => mangayomi.setting_definitions(),
            SourceBackend::Keiyoushi(keiyoushi) => keiyoushi.setting_definitions.clone(),
        }
    }

    /// Runs the backend probe eagerly. Used by the install pipeline so the
    /// probe cache is written right away and the source is fully probed from
    /// the start (later loads then read the cache). WASM and keiyoushi
    /// sources are probed at load time already and need no-op here.
    pub fn probe(&self) -> Result<()> {
        match &self.backend {
            SourceBackend::LnReader(lnreader) => lnreader.ensure_probed().map(|_| ()),
            SourceBackend::Mangayomi(mangayomi) => mangayomi.ensure_probed().map(|_| ()),
            SourceBackend::Aidoku(_) | SourceBackend::Keiyoushi(_) => Ok(()),
        }
    }

    pub fn write_meta_file(
        path: &Path,
        source_of_source: String,
        languages: Option<Vec<String>>,
    ) -> anyhow::Result<()> {
        fs::write(
            BlockingSource::meta_source_path(path)?,
            serde_json::to_string(&SourceMeta {
                source_of_source: Some(source_of_source),
                is_next_sdk: None,
                languages,
            })?,
        )
        .context("while writing meta file")
    }

    wrap_blocking_source_fn!(
        get_manga_list,
        Result<Vec<Manga>>,
        cancellation_token: CancellationToken,
        listing: aidoku::Listing
    );

    wrap_blocking_source_fn!(
        search_mangas,
        Result<(Vec<Manga>, bool)>,
        cancellation_token: CancellationToken,
        query: String,
        page: i32
    );

    wrap_blocking_source_fn!(
        get_manga_details,
        Result<Manga>,
        cancellation_token: CancellationToken,
        manga_id: String
    );

    wrap_blocking_source_fn!(
        get_chapter_list,
        Result<Vec<Chapter>>,
        cancellation_token: CancellationToken,
        manga_id: String
    );

    wrap_blocking_source_fn!(
        get_page_list,
        Result<Vec<Page>>,
        cancellation_token: CancellationToken,
        manga_id: String,
        chapter_id: String,
        chapter_num: Option<f32>
    );

    /// Fetches the plaintext bytes of a page image. Keiyoushi sources run
    /// the request through the extension's own OkHttpClient inside the
    /// persistent engine session (`getPageList` is parsed once per chapter,
    /// then every image of the chapter), so client-side interceptors
    /// (IMGX-style decryption, per-host auth) apply; the remaining backends
    /// keep the plain GET path via
    /// [`get_image_request`](Self::get_image_request).
    pub async fn fetch_page_image(&self, chapter_id: &str, url: &str) -> Result<Vec<u8>, String> {
        match &self.backend {
            SourceBackend::Keiyoushi(keiyoushi) => {
                let keiyoushi = keiyoushi.clone();
                let chapter_id = chapter_id.to_string();
                let url = url.to_string();
                tokio::task::spawn_blocking(move || {
                    keiyoushi
                        .fetch_page_image(&chapter_id, &url)
                        .map_err(|err| err.to_string())
                })
                .await
                .unwrap_or_else(|err| Err(err.to_string()))
            }
            _ => Err("fetch_page_image is only supported by keiyoushi sources".to_string()),
        }
    }

    wrap_blocking_source_fn!(
        get_image_request,
        Result<Request>,
        url: Url,
        ctx: Option<aidoku::PageContext>
    );

    wrap_blocking_source_fn!(
        process_page_image,
        Result<Vec<u8>>,
        cancellation_token: CancellationToken,
        request: (Url, HeaderMap),
        response: (StatusCode, HeaderMap),
        bytes: Bytes,
        ctx: Option<aidoku::PageContext>
    );

    wrap_blocking_source_fn!(
        get_search_manga_list_next,
        Result<NextMangaPageResult>,
        cancellation_token: CancellationToken,
        query: String,
        page: i32,
        filters: Vec<aidoku::FilterValue>
    );

    wrap_blocking_source_fn!(
        get_manga_update_next,
        Result<aidoku::Manga>,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        needs_details: bool,
        needs_chapters: bool
    );

    wrap_blocking_source_fn!(
        get_page_list_next,
        Result<Vec<aidoku::Page>>,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        chapter: aidoku::Chapter
    );

    wrap_blocking_source_fn!(
        get_image_request_next,
        Result<Request>,
        url: Url,
        ctx: Option<aidoku::PageContext>
    );

    wrap_blocking_source_fn!(
        get_manga_list_next,
        Result<NextMangaPageResult>,
        cancellation_token: CancellationToken,
        listing: aidoku::Listing,
        page: i32
    );

    wrap_blocking_source_fn!(
        handle_notification_next,
        Result<()>,
        cancellation_token: CancellationToken,
        key: String
    );
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SourceInfo {
    pub id: String,
    pub lang: Option<String>,
    pub languages: Option<Vec<String>>,
    #[cfg(not(feature = "all"))]
    #[serde(rename = "contentRating")]
    pub content_rating: Option<i32>,
    pub name: String,
    pub version: serde_json::Value,
    pub url: Option<String>,
    pub urls: Option<Vec<String>>,
    #[serde(rename = "minAppVersion")]
    pub min_app_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceConfig {
    #[serde(rename = "allowsBaseUrlSelect")]
    pub allows_base_url_select: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceManifest {
    pub info: SourceInfo,
    pub config: Option<SourceConfig>,
    #[serde(skip)]
    pub source_of_source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SourceFeatures {
    pub process_page_image: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceMeta {
    #[serde(rename = "from")]
    pub source_of_source: Option<String>,
    pub is_next_sdk: Option<bool>,
    /// The languages selected at install time for a multi-source keiyoushi
    /// APK; `None` (or missing) keeps every bundled source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<String>>,
}

fn get_memory(instance: Instance, store: &mut Store<WasmStore>) -> Result<Memory> {
    match instance.get_export(store, "memory") {
        Some(Extern::Memory(memory)) => Ok(memory),
        _ => bail!("failed to get memory"),
    }
}

/// from aidoku sdk
/// A page of manga entries.
#[derive(Default, Clone, Debug, PartialEq, Deserialize)]
pub struct NextMangaPageResult {
    /// List of manga entries.
    pub entries: Vec<aidoku::Manga>,
    /// Whether the next page is available or not.
    pub has_next_page: bool,
}

#[cfg(not(feature = "all"))]
pub struct BlockingSource {
    pub id: String,
    /// Lazily booted engine: `None` until the first call. The module is
    /// compiled and instantiated from the `.aix` file on first use.
    pub store: Option<Store<WasmStore>>,
    pub instance: Option<Instance>,
    pub manifest: SourceManifest,
    pub setting_definitions: Vec<SettingDefinition>,
    pub next_sdk: bool,
    pub features: SourceFeatures,
    path: PathBuf,
    source_settings: Option<SourceSettings>,
    manager_settings: Settings,
    /// The SDK mode recorded in the sidecar meta file after the first boot
    /// (`None` until then), so the first boot attempt matches the mode the
    /// module actually instantiated with, without re-detecting.
    aidoku_sdk_next_from_meta: Option<bool>,
}
#[cfg(feature = "all")]
pub struct BlockingSource {
    id: String,
    /// Lazily booted engine: `None` until the first call. The module is
    /// compiled and instantiated from the `.aix` file on first use.
    store: Option<Store<WasmStore>>,
    instance: Option<Instance>,
    manifest: SourceManifest,
    setting_definitions: Vec<SettingDefinition>,
    pub next_sdk: bool,
    pub features: SourceFeatures,
    path: PathBuf,
    source_settings: Option<SourceSettings>,
    manager_settings: Settings,
    /// The SDK mode recorded in the sidecar meta file after the first boot
    /// (`None` until then), so the first boot attempt matches the mode the
    /// module actually instantiated with, without re-detecting.
    aidoku_sdk_next_from_meta: Option<bool>,
}

impl BlockingSource {
    /// Loads a source archive from an AIX file without booting its WASM
    /// engine; the engine is compiled and instantiated lazily on first use
    /// by [`BlockingSource::ensure_booted`].
    pub fn from_aix_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Self> {
        let file =
            fs::File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("couldn't open source archive {}", path.display()))?;

        let manifest_file = archive
            .by_name("Payload/source.json")
            .with_context(|| "while loading source.json")?;
        let (manifest, aidoku_sdk_next_from_meta): (SourceManifest, Option<bool>) = {
            let mut manifest: SourceManifest = serde_json::from_reader(manifest_file)?;

            let meta_file = Self::meta_source_path(path)?;

            let mut is_next_sdk = None;
            if fs::exists(&meta_file).unwrap_or(false) {
                let meta: Option<SourceMeta> = serde_json::from_str(
                    &fs::read_to_string(&meta_file)
                        .with_context(|| format!("failed to read file: {:?}", path))?,
                )
                .map(Some)
                .unwrap_or(None);

                if let Some(meta) = meta {
                    manifest.source_of_source = meta.source_of_source;
                    is_next_sdk = meta.is_next_sdk;
                }
            }

            (manifest, is_next_sdk)
        };

        let url_settings = {
            let manifest = manifest.clone();
            manifest.info.urls.map(|urls| SettingDefinition::Select {
                title: "URL".to_owned(),
                key: "url".to_owned(),
                default: Some(urls.first().unwrap_or(&"".to_owned()).to_string()),
                values: urls,
                titles: None,
            })
        };
        let url_settings_support = url_settings.is_some();

        let mut setting_definitions: Vec<SettingDefinition> =
            if let Ok(file) = archive.by_name("Payload/settings.json") {
                serde_json::from_reader(file).map_err(|err| {
                    eprintln!("read file settings.json failed {}", err);

                    err
                })?
            } else {
                Vec::new()
            };
        if let Some(url) = url_settings {
            setting_definitions.insert(0, url);
        }

        let stored_source_settings = manager
            .settings
            .source_settings
            .get(&manifest.info.id)
            .cloned()
            .unwrap_or_default();

        let id = { manifest.info.id.clone() };

        let source_settings = SourceSettings::new(
            id.clone(),
            &setting_definitions,
            &stored_source_settings,
            arc_manager,
        )?;
        if !url_settings_support && source_settings.get(&"url".to_string()).is_none() {
            if let Some(url) = manifest.info.url.clone() {
                source_settings.set("url", SourceSettingValue::String(url));
            }
        }

        // The engine is not booted here: `ensure_booted` compiles and
        // instantiates the module from the `.aix` file on the first actual
        // call, so load time never runs the wasm runtime.
        Ok(Self {
            id,
            store: None,
            instance: None,
            manifest,
            next_sdk: false,
            setting_definitions,
            features: SourceFeatures {
                process_page_image: false,
            },
            path: path.to_path_buf(),
            source_settings: Some(source_settings),
            manager_settings: manager.settings.clone(),
            aidoku_sdk_next_from_meta,
        })
    }

    /// Boots the engine from the `.aix` file on first use, retrying with the
    /// opposite SDK mode when instantiation fails (the legacy fallback that
    /// used to live in [`Self::from_aix_file`]).
    fn ensure_booted(&mut self) -> Result<()> {
        if self.instance.is_some() {
            return Ok(());
        }
        let sdk_next = self
            .aidoku_sdk_next_from_meta
            .unwrap_or_else(|| Self::is_aidoku_sdk_next(&self.manifest.info.min_app_version));
        let (mut store, instance, sdk_next) = match self.boot(sdk_next) {
            Ok((store, instance)) => (store, instance, sdk_next),
            Err(error) => {
                let retry = !sdk_next;
                let (store, instance) = self.boot(retry).map_err(|retry_error| {
                    anyhow!(
                        "failed instantiating {} ({}): {retry_error:#} (first attempt: {error:#})",
                        self.id,
                        if sdk_next { "next" } else { "legacy" }
                    )
                })?;
                (store, instance, retry)
            }
        };

        self.features.process_page_image = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "process_page_image")
            .map(|_| true)
            .ok()
            .unwrap_or_default();
        self.next_sdk = sdk_next;

        if self.aidoku_sdk_next_from_meta != Some(sdk_next) {
            let meta_file = Self::meta_source_path(&self.path)?;
            fs::write(
                &meta_file,
                serde_json::to_string(&SourceMeta {
                    source_of_source: self.manifest.source_of_source.clone(),
                    is_next_sdk: Some(sdk_next),
                    languages: None,
                })?,
            )
            .with_context(|| format!("failed persisting SDK mode for {}", self.id))?;
        }

        self.store = Some(store);
        self.instance = Some(instance);

        // Aidoku SDK-next sources run a `start` init function once the
        // module is live; it used to run right after install.
        if sdk_next {
            if let Err(error) = self.start() {
                // Roll back the boot so a later call re-boots the engine
                // instead of exiting through the `instance.is_some()` fast
                // path with a partially initialized module.
                self.store = None;
                self.instance = None;
                self.next_sdk = false;
                return Err(error);
            }
        }
        // The engine now owns its own copy; drop the load-time snapshot.
        self.source_settings = None;
        Ok(())
    }

    /// Returns the wasm store, erroring when the engine has not been booted.
    fn engine_store_mut(&mut self) -> Result<&mut Store<WasmStore>> {
        self.store
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("wasm engine is not booted"))
    }

    /// Returns the wasm store, erroring when the engine has not been booted.
    fn engine_store(&self) -> Result<&Store<WasmStore>> {
        self.store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("wasm engine is not booted"))
    }

    /// Returns the wasm instance, erroring when the engine has not been booted.
    fn engine_instance(&self) -> Result<Instance> {
        self.instance
            .ok_or_else(|| anyhow::anyhow!("wasm engine is not booted"))
    }

    /// Compiles and instantiates the module in the requested SDK mode.
    fn boot(&mut self, aidoku_sdk_next: bool) -> Result<(Store<WasmStore>, Instance)> {
        let mut archive = ZipArchive::new(
            fs::File::open(&self.path)
                .with_context(|| format!("couldn't open {}", self.path.display()))?,
        )
        .with_context(|| format!("couldn't open source archive {}", self.path.display()))?;

        let mut wasm_bytes = Vec::new();
        archive
            .by_name("Payload/main.wasm")
            .with_context(|| "while loading main.wasm")?
            .read_to_end(&mut wasm_bytes)
            .with_context(|| {
                format!("failed reading wasm from zip entry {}", self.path.display())
            })?;

        let engine = Engine::default();
        let wasm_store = WasmStore::new(
            self.manifest.info.id.clone(),
            self.source_settings
                .clone()
                .ok_or_else(|| anyhow::anyhow!("source settings are missing"))?,
            self.manager_settings.clone(),
        );
        let mut store = Store::new(&engine, wasm_store);

        let module = Module::new(&engine, &wasm_bytes)
            .with_context(|| format!("failed loading module from {}", self.path.display()))?;

        let mut linker = Linker::new(&engine);

        if aidoku_sdk_next {
            // register_aidoku_imports(&mut linker)?;
            // register_json_imports(&mut linker)?;
            sdk_next::register_std_imports(&mut linker)?; // ok
            sdk_next::register_canvas_imports(&mut linker)?; // check
            sdk_next::register_defaults_imports(&mut linker)?; // ok
            sdk_next::register_env_imports(&mut linker)?; // ok
            sdk_next::register_html_imports(&mut linker)?;
            sdk_next::register_js_imports(&mut linker)?;
            sdk_next::register_net_imports(&mut linker)?;
        } else {
            register_aidoku_imports(&mut linker)?;
            register_defaults_imports(&mut linker)?;
            register_env_imports(&mut linker)?;
            register_html_imports(&mut linker)?;
            register_json_imports(&mut linker)?;
            register_net_imports(&mut linker)?;
            register_std_imports(&mut linker)?;
        }

        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .with_context(|| format!("failed creating instance from {}", self.path.display()))?;

        Ok((store, instance))
    }

    pub fn meta_source_path(path: &Path) -> anyhow::Result<std::path::PathBuf> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("AIX file has no parent directory"))?;

        let file_stem = path
            .file_stem()
            .ok_or_else(|| anyhow::anyhow!("AIX file has no filename stem"))?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Filename is not valid UTF-8"))?;

        // Build ".{filename}.source"
        let meta_name = format!(".{}.source", file_stem);

        Ok(parent.join(meta_name))
    }

    fn is_aidoku_sdk_next(min: &Option<String>) -> bool {
        use semver::Version;
        // parse "0.7" into a SemVer
        let target = Version::new(0, 7, 0);

        match min {
            Some(v) => {
                // Safely parse user's version
                match Version::parse(v) {
                    Ok(parsed) => parsed >= target,
                    Err(_) => false, // invalid version string → treat as old
                }
            }
            None => false,
        }
    }

    pub fn get_manga_list(
        &mut self,
        cancellation_token: CancellationToken,
        listing: aidoku::Listing,
    ) -> Result<Vec<Manga>> {
        self.ensure_booted()?;
        if self.next_sdk {
            return self
                .get_manga_list_next(cancellation_token, listing, 1)
                .map(|list| {
                    list.entries
                        .into_iter()
                        .map(|v| Manga::from(v, self.id.clone()))
                        .collect::<Vec<_>>()
                });
        }
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.search_mangas_by_filters_inner(vec![])
        })
    }

    pub fn search_mangas(
        &mut self,
        cancellation_token: CancellationToken,
        query: String,
        page: i32,
    ) -> Result<(Vec<Manga>, bool)> {
        self.ensure_booted()?;
        if self.next_sdk {
            return self
                .get_search_manga_list_next(cancellation_token, query, page, [].to_vec())
                .map(|list| {
                    let mangas = list
                        .entries
                        .into_iter()
                        .map(|v| Manga::from(v, self.id.clone()))
                        .collect::<Vec<_>>();
                    (mangas, list.has_next_page)
                });
        }
        if page > 1 {
            return Ok((Vec::new(), false));
        }
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.search_mangas_by_filters_inner(vec![Filter::Title(query)])
        })
        .map(|mangas| (mangas, false))
    }

    fn search_mangas_by_filters_inner(&mut self, filters: Vec<Filter>) -> Result<Vec<Manga>> {
        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<(i32, i32), i32>(&mut self.engine_store_mut()?, "get_manga_list")?;
        let filters_descriptor = self.engine_store_mut()?.data_mut().store_std_value(
            Value::from(
                filters
                    .iter()
                    .map(|filter| Value::Object(ObjectValue::Filter(filter.clone())))
                    .collect::<Vec<_>>(),
            )
            .into(),
            None,
        );

        let mangas = call_cleanup!(
            blocking = self,
            func = wasm_function,
            args = (filters_descriptor as i32, 1),
            free = [filters_descriptor],
            as Vec<Manga>,
            parse = |descriptor, store: &mut Store<WasmStore>, _| {
                match store.data_mut()
                    .get_std_value(descriptor as usize)
                    .ok_or(anyhow!("could not read data from page descriptor"))?
                    .as_ref()
                {
                    Value::Object(ObjectValue::MangaPageResult(MangaPageResult {
                        manga: mangas, ..
                    })) => Ok(mangas.clone()),
                    other => bail!(
                        "expected page descriptor to be an array, found {:?} instead",
                        other
                    ),
                }
            }
        )?;

        Ok(mangas)
    }

    pub fn get_manga_details(
        &mut self,
        cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Manga> {
        self.ensure_booted()?;
        if self.next_sdk {
            return self
                .get_manga_update_next(
                    cancellation_token,
                    BlockingSource::create_aidoku_manga(manga_id),
                    true,
                    false,
                )
                .map(|v| Manga::from(v, self.id.clone()));
        }
        self.run_under_context(
            cancellation_token,
            OperationContextObject::Manga {
                id: manga_id.clone(),
            },
            |this| this.get_manga_details_inner(manga_id),
        )
    }

    fn get_manga_details_inner(&mut self, manga_id: String) -> Result<Manga> {
        // HACK aidoku actually places the entire `Manga` object into the store, but it seems only
        // the `id` field is needed, so we just store a `HashMap` with the `id` set.
        // surely this wont break in the future!
        let mut manga_hashmap = ValueMap::new();
        manga_hashmap.insert("id".to_string(), manga_id.into());

        let manga_descriptor = self.engine_store_mut()?.data_mut().store_std_value(
            Value::Object(ObjectValue::ValueMap(manga_hashmap)).into(),
            None,
        );

        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<i32, i32>(&mut self.engine_store_mut()?, "get_manga_details")?;

        let manga = call_cleanup!(
            blocking = self,
            func = wasm_function,
            args = (manga_descriptor as i32),
            free = [manga_descriptor],
            as Manga,
            parse = |descriptor, store: &mut Store<WasmStore>, _| {
                match store.data_mut()
                    .get_std_value(descriptor as usize)
                    .ok_or(anyhow!("could not read data from manga details descriptor"))?
                    .as_ref()
                {
                    Value::Object(ObjectValue::Manga(manga)) => Ok(manga.clone()),
                    other => bail!(
                    "expected manga details descriptor to be a manga object, found {:?} instead",
                    other
                ),
                }
            }
        )?;

        Ok(manga)
    }

    fn create_aidoku_manga(manga_id: String) -> aidoku::Manga {
        aidoku::Manga {
            key: manga_id,
            title: "".to_owned(),
            cover: None,
            artists: None,
            authors: None,
            description: None,
            url: None,
            tags: None,
            status: aidoku::MangaStatus::Unknown,
            content_rating: aidoku::ContentRating::Unknown,
            viewer: aidoku::Viewer::Unknown,
            update_strategy: aidoku::UpdateStrategy::Never,
            next_update_time: None,
            chapters: None,
        }
    }
    fn create_aidoku_chapter(chapter_id: String) -> aidoku::Chapter {
        aidoku::Chapter {
            key: chapter_id,
            title: None,
            chapter_number: None,
            volume_number: None,
            date_uploaded: None,
            scanlators: None,
            url: None,
            language: None,
            thumbnail: None,
            locked: false,
        }
    }

    pub fn get_chapter_list(
        &mut self,
        cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<Chapter>> {
        self.ensure_booted()?;
        if self.next_sdk {
            return self
                .get_manga_update_next(
                    cancellation_token,
                    BlockingSource::create_aidoku_manga(manga_id.clone()),
                    false,
                    true,
                )
                .map(|manga| {
                    manga
                        .chapters
                        .unwrap_or_default()
                        .into_iter()
                        .map(|v| Chapter::from(v, self.id.clone(), manga_id.clone()))
                        .collect::<Vec<_>>()
                });
        }
        self.run_under_context(
            cancellation_token,
            OperationContextObject::Manga {
                id: manga_id.clone(),
            },
            |this| this.get_chapter_list_inner(manga_id),
        )
    }

    fn get_chapter_list_inner(&mut self, manga_id: String) -> Result<Vec<Chapter>> {
        // HACK aidoku actually places the entire `Manga` object into the store, but it seems only
        // the `id` field is needed, so we just store a `HashMap` with the `id` set.
        // surely this wont break in the future!
        let mut manga_hashmap = ValueMap::new();
        manga_hashmap.insert("id".to_string(), manga_id.into());

        let manga_descriptor = self.engine_store_mut()?.data_mut().store_std_value(
            Value::Object(ObjectValue::ValueMap(manga_hashmap)).into(),
            None,
        );

        // FIXME what the fuck is chapter counter, aidoku sets it here
        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<i32, i32>(&mut self.engine_store_mut()?, "get_chapter_list")?;

        let chapters = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (manga_descriptor as i32),
        free = [manga_descriptor],
        as  Vec<Chapter>,
        parse = |chapter_list_descriptor, store: &mut Store<WasmStore>, _| {
            Ok(match store.data_mut()
                .get_std_value(chapter_list_descriptor as usize)
                .ok_or(anyhow!("could not read data from chapter list descriptor"))?
                .as_ref() {
                    Value::Array(array) => array
                        .iter()
                        .enumerate()
                        .map(|(index, v)| match v {
                            Value::Object(ObjectValue::Chapter(chapter)) => {
                                let mut chapter = chapter.clone();

                                if chapter.title.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                                    chapter.title = Some(format!(
                                        "Ch.{}",
                                        chapter
                                            .chapter_num
                                            .unwrap_or(chapter.volume_num.unwrap_or(index as f32))
                                    ));
                                }

                                Some(chapter)
                            }
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()
                        .ok_or(anyhow!("unexpected element in chapter array"))?,
                    other => bail!(
                        "expected page descriptor to be an array, found {:?} instead",
                        other
                    ),
                })
        })?;

        Ok(chapters)
    }

    pub fn get_page_list(
        &mut self,
        cancellation_token: CancellationToken,
        manga_id: String,
        chapter_id: String,
        chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        self.ensure_booted()?;
        if self.next_sdk {
            return self
                .get_page_list_next(
                    cancellation_token,
                    BlockingSource::create_aidoku_manga(manga_id.clone()),
                    BlockingSource::create_aidoku_chapter(chapter_id),
                )
                .map(|pages| {
                    pages
                        .into_iter()
                        .enumerate()
                        .map(|(index, page)| {
                            Page::from(index, page, self.id.clone(), manga_id.clone())
                        })
                        .collect()
                });
        }
        self.run_under_context(
            cancellation_token,
            OperationContextObject::Chapter {
                id: chapter_id.clone(),
            },
            |this| this.get_page_list_inner(manga_id, chapter_id, chapter_num),
        )
    }

    fn get_page_list_inner(
        &mut self,
        manga_id: String,
        chapter_id: String,
        chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        // HACK the same thing with the `Manga` said above, we also usually only need the `id`
        // from the `Chapter` object and the `mangaId`.
        let mut chapter_hashmap = ValueMap::new();
        chapter_hashmap.insert("id".to_string(), Value::String(chapter_id));
        chapter_hashmap.insert("mangaId".to_string(), Value::String(manga_id));

        // HACK guya sources actually use the `chapterNum` field for some fucking reason????
        // like it's a huge fucking hack it's not even by accident XD
        // ref: https://github.com/Skittyblock/aidoku-community-sources/blob/bd79840e182ff7c90c8444ed160e2e8d50b6a219/src/rust/guya/sources/dankefurslesen/src/lib.rs#L54
        if let Some(chapter_num) = chapter_num {
            chapter_hashmap.insert("chapterNum".to_string(), Value::Float(chapter_num as f64));
        }

        let chapter_descriptor = self.engine_store_mut()?.data_mut().store_std_value(
            Value::Object(ObjectValue::ValueMap(chapter_hashmap)).into(),
            None,
        );

        // FIXME what the fuck is chapter counter, aidoku sets it here
        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<i32, i32>(&mut self.engine_store_mut()?, "get_page_list")?;

        let pages = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (chapter_descriptor as i32),
        free = [chapter_descriptor],
        as  Vec<Page>,
        parse = |page_list_descriptor, store: &mut Store<WasmStore>, _| {
            Ok(match store.data_mut()
            .get_std_value(page_list_descriptor as usize)
            .ok_or(anyhow!("could not read data from page list descriptor"))?
            .as_ref() {
                Value::Array(array) => array
                    .iter()
                    .map(|v| match v {
                        Value::Object(ObjectValue::Page(page)) => Some(page.clone()),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or(anyhow!("unexpected element in page array"))?,
                other => bail!(
                    "expected page descriptor to be an array, found {:?} instead",
                    other
                ),
            })
        })?;

        Ok(pages)
    }

    pub fn get_image_request(
        &mut self,
        url: Url,
        ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        self.ensure_booted()?;
        if self.next_sdk {
            self.get_image_request_next(url, ctx)
        } else {
            self.get_image_request_inner(url)
        }
    }
    pub fn get_image_request_inner(&mut self, url: Url) -> Result<Request> {
        let request_descriptor = self.engine_store_mut()?.data_mut().create_request();

        // FIXME scoping here is so fucking scuffed
        {
            let request_state = &mut self
                .engine_store_mut()?
                .data_mut()
                .get_mut_request(request_descriptor)
                .ok_or_else(|| anyhow::anyhow!("failed to get mutable request state"))?;

            let request_building_state = match request_state {
                RequestState::Building(building_state) => building_state,
                _ => return Err(anyhow::anyhow!("expected request to be in Building state")),
            };

            request_building_state.method = Some(Method::GET);
            request_building_state.url = Some(url);

            request_building_state
                .headers
                .insert("User-Agent".to_string(), DEFAULT_USER_AGENT.to_string());
        };

        // TODO add support for cookies
        // it seems that it's fine for an extension to not have this function defined, so we only
        // call it if it exists
        {
            let instance = self.engine_instance()?;
            let mut wasm_store = self.engine_store_mut()?;

            if let Ok(wasm_function) =
                instance.get_typed_func::<i32, ()>(&mut wasm_store, "modify_image_request")
            {
                wasm_function.call(&mut wasm_store, request_descriptor as i32)?;
            }
        }

        let request_state = &mut self
            .engine_store_mut()?
            .data_mut()
            .remove_request(request_descriptor)
            .ok_or_else(|| anyhow::anyhow!("failed to remove request state"))?;

        let request_building_state = match request_state {
            RequestState::Building(building_state) => building_state,
            _ => return Err(anyhow::anyhow!("expected request to be in Building state")),
        };

        (request_building_state as &RequestBuildingState).try_into()
    }

    // next sdk

    pub fn start(&mut self) -> Result<()> {
        self.ensure_booted()?;
        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<(), ()>(&mut self.engine_store_mut()?, "start")?;

        wasm_function.call(self.engine_store_mut()?, ())?;

        Ok(())
    }
    pub fn free_result(&mut self, pointer: i32) {
        let Ok(wasm_function) = self.engine_instance().and_then(|instance| {
            let store = self.engine_store_mut()?;
            instance
                .get_typed_func::<i32, ()>(store, "free_memory")
                .map_err(anyhow::Error::from)
        }) else {
            return;
        };

        if let Err(e) = self.engine_store_mut().and_then(|store| {
            wasm_function
                .call(store, pointer)
                .map_err(anyhow::Error::from)
        }) {
            log::warn!("failed to free WASM memory at pointer {pointer}: {e}");
        }
    }

    pub fn get_search_manga_list_next(
        &mut self,
        cancellation_token: CancellationToken,
        query: String,
        page: i32,
        filters: Vec<aidoku::FilterValue>,
    ) -> Result<NextMangaPageResult> {
        self.ensure_booted()?;
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.get_search_manga_list_next_inner(query, page, filters)
        })
    }

    fn get_memory(&mut self) -> Result<Memory> {
        self.ensure_booted()?;
        match self
            .engine_instance()?
            .get_export(self.engine_store()?, "memory")
        {
            Some(Extern::Memory(memory)) => Ok(memory),
            _ => bail!("failed to get memory"),
        }
    }

    fn get_search_manga_list_next_inner(
        &mut self,
        keyword: String,
        page: i32,
        filters: Vec<FilterValue>,
    ) -> Result<NextMangaPageResult> {
        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<(i32, i32, i32), i32>(
                &mut self.engine_store_mut()?,
                "get_search_manga_list",
            )?;

        let store = self.engine_store_mut()?.data_mut();

        let keyword = store.store_std_value(Value::from(keyword).into(), None);
        let filters = store.store_std_value(Value::NextFilters(filters).into(), None);

        let result = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (keyword as i32, page, filters as i32),
        free = [keyword, filters],
        as NextMangaPageResult,
        parse = |pointer, store: &mut Store<WasmStore>, instance| {
            let memory = get_memory(instance, store)?;

            read_next::<NextMangaPageResult>(&memory, &store, pointer)
        })?;

        Ok(result)
    }

    pub fn get_manga_update_next(
        &mut self,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<aidoku::Manga> {
        self.ensure_booted()?;
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.get_manga_update_next_inner(manga, needs_details, needs_chapters)
        })
    }

    fn get_manga_update_next_inner(
        &mut self,
        manga: aidoku::Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<aidoku::Manga> {
        let store = self.engine_store_mut()?.data_mut();

        let manga = store.store_std_value(Value::NextManga(manga).into(), None);

        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<(i32, i32, i32), i32>(
                &mut self.engine_store_mut()?,
                "get_manga_update",
            )?;

        let manga_o = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (manga as i32, if needs_details { 1 } else { 0 }, if needs_chapters { 1 } else { 0 }),
        free = [manga],
        as  aidoku::Manga,
        parse = |pointer, store: &mut Store<WasmStore>, instance| {
            let memory = get_memory(instance, store)?;
            let manga_o = read_next::<aidoku::Manga>(&memory, &store, pointer)?;

            Ok(manga_o)
        })?;

        Ok(manga_o)
    }

    pub fn get_page_list_next(
        &mut self,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        chapter: aidoku::Chapter,
    ) -> Result<Vec<aidoku::Page>> {
        self.ensure_booted()?;
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.get_page_list_next_inner(manga, chapter)
        })
    }

    fn get_page_list_next_inner(
        &mut self,
        manga: aidoku::Manga,
        chapter: aidoku::Chapter,
    ) -> Result<Vec<aidoku::Page>> {
        let store = self.engine_store_mut()?.data_mut();

        let manga = store.store_std_value(Value::NextManga(manga).into(), None);
        let chapter = store.store_std_value(Value::NextChapter(chapter).into(), None);

        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<(i32, i32), i32>(&mut self.engine_store_mut()?, "get_page_list")?;

        let pages = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (manga as i32, chapter as i32),
        free = [manga, chapter],
        as  Vec<aidoku::Page>,
        parse = |pointer, store: &mut Store<WasmStore>, instance| {
            let memory = get_memory(instance, store)?;
            let pages = read_next::<Vec<aidoku::Page>>(&memory, &store, pointer)?;

            Ok(pages)
        })?;

        Ok(pages)
    }

    pub fn get_image_request_next(
        &mut self,
        url: Url,
        ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        self.get_image_request_next_inner(url, ctx)
    }

    pub fn get_image_request_next_inner(
        &mut self,
        url: Url,
        context: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        let (url_key, context_key) = {
            let store = self.engine_store_mut()?.data_mut();

            let url_key = store.store_std_value(Value::String(url.clone().into()).into(), None);
            store.mark_str_encode(url_key);
            let context_key = if let Some(context) = context {
                store.store_std_value(Value::NextPageContext(context).into(), None) as i32
            } else {
                -1
            };

            // Drops here automatically
            (url_key as i32, context_key)
        };

        let request_state_ptr = {
            let wasm_function = self.engine_instance()?.get_typed_func::<(i32, i32), i32>(
                &mut self.engine_store_mut()?,
                "get_image_request",
            );

            match wasm_function {
                Ok(func) => Some(func.call(self.engine_store_mut()?, (url_key, context_key))?),
                Err(_) => None,
            }
        };
        // Drop std_value entries now
        {
            let store = self.engine_store_mut()?.data_mut();
            store.take_std_value(url_key as usize);
            if context_key >= 0 {
                store.take_std_value(context_key as usize);
            }
        }

        let request_state_opt = if let Some(request_state_ptr) = request_state_ptr {
            if request_state_ptr < 0 {
                eprintln!("get_image_request failed");
                bail!("get_image_request failed");
            }

            let memory = self.get_memory()?;
            let req_id = read_next::<i32>(&memory, self.engine_store()?, request_state_ptr)?;
            self.free_result(request_state_ptr);

            let store = self.engine_store_mut()?.data_mut();

            store.remove_request(req_id as usize)
        } else {
            None
        };

        // Take request_state or build a fresh one
        let request_state = &mut if let Some(state) = request_state_opt {
            state
        } else {
            RequestState::Building(RequestBuildingState::default())
        };

        // Extract mutable building state
        let building_state: &mut RequestBuildingState = match request_state {
            RequestState::Building(state) => state,
            _ => return Err(anyhow::anyhow!("Not building state")),
        };

        if building_state.url.is_none() {
            building_state.url = Some(url);
        }
        if building_state.method.is_none() {
            building_state.method = Some(Method::GET);
        }

        if !building_state.headers.contains_key("User-Agent") {
            building_state
                .headers
                .insert("User-Agent".to_string(), DEFAULT_USER_AGENT.to_string());
        }

        (&*building_state).try_into()
    }

    pub fn process_page_image(
        &mut self,
        cancellation_token: CancellationToken,
        request: (Url, HeaderMap),
        response: (StatusCode, HeaderMap),
        bytes: Bytes,
        ctx: Option<aidoku::PageContext>,
    ) -> Result<Vec<u8>> {
        self.ensure_booted()?;
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.process_page_image_inner(request, response, bytes, ctx)
        })
    }

    pub fn process_page_image_inner(
        &mut self,
        request: (Url, HeaderMap),
        response: (StatusCode, HeaderMap),
        bytes: Bytes,
        context: Option<aidoku::PageContext>,
    ) -> Result<Vec<u8>> {
        let (image_id, image_ref, context_id) = {
            let store = self.engine_store_mut()?.data_mut();

            // Try to decode the image normally first. If decoding fails (e.g. the
            // bytes are encrypted like MangaPlus), store the raw bytes as a 0x0
            // image so the WASM `process_page_image` can still access them via
            // `response.image.data()` and perform decryption before creating the
            // final image.
            let image_ref = store.create_image(&bytes).unwrap_or_else(|| {
                store.set_image_data(wasm_store::ImageData {
                    raw_byte_len: bytes.len(),
                    data: bytes
                        .chunks(4)
                        .map(|chunk| {
                            let mut array = [0u8; 4];
                            for (i, &byte) in chunk.iter().enumerate() {
                                array[i] = byte;
                            }
                            u32::from_le_bytes(array)
                        })
                        .collect::<Vec<u32>>(),
                    width: 0,
                    height: 0,
                })
            });

            let image_response = ImageResponse {
                code: response.0.into(),
                headers: response
                    .1
                    .iter()
                    .map(|(k, v)| {
                        let key = k.to_string();
                        let value = v.to_str().unwrap_or("").to_string();
                        (key, value)
                    })
                    .collect(),
                request: ImageRequest {
                    url: Some(String::from(request.0)),
                    headers: request
                        .1
                        .iter()
                        .map(|(k, v)| {
                            let key = k.to_string();
                            let value = v.to_str().unwrap_or("").to_string();
                            (key, value)
                        })
                        .collect(),
                },
                image: ImageRef {
                    rid: image_ref as i32,
                    externally_managed: false,
                },
            };

            let image_id =
                store.store_std_value(Value::NextImageResponse(image_response).into(), None) as i32;

            let context_id = if let Some(context) = context {
                store.store_std_value(Value::NextPageContext(context).into(), None) as i32
            } else {
                -1
            };

            (image_id, image_ref, context_id)
        };

        let wasm_function = self.engine_instance()?.get_typed_func::<(i32, i32), i32>(
            &mut self.engine_store_mut()?,
            "process_page_image",
        )?;

        let image_data = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (image_id, context_id),
        free = [image_id, context_id, image_ref],
        as  Vec<u8>,
        parse = |pointer, store: &mut Store<WasmStore>, instance| {
            let memory = get_memory(instance, store)?;

            let Some(image_pointer) = read_next::<i32>(&memory, &store, pointer).ok() else {
                return Err(anyhow::anyhow!("pointer image error {pointer}"));
            };

            let image_data = {
                let store =store.data_mut();
                let (width, height, pixels) = {
                    let Some(image) = store.get_image(image_pointer as usize) else {
                        return Err(anyhow::anyhow!(
                            "failed to get image for process_page_image point = {image_pointer}"
                        ));
                    };

                    // image.data は Vec<u32> の参照なので、clone して borrow を即終了する
                    (image.width as u32, image.height as u32, image.data.clone())
                };

                let pointer = usize::try_from(image_pointer)
                    .context(format!("process_page_image failed {image_pointer}"))?;
                store.take_std_value(pointer);

                let rgb_pixels = crate::source::decode_image::decode_argb_to_rgb(
                    width as i32, height as i32, &pixels,
                )?;
                let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
                comp.set_size(width as usize, height as usize);
                comp.set_fastest_defaults();

                let mut comp =  comp.start_compress(Vec::new())?;
                comp.write_scanlines(&rgb_pixels)?;


                comp.finish()?
            };

            Ok(image_data)
        })?;

        Ok(image_data)
    }

    pub fn get_manga_list_next(
        &mut self,
        cancellation_token: CancellationToken,
        listing: aidoku::Listing,
        page: i32,
    ) -> Result<NextMangaPageResult> {
        self.ensure_booted()?;
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.get_manga_list_next_inner(listing, page)
        })
    }

    fn get_manga_list_next_inner(
        &mut self,
        listing: aidoku::Listing,
        page: i32,
    ) -> Result<NextMangaPageResult> {
        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<(i32, i32), i32>(&mut self.engine_store_mut()?, "get_manga_list")?;

        let store = self.engine_store_mut()?.data_mut();

        let listing = store.store_std_value(Value::NextListing(listing).into(), None);

        let result = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (listing as i32, page),
        free = [listing],
        as NextMangaPageResult,
        parse = |pointer, store: &mut Store<WasmStore>, instance| {
            let memory = get_memory(instance, store)?;

            read_next::<NextMangaPageResult>(&memory, &store, pointer)
        })?;

        Ok(result)
    }

    pub fn handle_notification_next(
        &mut self,
        cancellation_token: CancellationToken,
        key: String,
    ) -> Result<()> {
        self.ensure_booted()?;
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.handle_notification_next_inner(key)
        })
    }

    fn handle_notification_next_inner(&mut self, key: String) -> Result<()> {
        let wasm_function = self
            .engine_instance()?
            .get_typed_func::<i32, i32>(&mut self.engine_store_mut()?, "handle_notification")?;

        let store = self.engine_store_mut()?.data_mut();

        let key = store.store_std_value(Value::from(key).into(), None);

        wasm_function.call(self.engine_store_mut()?, key as i32)?;
        self.engine_store_mut()?.data_mut().take_std_value(key);

        Ok(())
    }

    pub fn run_under_context<T, F>(
        &mut self,
        cancellation_token: CancellationToken,
        current_object: OperationContextObject,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut Self) -> Result<T>,
    {
        self.ensure_booted()?;
        self.engine_store_mut()?.data_mut().context = OperationContext {
            cancellation_token,
            current_object,
        };

        let result = f(self);

        self.engine_store_mut()?.data_mut().context = OperationContext::default();

        result
    }
}
