//! MangaYomi extension source backend.
//!
//! Loads a single `*.mangayomi.dart` / `*.mangayomi.js` extension (as
//! published by mangayomi-extensions), runs it inside the embedded d4rt_rs
//! Dart interpreter or QuickJS runtime and exposes it through the same
//! [`Source`] interface as WASM and LNReader sources.

pub mod bridge;
pub mod html;
pub mod js;
pub mod model;
pub mod runtime;
pub mod xpath;

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
        BlockingSource, SourceFeatures, SourceInfo, SourceManifest, SourceMeta,
    },
    source_manager::SourceManager,
    util::DEFAULT_USER_AGENT,
};

use self::{
    model::ExtensionMeta,
    runtime::{MangayomiRuntime, DEFAULT_INVOKE_TIMEOUT},
};

/// The suffix of installed Dart extension files (`<id>.mangayomi.dart`). The
/// `index.json` entry is stored next to it as `<id>.mangayomi.json`.
pub(crate) const MANGA_YOMI_FILE_SUFFIX: &str = ".mangayomi.dart";

/// The suffix of installed JavaScript extension files (`<id>.mangayomi.js`),
/// distinguished by the `sourceCodeLanguage` field of the index entry
/// (`0` Dart, `1` JavaScript).
pub(crate) const MANGA_YOMI_JS_FILE_SUFFIX: &str = ".mangayomi.js";

/// A MangaYomi provider runtime: either the d4rt_rs Dart interpreter
/// ([`MangayomiRuntime`]) or the QuickJS JavaScript runtime
/// ([`js::MangayomiJsRuntime`]). Method calls are synchronous from the
/// outside.
pub trait MangayomiProvider: Send + Sync {
    /// Calls a method of the provider instance with JSON args and returns
    /// the JSON value produced by the runtime.
    fn invoke(
        &self,
        method: &str,
        args: Vec<serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value>;
}

impl MangayomiProvider for MangayomiRuntime {
    fn invoke(
        &self,
        method: &str,
        args: Vec<serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        self.invoke(method, args)
    }
}

impl MangayomiProvider for js::MangayomiJsRuntime {
    fn invoke(
        &self,
        method: &str,
        args: Vec<serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        self.invoke(method, args)
    }
}

/// A single MangaYomi extension exposed through the RakuYomi source API.
///
/// All method calls block on the underlying Dart runtime; the async wrappers
/// used by the rest of the codebase run them inside `spawn_blocking`.
pub struct MangayomiSource {
    pub id: String,
    pub manifest: SourceManifest,
    pub setting_definitions: Vec<SettingDefinition>,
    pub features: SourceFeatures,
    pub base_url: String,
    pub name: String,
    pub lang: String,
    pub supports_latest: bool,
    /// Extension kind from the index: `0` manga, `2` light novel (`1` anime
    /// is rejected at install). `2` switches page listing to `getHtmlContent`.
    pub item_type: u8,
    /// Merged source settings (stored values overlaid on the extension
    /// preference defaults), visible to the extension through
    /// `getPreferenceValue`.
    pub settings: Arc<Mutex<HashMap<String, SourceSettingValue>>>,
    runtime: Arc<dyn MangayomiProvider>,
}

impl std::fmt::Debug for MangayomiSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MangayomiSource")
            .field("id", &self.id)
            .field("manifest", &self.manifest)
            .field("setting_definitions", &self.setting_definitions)
            .field("base_url", &self.base_url)
            .field("name", &self.name)
            .field("lang", &self.lang)
            .field("supports_latest", &self.supports_latest)
            .finish()
    }
}

impl MangayomiSource {
    /// Loads an extension from a `<id>.mangayomi.dart` or
    /// `<id>.mangayomi.js` file with its `<id>.mangayomi.json` sidecar (the
    /// index.json entry) and prepares the runtime. The language is taken
    /// from the file suffix, falling back to the `sourceCodeLanguage`
    /// metadata for misnamed installs.
    pub fn from_mangayomi_file(path: &Path, manager: &SourceManager) -> Result<Self> {
        let code = fs::read_to_string(path)
            .with_context(|| format!("failed to read extension file {}", path.display()))?;
        let meta_path = path.with_extension("json");
        let mut metadata: Value = serde_json::from_str(
            &fs::read_to_string(&meta_path)
                .with_context(|| format!("failed to read extension metadata {meta_path:?}"))?,
        )
        .with_context(|| format!("failed to parse extension metadata {meta_path:?}"))?;
        // Older installs may lack the `id` key (it used to be swallowed by
        // `#[serde(flatten)]` in the install pipeline). Extensions are always
        // stored as `<id>.mangayomi.<lang>`, so recover it from the file
        // name: stem `<id>.mangayomi` -> strip the `.mangayomi` suffix.
        if ExtensionMeta::from_value(&metadata).id.is_empty() {
            let fallback_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .trim_end_matches(".mangayomi")
                .to_string();
            if fallback_id.is_empty() {
                bail!("extension metadata is missing an id for {}", path.display());
            }
            if let Value::Object(map) = &mut metadata {
                map.insert("id".to_string(), Value::String(fallback_id));
            }
        }
        let meta = ExtensionMeta::from_value(&metadata);
        if meta.id.is_empty() {
            bail!("extension metadata is missing an id");
        }
        // The source list key (e.g. `m2k3a/mangayomi-extensions`) is stored
        // in the sidecar meta file at install time; `/available-sources`
        // reports the same value as `source_of_source`, so it is what the
        // frontend matches installed sources against. Fall back to the
        // extension's `sourceCodeUrl` (often absent from the stored
        // metadata) when no meta file exists.
        let source_of_source = {
            let meta_file = BlockingSource::meta_source_path(path)?;
            let mut source_of_source = meta.source_code_url.clone();
            if meta_file.exists() {
                let meta: SourceMeta = serde_json::from_str(
                    &fs::read_to_string(&meta_file)
                        .with_context(|| format!("failed to read meta file {:?}", meta_file))?,
                )?;
                if let Some(from) = meta.source_of_source {
                    source_of_source = Some(from);
                }
            }
            source_of_source
        };

        // Dispatch by file suffix first (it is the ground truth for the
        // stored code); the `sourceCodeLanguage` metadata covers misnamed
        // installs (e.g. a JS extension stored with the Dart suffix).
        let is_js = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(MANGA_YOMI_JS_FILE_SUFFIX))
            || meta.source_code_language == 1;

        let stored_settings = manager
            .settings
            .source_settings
            .get(&meta.id)
            .cloned()
            .unwrap_or_default();

        let prefs = Arc::new(Mutex::new(HashMap::new()));
        let runtime: Arc<dyn MangayomiProvider> = if is_js {
            Arc::new(js::MangayomiJsRuntime::new(
                code,
                metadata,
                prefs.clone(),
                DEFAULT_INVOKE_TIMEOUT,
            )?)
        } else {
            Arc::new(MangayomiRuntime::new(
                code,
                metadata,
                prefs.clone(),
                DEFAULT_INVOKE_TIMEOUT,
            )?)
        };

        // Extension-declared preferences become the source's settings
        // definitions; stored values are merged on top of their defaults.
        let setting_definitions = setting_definitions(&*runtime)?;
        let mut settings: HashMap<String, SourceSettingValue> = HashMap::new();
        for definition in &setting_definitions {
            collect_defaults(definition, &mut settings);
        }
        for (key, value) in stored_settings {
            settings.insert(key, value);
        }
        *prefs.lock().unwrap() = settings.clone();

        // JavaScript extensions expose the base URL through the `MSource`
        // JSON only, so fall back to the metadata for them.
        let base_url = if is_js {
            meta.base_url.clone()
        } else {
            runtime
                .invoke("baseUrl", vec![])
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| meta.base_url.clone())
        };
        let supports_latest = runtime
            .invoke("supportsLatest", vec![])
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let manifest = SourceManifest {
            info: SourceInfo {
                id: meta.id.clone(),
                lang: Some(meta.lang.clone()),
                languages: None,
                #[cfg(not(feature = "all"))]
                content_rating: None,
                name: meta.name.clone(),
                version: Value::String(meta.version.clone()),
                url: Some(base_url.clone()),
                urls: None,
                min_app_version: None,
            },
            config: None,
            source_of_source,
        };

        Ok(Self {
            id: meta.id.clone(),
            manifest,
            setting_definitions,
            features: SourceFeatures {
                process_page_image: false,
            },
            base_url,
            name: meta.name,
            lang: meta.lang,
            supports_latest,
            item_type: meta.item_type,
            settings: prefs,
            runtime,
        })
    }

    /// Invokes an extension method with JSON args and parses the response.
    pub fn invoke(&self, method: &str, args: Value) -> Result<Value> {
        let args: Vec<Value> = args
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("mangayomi args must be a JSON array"))?;
        self.runtime
            .invoke(method, args)
            .with_context(|| format!("extension method `{}` failed", method))
    }

    /// The per-extension HTTP headers, from the `headers` getter.
    fn headers(&self) -> HashMap<String, String> {
        self.invoke("headers", json!([]))
            .ok()
            .and_then(|v| v.as_object().cloned())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Converts an `MPages` result (already serialised as
    /// `{"list": [...], "hasNextPage": bool}`) into rakuyomi mangas.
    fn mangas_from_page(&self, value: &Value) -> Vec<Manga> {
        value
            .get("list")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|manga| model::manga_from_value(&self.id, &self.base_url, manga))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Converts rakuyomi mangas to the aidoku representation used by the
    /// next-SDK shaped results.
    fn aidoku_mangas(&self, mangas: Vec<Manga>) -> Vec<aidoku::Manga> {
        mangas
            .into_iter()
            .map(|manga| aidoku::Manga {
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
            })
            .collect()
    }

    /// Implements `get_manga_list`: `getPopular` (or `getLatestUpdates` for
    /// the "latest" listing), page 1.
    pub fn get_manga_list(
        &self,
        _cancellation_token: CancellationToken,
        listing: aidoku::Listing,
    ) -> Result<Vec<Manga>> {
        let value = if listing.name.eq_ignore_ascii_case("latest") && self.supports_latest {
            self.invoke("getLatestUpdates", json!([1]))?
        } else {
            self.invoke("getPopular", json!([1]))?
        };
        Ok(self.mangas_from_page(&value))
    }

    /// Implements `search_mangas`. An empty query means "browse", which the
    /// extensions expose through `getPopular`; otherwise `search` is used.
    pub fn search_mangas(
        &self,
        _cancellation_token: CancellationToken,
        query: String,
        page: i32,
    ) -> Result<(Vec<Manga>, bool)> {
        let value = if query.trim().is_empty() {
            self.invoke("getPopular", json!([page.max(1)]))?
        } else {
            self.invoke("search", json!([query, page.max(1), []]))?
        };
        let has_next_page = value
            .get("hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok((self.mangas_from_page(&value), has_next_page))
    }

    /// Implements `get_manga_details`: `getDetail(mangaUrl)`.
    pub fn get_manga_details(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Manga> {
        let value = self.invoke("getDetail", json!([manga_id]))?;
        Ok(model::manga_from_value(&self.id, &self.base_url, &value))
    }

    /// Implements `get_chapter_list` from the `chapters` field of
    /// `getDetail`.
    pub fn get_chapter_list(
        &self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<crate::source::model::Chapter>> {
        let value = self.invoke("getDetail", json!([manga_id.clone()]))?;
        let chapters = value.get("chapters").unwrap_or(&Value::Null);
        let out = model::chapters_from_value(&self.id, &manga_id, &self.base_url, chapters);
        Ok(out
            .into_iter()
            .enumerate()
            .map(|(index, mut chapter)| {
                chapter.source_order = index;
                chapter
            })
            .collect())
    }

    /// Implements `get_page_list`:
    /// - manga (`item_type != 2`): `getPageList(chapterUrl)`.
    /// - light novel (`item_type == 2`): `getHtmlContent(name, chapterUrl)`
    ///   produces the chapter HTML, exposed as a single text page so the
    ///   downloader renders it like LNReader chapter HTML.
    pub fn get_page_list(
        &self,
        _cancellation_token: CancellationToken,
        _manga_id: String,
        chapter_id: String,
        _chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        if self.item_type == 2 {
            let html = self.invoke(
                "getHtmlContent",
                json!([self.name.clone(), chapter_id.clone()]),
            )?;
            let html = html
                .as_str()
                .ok_or_else(|| anyhow!("getHtmlContent returned a non-string value"))?;
            return Ok(vec![model::page_from_chapter_html(
                &self.id,
                &chapter_id,
                0,
                html,
            )]);
        }
        let value = self.invoke("getPageList", json!([chapter_id.clone()]))?;
        Ok(model::pages_from_value(
            &self.id,
            &chapter_id,
            &self.base_url,
            &value,
        ))
    }

    /// Implements `get_image_request` using the extension's `headers` getter.
    pub fn get_image_request(
        &self,
        url: Url,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        let headers = self.headers();
        let mut builder = crate::tls::client_builder()
            .build()
            .context("failed to build HTTP client")?
            .request(Method::GET, url.clone());
        let mut header_map = HeaderMap::new();
        for (key, value) in &headers {
            if let (Ok(name), Ok(v)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                header_map.insert(name, v);
            }
        }
        if !header_map.contains_key(reqwest::header::USER_AGENT) {
            header_map.insert(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static(DEFAULT_USER_AGENT),
            );
        }
        builder = builder.headers(header_map);
        let request = builder
            .build()
            .with_context(|| format!("failed to build image request for {}", url))?;
        Ok(request)
    }

    /// MangaYomi extensions have no image processing step.
    pub fn process_page_image(
        &self,
        _cancellation_token: CancellationToken,
        _request: (Url, HeaderMap),
        _response: (reqwest::StatusCode, HeaderMap),
        _bytes: tokio_util::bytes::Bytes,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Vec<u8>> {
        bail!("process_page_image is not supported by MangaYomi extensions")
    }

    /// Notifications are not supported by MangaYomi extensions.
    pub fn handle_notification_next(
        &self,
        _cancellation_token: CancellationToken,
        _key: String,
    ) -> Result<()> {
        Ok(())
    }

    // next-SDK shaped helpers, uniform with the other backends.

    pub fn get_manga_list_next(
        &self,
        cancellation_token: CancellationToken,
        listing: aidoku::Listing,
        _page: i32,
    ) -> Result<crate::source::NextMangaPageResult> {
        let _ = cancellation_token;
        let value = if listing.name.eq_ignore_ascii_case("latest") && self.supports_latest {
            self.invoke("getLatestUpdates", json!([1]))?
        } else {
            self.invoke("getPopular", json!([1]))?
        };
        let has_next_page = value
            .get("hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(crate::source::NextMangaPageResult {
            entries: self.aidoku_mangas(self.mangas_from_page(&value)),
            has_next_page,
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
        let value = if query.trim().is_empty() {
            self.invoke("getPopular", json!([page.max(1)]))?
        } else {
            self.invoke("search", json!([query, page.max(1), []]))?
        };
        let has_next_page = value
            .get("hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(crate::source::NextMangaPageResult {
            entries: self.aidoku_mangas(self.mangas_from_page(&value)),
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
        let _ = cancellation_token;
        if needs_details {
            let value = self.invoke("getDetail", json!([manga.key.clone()]))?;
            let updated = model::manga_from_value(&self.id, &self.base_url, &value);
            Ok(aidoku::Manga {
                key: updated.id.clone(),
                title: updated.title.unwrap_or_default(),
                cover: updated.cover_url.map(|u| u.to_string()),
                artists: updated.artist.map(|a| vec![a]),
                authors: updated.author.map(|a| vec![a]),
                description: updated.description,
                url: updated.url.map(|u| u.to_string()),
                tags: updated.tags,
                status: aidoku::MangaStatus::Unknown,
                content_rating: aidoku::ContentRating::Unknown,
                viewer: aidoku::Viewer::Unknown,
                update_strategy: aidoku::UpdateStrategy::Never,
                next_update_time: None,
                chapters: None,
            })
        } else {
            Ok(manga)
        }
    }

    pub fn get_page_list_next(
        &self,
        cancellation_token: CancellationToken,
        _manga: aidoku::Manga,
        chapter: aidoku::Chapter,
    ) -> Result<Vec<aidoku::Page>> {
        let _ = cancellation_token;
        let value = self.invoke("getPageList", json!([chapter.key.clone()]))?;
        Ok(value
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|page| {
                        let raw = match page {
                            Value::String(s) => s.clone(),
                            _ => page
                                .get("url")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        };
                        aidoku::Page {
                            content: aidoku::PageContent::Url(raw, None),
                            thumbnail: None,
                            has_description: false,
                            description: None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn get_image_request_next(
        &self,
        url: Url,
        ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        self.get_image_request(url, ctx)
    }
}

/// Builds the setting definitions from the extension's
/// `getSourcePreferences` result. Failures are tolerated: the extension may
/// not declare any preferences (the base `MProvider` throws for it).
fn setting_definitions(runtime: &dyn MangayomiProvider) -> Result<Vec<SettingDefinition>> {
    let value = runtime
        .invoke("getSourcePreferences", vec![])
        .unwrap_or_default();
    let mut out = Vec::new();
    let Some(list) = value.as_array() else {
        return Ok(out);
    };
    for item in list {
        let Some(key) = item.get("key").and_then(Value::as_str) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        if let Some(pref) = item
            .get("checkBoxPreference")
            .or_else(|| item.get("switchPreferenceCompat"))
        {
            out.push(SettingDefinition::Switch {
                title: pref
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(key)
                    .to_string(),
                key: key.to_string(),
                default: pref.get("value").and_then(Value::as_bool).unwrap_or(false),
            });
        } else if let Some(value) = item.get("value") {
            // Flat preference form: the extension constructs the class
            // directly (`EditTextPreference(key:..., value:...)`,
            // `CheckBoxPreference(...)`), which the bridge stores as a flat
            // map instead of a `SourcePreference`-style nested map.
            if let Some(default) = value.as_str() {
                out.push(SettingDefinition::Text {
                    placeholder: None,
                    key: key.to_string(),
                    default: Some(default.to_string()),
                });
            } else if let Some(default) = value.as_bool() {
                out.push(SettingDefinition::Switch {
                    title: item
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or(key)
                        .to_string(),
                    key: key.to_string(),
                    default,
                });
            }
        } else if let Some(pref) = item.get("listPreference") {
            let entries: Vec<String> = pref
                .get("entries")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let entry_values: Vec<String> = pref
                .get("entryValues")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let index = pref.get("valueIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
            let values = if entry_values.is_empty() {
                entries.clone()
            } else {
                entry_values
            };
            let default = values
                .get(index)
                .cloned()
                .or_else(|| values.first().cloned());
            if !values.is_empty() {
                out.push(SettingDefinition::Select {
                    title: pref
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or(key)
                        .to_string(),
                    key: key.to_string(),
                    values,
                    titles: if entries.is_empty() {
                        None
                    } else {
                        Some(entries)
                    },
                    default,
                });
            }
        } else if let Some(pref) = item.get("multiSelectListPreference") {
            let entries: Vec<String> = pref
                .get("entries")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let entry_values: Vec<String> = pref
                .get("entryValues")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let default: Vec<String> = pref
                .get("values")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            if !entry_values.is_empty() {
                out.push(SettingDefinition::MultiSelect {
                    title: pref
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or(key)
                        .to_string(),
                    key: key.to_string(),
                    values: entry_values,
                    titles: if entries.is_empty() {
                        None
                    } else {
                        Some(entries)
                    },
                    default,
                });
            }
        } else if let Some(pref) = item.get("editTextPreference") {
            out.push(SettingDefinition::Text {
                placeholder: None,
                key: key.to_string(),
                default: pref
                    .get("value")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string()),
            });
        }
    }
    Ok(out)
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
