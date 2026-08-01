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
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{header::HeaderMap, Method, Request};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    settings::SourceSettingValue,
    source::{
        model::{Manga, Page, SettingDefinition},
        SourceFeatures, SourceManifest,
    },
    source_manager::SourceManager,
};

use self::{
    convert::{
        chapters, manga_from_novel, mangas_from_search, page_from_chapter_html, HTML_MARKER,
    },
    manifest::{manifest_from_props, parse_props, setting_definitions, PluginProps},
    runtime::{LnReaderRuntime, DEFAULT_INVOKE_TIMEOUT},
};

/// Default user agent used for plugin requests. The LNReader app sends a
/// device-specific UA; a modern desktop Chrome UA works for the vast majority
/// of sites and can be overridden by the plugin itself.
pub(crate) const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36";

/// The suffix of installed plugin files (`<id>.lnreader.js`).
pub(crate) const LNREADER_FILE_SUFFIX: &str = ".lnreader.js";

/// A single LNReader plugin exposed through the RakuYomi source API.
///
/// All method calls block on the underlying JS runtime; the async wrappers
/// used by the rest of the codebase run them inside `spawn_blocking`.
pub struct LnReaderSource {
    pub id: String,
    pub manifest: SourceManifest,
    pub setting_definitions: Vec<SettingDefinition>,
    pub features: SourceFeatures,
    props: PluginProps,
    /// Merged source settings (stored values overlaid on the definition
    /// defaults), used to build the filter/settings JSON for each call.
    settings: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
    runtime: LnReaderRuntime,
}

impl std::fmt::Debug for LnReaderSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LnReaderSource")
            .field("id", &self.id)
            .field("manifest", &self.manifest)
            .field("setting_definitions", &self.setting_definitions)
            .field("props", &self.props)
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
    /// Loads a plugin from a `*.lnreader.js` file and prepares the runtime.
    pub fn from_lnreader_file(path: &Path, manager: &SourceManager) -> Result<Self> {
        let plugin_code = fs::read_to_string(path)
            .with_context(|| format!("failed to read plugin file {}", path.display()))?;

        // The runtime needs the plugin id up front (storage namespacing);
        // the site is only known after the props are evaluated.
        let plugin_id = plugin_id_from_path(path)?;
        let runtime = LnReaderRuntime::new(
            plugin_id,
            plugin_code,
            String::new(),
            DEFAULT_USER_AGENT.to_string(),
            DEFAULT_INVOKE_TIMEOUT,
        )?;

        let props_json = runtime
            .invoke("props", "[]")
            .context("plugin `props` failed")?;
        let props = parse_props(&props_json)?;
        let manifest = manifest_from_props(&props, Self::read_source_of_source(path)?);

        let setting_definitions = setting_definitions(&props.plugin_settings)?;

        // A `url`-like Select mirrors what wasm sources do for base URLs.
        // LNReader plugins have no such concept, so nothing is added.

        let stored_settings = manager
            .settings
            .source_settings
            .get(&props.id)
            .cloned()
            .unwrap_or_default();

        let mut settings: HashMap<String, SourceSettingValue> = HashMap::new();
        for definition in &setting_definitions {
            collect_defaults(definition, &mut settings);
        }
        for (key, value) in stored_settings {
            settings.insert(key, value);
        }

        let settings = Arc::new(Mutex::new(settings));

        // Seed the plugin's `@libs/storage` with the pluginSettings values,
        // mirroring how the LNReader app stores them (`pluginId_DB_key`).
        runtime.seed_storage(Self::storage_seed(
            &props.id,
            &props.plugin_settings,
            &settings,
        )?);

        Ok(Self {
            id: props.id.clone(),
            manifest,
            setting_definitions,
            features: SourceFeatures {
                process_page_image: false,
            },
            props,
            settings,
            runtime,
        })
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
        settings: &Arc<Mutex<HashMap<String, SourceSettingValue>>>,
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
                .unwrap()
                .get(key)
                .cloned()
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
    pub fn settings_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for (key, value) in self.settings.lock().unwrap().iter() {
            out.insert(key.clone(), setting_to_json(value));
        }
        Value::Object(out)
    }

    /// Invokes a plugin method with JSON args and parses the JSON response.
    pub fn invoke(&self, method: &str, args: Value) -> Result<Value> {
        let output = self
            .runtime
            .invoke(method, &args.to_string())
            .with_context(|| format!("plugin method `{}` failed", method))?;
        Ok(serde_json::from_str(&output)?)
    }

    /// Resolves a path to an absolute URL the way the app does
    /// (`resolveUrl`, falling back to `site + path`).
    fn resolve_url(&self, path: &str, is_novel: bool) -> Option<Url> {
        let out = self
            .invoke("resolveUrl", json!([path, is_novel]))
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{}{}", self.props.site, path));
        Url::parse(&out).ok()
    }

    /// Implements `get_manga_list`: the popular novels list (page 1).
    pub fn get_manga_list(
        &self,
        _cancellation_token: CancellationToken,
        listing: aidoku::Listing,
    ) -> Result<Vec<Manga>> {
        let show_latest = listing.name.eq_ignore_ascii_case("latest");
        let settings = self.settings_json();
        let value = self.invoke("popular", json!([1, settings, show_latest]))?;
        Ok(mangas_from_search(&value)?
            .into_iter()
            .map(|manga| Manga::from(manga, self.id.clone()))
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
        let value = if query.trim().is_empty() {
            let settings = self.settings_json();
            self.invoke("popular", json!([page.max(1), settings, false]))?
        } else {
            self.invoke("search", json!([query, page]))?
        };
        let mangas = mangas_from_search(&value)?
            .into_iter()
            .map(|manga| Manga::from(manga, self.id.clone()))
            .collect();
        Ok((mangas, false))
    }

    /// Implements `get_manga_details`: `parseNovel` + the chapter list.
    pub fn get_manga_details(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Manga> {
        let value = self.invoke("novel", json!([manga_id]))?;
        Ok(Manga::from(manga_from_novel(&value)?, self.id.clone()))
    }

    /// Implements `get_chapter_list`. Plugins with `parsePage` paginate their
    /// chapter list, so all pages are fetched here.
    pub fn get_chapter_list(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<crate::source::model::Chapter>> {
        let novel = self.invoke("novel", json!([manga_id]))?;
        let mut out = chapters(novel.get("chapters").unwrap_or(&Value::Null))?;

        let total_pages = novel.get("totalPages").and_then(Value::as_u64).unwrap_or(1);
        if self.props.has_parse_page && total_pages > 1 {
            for page in 2..=total_pages {
                let value = self.invoke("page", json!([manga_id, page]))?;
                out.extend(chapters(&value)?);
            }
        }

        // Resolve each chapter path to an absolute URL; the downloader uses it
        // as the base URL for the images inside the chapter HTML. This mirrors
        // the app's `chapterPathToUrl`: `resolveUrl` when the plugin defines
        // it, `site + path` otherwise.
        for chapter in out.iter_mut() {
            let url = if self.props.has_resolve_url {
                self.resolve_url(&chapter.key, false)
            } else {
                Url::parse(&format!("{}{}", self.props.site, chapter.key)).ok()
            };
            chapter.url = url.map(|u| u.to_string());
        }

        Ok(out
            .into_iter()
            .enumerate()
            .map(|(index, chapter)| {
                let mut chapter =
                    crate::source::model::Chapter::from(chapter, self.id.clone(), manga_id.clone());
                chapter.source_order = index;
                chapter
            })
            .collect())
    }

    /// Implements `get_page_list`: the chapter HTML becomes a single text page.
    pub fn get_page_list(
        &self,
        _cancellation_token: CancellationToken,
        _manga_id: String,
        chapter_id: String,
        _chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        let value = self.invoke("chapter", json!([chapter_id]))?;
        let html = value.as_str().unwrap_or_default();
        Ok(vec![page_from_chapter_html(0, html, chapter_id)])
    }

    /// Implements `get_image_request` using the plugin's `imageRequestInit`.
    pub fn get_image_request(
        &self,
        url: Url,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        let init = self.props.image_request_init.as_ref();
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
        let show_latest = listing.name.eq_ignore_ascii_case("latest");
        let settings = self.settings_json();
        let value = self.invoke("popular", json!([1, settings, show_latest]))?;
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
        let value = self.invoke("search", json!([query, page]))?;
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
        let mut manga = if needs_details {
            let value = self.invoke("novel", json!([manga.key]))?;
            manga_from_novel(&value)?
        } else {
            manga
        };
        if needs_chapters {
            let novel = self.invoke("novel", json!([manga.key]))?;
            manga.chapters = Some(chapters(novel.get("chapters").unwrap_or(&Value::Null))?);
        }
        Ok(manga)
    }

    pub fn get_page_list_next(
        &self,
        cancellation_token: CancellationToken,
        _manga: aidoku::Manga,
        chapter: aidoku::Chapter,
    ) -> Result<Vec<aidoku::Page>> {
        let _ = cancellation_token;
        let value = self.invoke("chapter", json!([chapter.key]))?;
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

fn collect_defaults(definition: &SettingDefinition, out: &mut HashMap<String, SourceSettingValue>) {
    match definition {
        SettingDefinition::Group { items, .. } => {
            for item in items {
                collect_defaults(item, out);
            }
        }
        SettingDefinition::Select {
            key,
            default,
            values,
            ..
        } => {
            out.insert(
                key.clone(),
                SourceSettingValue::String(
                    default
                        .clone()
                        .unwrap_or_else(|| values.first().cloned().unwrap_or_default()),
                ),
            );
        }
        SettingDefinition::MultiSelect { key, default, .. }
        | SettingDefinition::EditableList { key, default, .. } => {
            out.insert(key.clone(), SourceSettingValue::Vec(default.clone()));
        }
        SettingDefinition::Switch { key, default, .. } => {
            out.insert(key.clone(), SourceSettingValue::Bool(*default));
        }
        SettingDefinition::Text {
            key,
            default: Some(default),
            ..
        } => {
            out.insert(key.clone(), SourceSettingValue::String(default.clone()));
        }
        _ => {}
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
