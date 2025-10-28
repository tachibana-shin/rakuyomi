use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Method, Request};
use serde::Deserialize;
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;
use url::Url;
use wasmi::*;
use zip::ZipArchive;

use crate::settings::Settings;

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
mod source_settings;
mod wasm_imports;
mod wasm_store;

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

impl Source {
    pub fn from_aix_file(path: &Path, settings: Settings) -> Result<Self> {
        let blocking_source = BlockingSource::from_aix_file(path, settings)?;

        Ok(Self(Arc::new(Mutex::new(blocking_source))))
    }

    pub fn manifest(&self) -> SourceManifest {
        // FIXME we dont actually need to clone here but yeah it's easier
        self.0.lock().unwrap().manifest.clone()
    }

    pub fn setting_definitions(&self) -> Vec<SettingDefinition> {
        self.0.lock().unwrap().setting_definitions.clone()
    }

    wrap_blocking_source_fn!(
        get_manga_list,
        Result<Vec<Manga>>,
        cancellation_token: CancellationToken
    );

    wrap_blocking_source_fn!(
        search_mangas,
        Result<Vec<Manga>>,
        cancellation_token: CancellationToken,
        query: String
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
        chapter_num: Option<f64>
    );

    wrap_blocking_source_fn!(
        get_image_request,
        Result<Request>,
        url: Url
    );
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SourceInfo {
    pub id: String,
    pub lang: String,
    pub name: String,
    pub version: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceManifest {
    pub info: SourceInfo,
}

struct BlockingSource {
    store: Store<WasmStore>,
    instance: Instance,
    manifest: SourceManifest,
    setting_definitions: Vec<SettingDefinition>,
}

impl BlockingSource {
    pub fn from_aix_file(path: &Path, settings: Settings) -> Result<Self> {
        let file =
            fs::File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("couldn't open source archive {}", path.display()))?;

        let manifest_file = archive
            .by_name("Payload/source.json")
            .with_context(|| "while loading source.json")?;
        let manifest: SourceManifest = serde_json::from_reader(manifest_file)?;

        let setting_definitions: Vec<SettingDefinition> =
            if let Ok(file) = archive.by_name("Payload/settings.json") {
                serde_json::from_reader(file)?
            } else {
                Vec::new()
            };

        let stored_source_settings = settings
            .source_settings
            .get(&manifest.info.id)
            .cloned()
            .unwrap_or_default();

        let source_settings = SourceSettings::new(&setting_definitions, stored_source_settings)?;

        let wasm_file = archive
            .by_name("Payload/main.wasm")
            .with_context(|| "while loading main.wasm")?;

        let engine = Engine::default();
        let wasm_store = WasmStore::new(manifest.info.id.clone(), source_settings, settings);
        let mut store = Store::new(&engine, wasm_store);
        let module = Module::new_streaming(&engine, wasm_file)
            .with_context(|| format!("failed loading module from {}", path.display()))?;

        let mut linker = Linker::new(&engine);
        register_aidoku_imports(&mut linker)?;
        register_defaults_imports(&mut linker)?;
        register_env_imports(&mut linker)?;
        register_html_imports(&mut linker)?;
        register_json_imports(&mut linker)?;
        register_net_imports(&mut linker)?;
        register_std_imports(&mut linker)?;

        let instance = linker
            .instantiate(&mut store, &module)
            .with_context(|| {
                format!(
                    "failed creating instance when loading from {}",
                    path.display()
                )
            })?
            .start(&mut store)?;

        Ok(Self {
            store,
            instance,
            manifest,
            setting_definitions,
        })
    }

    pub fn get_manga_list(&mut self, cancellation_token: CancellationToken) -> Result<Vec<Manga>> {
        self.run_under_context(cancellation_token, OperationContextObject::None, |this| {
            this.search_mangas_by_filters_inner(vec![])
        })
    }

    pub fn search_mangas(
        &mut self,
        cancellation_token: CancellationToken,
        query: String,
    ) -> Result<Vec<Manga>> {
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

        // FIXME what if i actually want more pages tho
        let page = 1i32;
        let page_descriptor =
            wasm_function.call(&mut self.store, (filters_descriptor as i32, page))?;
        // TODO maybe use some `TryInto` implementation here to make things easier to read
        let mangas: Vec<Manga> = match self
            .store
            .data_mut()
            .get_std_value(page_descriptor as usize)
            .ok_or(anyhow!("could not read data from page descriptor"))?
            .as_ref()
        {
            Value::Object(ObjectValue::MangaPageResult(MangaPageResult { manga, .. })) => {
                manga.clone()
            }
            other => bail!(
                "expected page descriptor to be an array, found {:?} instead",
                other
            ),
        };

        // TODO remove page_descriptor and filters_descriptor from the source's storage

        Ok(mangas)
    }

    pub fn get_chapter_list(
        &mut self,
        cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<Chapter>> {
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
        let chapter_list_descriptor =
            wasm_function.call(&mut self.store, manga_descriptor as i32)?;

        let chapters: Vec<Chapter> = match self
            .store
            .data_mut()
            .get_std_value(chapter_list_descriptor as usize)
            .ok_or(anyhow!("could not read data from chapter list descriptor"))?
            .as_ref()
        {
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
        };

        Ok(chapters)
    }

    pub fn get_page_list(
        &mut self,
        cancellation_token: CancellationToken,
        manga_id: String,
        chapter_id: String,
        chapter_num: Option<f64>,
    ) -> Result<Vec<Page>> {
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
        chapter_num: Option<f64>,
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
            chapter_hashmap.insert("chapterNum".to_string(), Value::Float(chapter_num));
        }

        let chapter_descriptor = self.store.data_mut().store_std_value(
            Value::Object(ObjectValue::ValueMap(chapter_hashmap)).into(),
            None,
        );

        // FIXME what the fuck is chapter counter, aidoku sets it here
        let wasm_function = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, "get_page_list")?;
        let page_list_descriptor =
            wasm_function.call(&mut self.store, chapter_descriptor as i32)?;

        let pages: Vec<Page> = match self
            .store
            .data_mut()
            .get_std_value(page_list_descriptor as usize)
            .ok_or(anyhow!("could not read data from page list descriptor"))?
            .as_ref()
        {
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
        };

        Ok(pages)
    }

    pub fn get_image_request(&mut self, url: Url) -> Result<Request> {
        let request_descriptor = self.store.data_mut().create_request();

        // FIXME scoping here is so fucking scuffed
        {
            let request_state = self
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

        let request_state = self
            .store
            .data_mut()
            .get_mut_request(request_descriptor)
            .unwrap();

        let request_building_state = match request_state {
            RequestState::Building(building_state) => Some(building_state),
            _ => None,
        }
        .unwrap();

        (request_building_state as &RequestBuildingState).try_into()
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
