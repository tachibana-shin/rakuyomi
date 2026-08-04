use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

use anyhow::{bail, Context, Result};

use crate::{
    model::SourceId,
    settings::{Settings, SourceSettingValue},
    source::Source,
    source_collection::SourceCollection,
};

#[derive(Clone)]
pub struct SourceManager {
    sources_folder: PathBuf,
    pub sources_by_id: HashMap<SourceId, Source>,
    pub settings: Settings,
    #[cfg(not(feature = "all"))]
    pub file_sources: HashMap<String, String>,
}

impl SourceManager {
    pub fn new(
        sources_folder: PathBuf,
        sources_by_id: HashMap<SourceId, Source>,
        settings: Settings,
    ) -> Self {
        Self {
            sources_folder,
            sources_by_id,
            settings,
            #[cfg(not(feature = "all"))]
            file_sources: HashMap::new(),
        }
    }

    pub fn from_folder(path: PathBuf, settings: Settings) -> Result<Self> {
        fs::create_dir_all(&path).context("while trying to ensure sources folder exists")?;

        Ok(Self {
            sources_folder: path,
            sources_by_id: HashMap::new(),
            settings,
            #[cfg(not(feature = "all"))]
            file_sources: HashMap::new(),
        })
    }

    pub fn install_source(
        &mut self,
        id: &SourceId,
        contents: impl AsRef<[u8]>,
        source_of_source: String,
        arc_manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<()> {
        let target_path = self.source_path(id);
        fs::write(&target_path, contents)?;

        Source::write_meta_file(&target_path, source_of_source)?;

        let source = Source::from_aix_file(&target_path, self, arc_manager)?;
        self.sources_by_id.insert(id.clone(), source);
        #[cfg(not(feature = "all"))]
        self.file_sources.insert(
            id.value().to_owned(),
            target_path.to_string_lossy().to_string(),
        );

        Ok(())
    }

    /// Installs an LNReader plugin: the raw JS is stored as `<id>.lnreader.js`.
    pub fn install_lnreader_source(
        &mut self,
        id: &SourceId,
        contents: impl AsRef<[u8]>,
        source_of_source: String,
    ) -> Result<()> {
        let target_path = self.lnreader_source_path(id);
        fs::write(&target_path, contents)?;

        Source::write_meta_file(&target_path, source_of_source)?;

        let source = Source::from_lnreader_file(&target_path, self)?;
        self.sources_by_id.insert(id.clone(), source);
        #[cfg(not(feature = "all"))]
        self.file_sources.insert(
            id.value().to_owned(),
            target_path.to_string_lossy().to_string(),
        );

        Ok(())
    }

    /// Installs a MangaYomi extension: the Dart code is stored as
    /// `<id>.mangayomi.dart` with its `index.json` entry as a
    /// `<id>.mangayomi.json` sidecar. Anime extensions (`itemType: 1`) are
    /// rejected.
    pub fn install_mangayomi_source(
        &mut self,
        id: &SourceId,
        code: impl AsRef<[u8]>,
        metadata: impl AsRef<[u8]>,
        source_of_source: String,
    ) -> Result<()> {
        let metadata: serde_json::Value =
            serde_json::from_slice(metadata.as_ref()).context("invalid extension metadata JSON")?;
        if metadata
            .get("itemType")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            == 1
        {
            bail!(
                "MangaYomi anime extension '{}' is not supported",
                id.value()
            );
        }
        let target_path = self.mangayomi_source_path(id);
        fs::write(&target_path, code)?;
        fs::write(target_path.with_extension("json"), metadata.to_string())?;

        Source::write_meta_file(&target_path, source_of_source)?;

        let source = Source::from_mangayomi_file(&target_path, self)?;
        self.sources_by_id.insert(id.clone(), source);
        #[cfg(not(feature = "all"))]
        self.file_sources.insert(
            id.value().to_owned(),
            target_path.to_string_lossy().to_string(),
        );

        Ok(())
    }

    pub fn uninstall_source(&mut self, id: &SourceId) -> Result<()> {
        let source_path = self.source_path(id);
        fs::remove_file(&source_path)?;

        self.sources_by_id.remove(&id.clone());
        #[cfg(not(feature = "all"))]
        self.file_sources.remove(id.value());

        Ok(())
    }

    /// Removes a WASM, an LNReader and a MangaYomi source file if present.
    pub fn uninstall_any_source(&mut self, id: &SourceId) -> Result<()> {
        for path in [
            self.source_path(id),
            self.lnreader_source_path(id),
            self.mangayomi_source_path(id),
        ] {
            if path.exists() {
                fs::remove_file(&path)?;
            }
            let meta_path = path.with_extension("json");
            if meta_path.exists() {
                fs::remove_file(&meta_path)?;
            }
        }
        self.sources_by_id.remove(id);
        #[cfg(not(feature = "all"))]
        self.file_sources.remove(id.value());
        Ok(())
    }

    pub fn update_settings(
        &mut self,
        settings: Settings,
        manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<()> {
        self.settings = settings;
        self.sources_by_id = self.load_all_sources(manager)?;

        Ok(())
    }

    pub fn update_source_setting(
        &mut self,
        source_id: String,
        snapshot: HashMap<String, SourceSettingValue>,
        arc_manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<()> {
        let mut settings = self.settings.clone();
        settings.source_settings.insert(source_id, snapshot);

        self.settings = settings;
        self.sources_by_id = self.load_all_sources(arc_manager)?;

        Ok(())
    }

    pub fn load_all_sources(
        &mut self,
        manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<HashMap<SourceId, Source>> {
        let files = fs::read_dir(&self.sources_folder).with_context(|| {
            format!(
                "while attempting to read source collection at {}",
                self.sources_folder.display()
            )
        })?;

        #[cfg(not(feature = "all"))]
        self.file_sources.clear();

        let mut sources_by_id = HashMap::new();
        for entry in files.flatten() {
            let path = entry.path();
            let is_lnreader = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
                && path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .ends_with(crate::source::lnreader::LNREADER_FILE_SUFFIX)
                });
            let is_mangayomi = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dart"))
                && path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .ends_with(crate::source::mangayomi::MANGA_YOMI_FILE_SUFFIX)
                });

            let source = if is_lnreader {
                Source::from_lnreader_file(&path, self)?
            } else if is_mangayomi {
                Source::from_mangayomi_file(&path, self)?
            } else {
                if !path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("aix"))
                {
                    continue;
                }
                Source::from_aix_file(&path, self, manager)?
            };
            #[cfg(not(feature = "all"))]
            self.file_sources.insert(
                source.manifest().info.id.clone(),
                path.as_path().to_string_lossy().to_string(),
            );

            sources_by_id.insert(SourceId::new(source.manifest().info.id.clone()), source);
        }

        Ok(sources_by_id)
    }

    pub fn source_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!("{}.aix", id.value()))
    }

    pub fn lnreader_source_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!(
            "{}{}",
            id.value(),
            crate::source::lnreader::LNREADER_FILE_SUFFIX
        ))
    }

    pub fn mangayomi_source_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!(
            "{}{}",
            id.value(),
            crate::source::mangayomi::MANGA_YOMI_FILE_SUFFIX
        ))
    }
}

impl SourceCollection for SourceManager {
    fn get_by_id(&self, id: &SourceId) -> Option<&Source> {
        self.sources_by_id.get(id)
    }

    fn sources(&self) -> Vec<&Source> {
        self.sources_by_id.values().collect()
    }
}
