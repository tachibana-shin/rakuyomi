use aidoku::FilterValue;
use anyhow::{anyhow, bail, Context, Result};
use image::{codecs::jpeg::JpegEncoder, ColorType, ImageEncoder};
use reqwest::{header::HeaderMap, Method, Request, StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio_util::bytes::Bytes;
use tokio_util::sync::CancellationToken;
use url::Url;
use wasmi::*;
use zip::ZipArchive;

use crate::{
    settings::SourceSettingValue,
    source::{
        next_reader::read_next,
        wasm_imports::next as sdk_next,
        wasm_store::{ImageRef, ImageRequest, ImageResponse},
    },
    source_manager::SourceManager,
};

use self::{
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

pub mod model;
mod next_reader;
mod source_settings;
mod wasm_imports;
mod wasm_store;

/**
 * params need mark encode
 * handle_notification
 * handle_deep_link
 * handle_basic_login
 * handle_web_login
 * handle_key_migration
 *
 */

#[derive(Clone)]
pub struct Source(
    /// In order to avoid issues when calling functions that block inside the `Source` from an
    /// async context, we wrap all data and functions that need to block inside `BlockingSource`
    /// and call them using `spawn_blocking` from within the facades exposed by `Source`.
    /// Particularly, all calls to `reqwest::blocking` methods from an async context causes the
    /// program to panic (see https://github.com/seanmonstar/reqwest/issues/1017), and we do call
    /// them inside the `net` module.
    ///
    /// This also provides interior mutability, but we probably could also do it inside the
    /// `BlockingSource` itself, by placing things inside a mutex. It might be a cleaner design.
    Arc<Mutex<BlockingSource>>,
);

macro_rules! wrap_blocking_source_fn {
    ($fn_name:ident, $return_type:ty, $($param:ident : $type:ty),*) => {
        pub async fn $fn_name(&self, $($param: $type),*) -> $return_type {
            let blocking_source = self.0.clone();

            ::tokio::task::spawn_blocking(move || blocking_source.lock().unwrap().$fn_name($($param),*)).await?
        }
    };
}

macro_rules! call_cleanup {
    (
        blocking = $blocking:expr,
        func = $func:expr,
        args = ($($args:expr),*),
        free = [$($descriptor:expr),*],
        as $result_ty:ty,
        parse = $parse_fn:expr
    ) => {{
        let result_descriptor = {$func.call(&mut $blocking.store, ($($args),*))
            .expect("wasm call failed")};

        let parsed: Result<$result_ty> = {
            let store: &mut Store<WasmStore> = &mut $blocking.store;
            $parse_fn(result_descriptor, store, $blocking.instance)
        };

        {
            let store_mut = $blocking.store.data_mut();
            $(store_mut.take_std_value($descriptor as usize);)*
            let _ = $blocking.free_result(result_descriptor);
        }

        parsed
    }};
}

impl Source {
    pub fn from_aix_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Self> {
        let mut blocking_source = BlockingSource::from_aix_file(path, manager, arc_manager, None)?;

        if blocking_source.next_sdk {
            blocking_source.start()?;
        }

        Ok(Self(Arc::new(Mutex::new(blocking_source))))
    }

    pub fn manifest(&self) -> SourceManifest {
        // FIXME we dont actually need to clone here but yeah it's easier
        self.0.lock().unwrap().manifest.clone()
    }

    pub fn setting_definitions(&self) -> Vec<SettingDefinition> {
        self.0.lock().unwrap().setting_definitions.clone()
    }

    pub fn write_meta_file(path: &Path, source_of_source: String) -> anyhow::Result<()> {
        fs::write(
            BlockingSource::meta_source_path(path)?,
            serde_json::to_string(&SourceMeta {
                source_of_source: Some(source_of_source),
                is_next_sdk: None,
            })?,
        )
        .context("while writing meta file")
    }

    wrap_blocking_source_fn!(
        get_manga_list,
        Result<Vec<Manga>>,
        cancellation_token: CancellationToken,
        listing: String
    );

    wrap_blocking_source_fn!(
        search_mangas,
        Result<Vec<Manga>>,
        cancellation_token: CancellationToken,
        query: String
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
    pub name: String,
    pub version: usize,
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

struct BlockingSource {
    id: String,
    store: Store<WasmStore>,
    instance: Instance,
    manifest: SourceManifest,
    setting_definitions: Vec<SettingDefinition>,
    pub next_sdk: bool,
    features: SourceFeatures,
}

impl BlockingSource {
    pub fn from_aix_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
        force_mode: Option<bool>,
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

        let aidoku_sdk_next = force_mode.unwrap_or_else(|| {
            aidoku_sdk_next_from_meta
                .unwrap_or_else(|| Self::is_aidoku_sdk_next(&manifest.info.min_app_version))
        });

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

        let mut wasm_bytes = Vec::new();
        archive
            .by_name("Payload/main.wasm")
            .with_context(|| "while loading main.wasm")?
            .read_to_end(&mut wasm_bytes)
            .with_context(|| format!("failed reading wasm from zip entry {}", path.display()))?;

        let engine = Engine::default();
        let wasm_store = WasmStore::new(
            manifest.info.id.clone(),
            source_settings,
            manager.settings.clone(),
        );
        let mut store = Store::new(&engine, wasm_store);

        let module = Module::new(&engine, &wasm_bytes)
            .with_context(|| format!("failed loading module from {}", path.display()))?;

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

        let instance = match linker
            .instantiate_and_start(&mut store, &module)
            .with_context(|| format!("failed creating instance from {}", path.display()))
        {
            Ok(instance) => instance,
            Err(error) => {
                if force_mode.is_none() {
                    println!(
                        "Info: failed instantiating {id} retry mode {}",
                        if aidoku_sdk_next { "legacy" } else { "next" }
                    );

                    return Self::from_aix_file(path, manager, arc_manager, Some(!aidoku_sdk_next));
                }

                eprintln!("Error instantiating: {:?}", error);
                return Err(error);
            }
        };

        let features = SourceFeatures {
            process_page_image: instance
                .get_typed_func::<(i32, i32), i32>(&mut store, "process_page_image")
                .map(|_| true)
                .ok()
                .unwrap_or(false),
        };

        if aidoku_sdk_next_from_meta.is_none()
            || aidoku_sdk_next_from_meta.unwrap() != aidoku_sdk_next
        {
            let meta_file = Self::meta_source_path(path)?;

            let _ = fs::write(
                &meta_file,
                serde_json::to_string(&SourceMeta {
                    source_of_source: manifest.source_of_source.clone(),
                    is_next_sdk: Some(aidoku_sdk_next),
                })?,
            );
        }

        Ok(Self {
            id,
            store,
            instance,
            manifest,
            next_sdk: aidoku_sdk_next,
            setting_definitions,
            features,
        })
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
        let target = Version::parse("0.7.0").unwrap();

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
        listing: String,
    ) -> Result<Vec<Manga>> {
        if self.next_sdk {
            return self
                .get_manga_list_next(cancellation_token, listing, 1)
                .map(|list| {
                    list.into_iter()
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
    ) -> Result<Vec<Manga>> {
        if self.next_sdk {
            return self
                .get_search_manga_list_next(cancellation_token, query, 1, [].to_vec())
                .map(|list| {
                    list.into_iter()
                        .map(|v| Manga::from(v, self.id.clone()))
                        .collect::<Vec<_>>()
                });
        }
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.search_mangas_by_filters_inner(vec![Filter::Title(query)])
        })
    }

    fn search_mangas_by_filters_inner(&mut self, filters: Vec<Filter>) -> Result<Vec<Manga>> {
        let wasm_function = self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "get_manga_list")?;
        let filters_descriptor = self.store.data_mut().store_std_value(
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

        let manga_descriptor = self.store.data_mut().store_std_value(
            Value::Object(ObjectValue::ValueMap(manga_hashmap)).into(),
            None,
        );

        let wasm_function = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, "get_manga_details")?;

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

        let manga_descriptor = self.store.data_mut().store_std_value(
            Value::Object(ObjectValue::ValueMap(manga_hashmap)).into(),
            None,
        );

        // FIXME what the fuck is chapter counter, aidoku sets it here
        let wasm_function = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, "get_chapter_list")?;

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

        let chapter_descriptor = self.store.data_mut().store_std_value(
            Value::Object(ObjectValue::ValueMap(chapter_hashmap)).into(),
            None,
        );

        // FIXME what the fuck is chapter counter, aidoku sets it here
        let wasm_function = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, "get_page_list")?;

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
        if self.next_sdk {
            self.get_image_request_next(url, ctx)
        } else {
            self.get_image_request_inner(url)
        }
    }
    pub fn get_image_request_inner(&mut self, url: Url) -> Result<Request> {
        let request_descriptor = self.store.data_mut().create_request();

        // FIXME scoping here is so fucking scuffed
        {
            let request_state = &mut self
                .store
                .data_mut()
                .get_mut_request(request_descriptor)
                .unwrap();

            let request_building_state = match request_state {
                RequestState::Building(building_state) => Some(building_state),
                _ => None,
            }
            .unwrap();

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
            let mut wasm_store = &mut self.store;

            if let Ok(wasm_function) = self
                .instance
                .get_typed_func::<i32, ()>(&mut wasm_store, "modify_image_request")
            {
                wasm_function.call(&mut wasm_store, request_descriptor as i32)?;
            }
        }

        let request_state = &mut self
            .store
            .data_mut()
            .remove_request(request_descriptor)
            .unwrap();

        let request_building_state = match request_state {
            RequestState::Building(building_state) => Some(building_state),
            _ => None,
        }
        .unwrap();

        (request_building_state as &RequestBuildingState).try_into()
    }

    // next sdk

    pub fn start(&mut self) -> Result<()> {
        let wasm_function = self
            .instance
            .get_typed_func::<(), ()>(&mut self.store, "start")?;

        wasm_function.call(&mut self.store, ())?;

        Ok(())
    }
    pub fn free_result(&mut self, pointer: i32) -> Result<()> {
        let wasm_function = self
            .instance
            .get_typed_func::<i32, ()>(&mut self.store, "free_memory")?;

        wasm_function.call(&mut self.store, pointer)?;

        Ok(())
    }

    pub fn get_search_manga_list_next(
        &mut self,
        cancellation_token: CancellationToken,
        query: String,
        page: i32,
        _filters: Vec<FilterValue>,
    ) -> Result<Vec<aidoku::Manga>> {
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.get_search_manga_list_next_inner(query, page, vec![])
        })
    }

    fn get_memory(&self) -> Result<Memory> {
        match self.instance.get_export(&self.store, "memory") {
            Some(Extern::Memory(memory)) => Ok(memory),
            _ => bail!("failed to get memory"),
        }
    }

    fn get_search_manga_list_next_inner(
        &mut self,
        keyword: String,
        page: i32,
        filters: Vec<Filter>,
    ) -> Result<Vec<aidoku::Manga>> {
        if !filters.is_empty() {
            eprintln!("The current version not support filters");
        }

        let wasm_function = self
            .instance
            .get_typed_func::<(i32, i32, i32), i32>(&mut self.store, "get_search_manga_list")?;

        let store = self.store.data_mut();

        let keyword = store.store_std_value(Value::from(keyword).into(), None);
        let filters = store.store_std_value(Value::NextFilters([].to_vec()).into(), None);

        let mangas = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (keyword as i32, page, filters as i32),
        free = [keyword, filters],
        as  Vec<aidoku::Manga>,
        parse = |pointer, store: &mut Store<WasmStore>, instance| {
            let memory = get_memory(instance, store)?;
            let mangas = read_next::<NextMangaPageResult>(&memory, &store, pointer)?;

            Ok(mangas.entries)
        })?;

        Ok(mangas)
    }

    pub fn get_manga_update_next(
        &mut self,
        cancellation_token: CancellationToken,
        manga: aidoku::Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<aidoku::Manga> {
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
        let store = self.store.data_mut();

        let manga = store.store_std_value(Value::NextManga(manga).into(), None);

        let wasm_function = self
            .instance
            .get_typed_func::<(i32, i32, i32), i32>(&mut self.store, "get_manga_update")?;

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
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.get_page_list_next_inner(manga, chapter)
        })
    }

    fn get_page_list_next_inner(
        &mut self,
        manga: aidoku::Manga,
        chapter: aidoku::Chapter,
    ) -> Result<Vec<aidoku::Page>> {
        let store = self.store.data_mut();

        let manga = store.store_std_value(Value::NextManga(manga).into(), None);
        let chapter = store.store_std_value(Value::NextChapter(chapter).into(), None);

        let wasm_function = self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "get_page_list")?;

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
            let store = self.store.data_mut();

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
            let wasm_function = self
                .instance
                .get_typed_func::<(i32, i32), i32>(&mut self.store, "get_image_request");

            match wasm_function {
                Ok(func) => Some(func.call(&mut self.store, (url_key, context_key))?),
                Err(_) => None,
            }
        };
        // Drop std_value entries now
        {
            let store = self.store.data_mut();
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
            let req_id = read_next::<i32>(&memory, &self.store, request_state_ptr)?;
            let _ = self.free_result(request_state_ptr);

            let store = self.store.data_mut();

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
        if !self.features.process_page_image {
            return Ok(bytes.to_vec());
        }

        let (image_id, image_ref, context_id) = {
            let store = self.store.data_mut();

            let image_ref = store
                .create_image(&bytes)
                .context("failed create image for process_page_image")?;

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

        let wasm_function = self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "process_page_image")?;

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

                // RGBA に変換（元は ARGB）
                let mut rgb_pixels: Vec<u8> = Vec::with_capacity((width * height * 3) as usize);

                for px in &pixels {
                    let _a = ((px >> 24) & 0xFF) as u8;
                    let r = ((px >> 16) & 0xFF) as u8;
                    let g = ((px >> 8) & 0xFF) as u8;
                    let b = (px & 0xFF) as u8;

                    // JPEG は alpha に対応しないため RGB のみ書き込む
                    rgb_pixels.extend_from_slice(&[r, g, b]);
                }

                let mut out = Vec::<u8>::new();

                // JPEG エンコーダ（Seek 不要）
                let encoder = JpegEncoder::new_with_quality(&mut out, 100);

                // RGB24 としてエンコード
                encoder
                    .write_image(&rgb_pixels, width, height, ColorType::Rgb8.into())
                    .context("JPEG encode failed")?;

                out
            };

            Ok(image_data)
        })?;

        Ok(image_data)
    }

    pub fn get_manga_list_next(
        &mut self,
        cancellation_token: CancellationToken,
        listing: String,
        page: i32,
    ) -> Result<Vec<aidoku::Manga>> {
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.get_manga_list_next_inner(listing, page)
        })
    }

    fn get_manga_list_next_inner(
        &mut self,
        listing: String,
        page: i32,
    ) -> Result<Vec<aidoku::Manga>> {
        let wasm_function = self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "get_manga_list")?;

        let store = self.store.data_mut();

        let listing = store.store_std_value(Value::from(listing).into(), None);

        let mangas = call_cleanup!(
        blocking = self,
        func = wasm_function,
        args = (listing as i32, page),
        free = [listing],
        as  Vec<aidoku::Manga>,
        parse = |pointer, store: &mut Store<WasmStore>, instance| {
            let memory = get_memory(instance, store)?;
            let mangas = read_next::<NextMangaPageResult>(&memory, &store, pointer)?;

            Ok(mangas.entries)
        })?;

        Ok(mangas)
    }

    pub fn handle_notification_next(
        &mut self,
        cancellation_token: CancellationToken,
        key: String,
    ) -> Result<()> {
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.handle_notification_next_inner(key)
        })
    }

    fn handle_notification_next_inner(&mut self, key: String) -> Result<()> {
        let wasm_function = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, "handle_notification")?;

        let store = self.store.data_mut();

        let key = store.store_std_value(Value::from(key).into(), None);

        wasm_function.call(&mut self.store, key as i32)?;
        let _ = &self.store.data_mut().take_std_value(key);

        Ok(())
    }

    fn run_under_context<T, F>(
        &mut self,
        cancellation_token: CancellationToken,
        current_object: OperationContextObject,
        f: F,
    ) -> T
    where
        F: FnOnce(&mut Self) -> T,
    {
        self.store.data_mut().context = OperationContext {
            cancellation_token,
            current_object,
        };

        let result = f(self);

        self.store.data_mut().context = OperationContext::default();

        result
    }
}
