use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

use anyhow::{Context, Result};

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
        }
    }

    pub fn from_folder(path: PathBuf, settings: Settings) -> Result<Self> {
        fs::create_dir_all(&path).context("while trying to ensure sources folder exists")?;

        Ok(Self {
            sources_folder: path,
            sources_by_id: HashMap::new(),
            settings: settings,
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

        Ok(())
    }

    pub fn uninstall_source(&mut self, id: &SourceId) -> Result<()> {
        let source_path = self.source_path(id);
        fs::remove_file(&source_path)?;

        self.sources_by_id.remove(&id.clone());

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
        let path = &self.sources_folder.clone();

        let files = fs::read_dir(path).with_context(|| {
            format!(
                "while attempting to read source collection at {}",
                &path.display()
            )
        })?;

        let sources: Vec<Source> = files
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("aix"))
            })
            .map(|path| Source::from_aix_file(&path, self, manager))
            .collect::<Result<_>>()?;

        let sources_by_id = sources
            .into_iter()
            .map(|source| (SourceId::new(source.manifest().info.id.clone()), source))
            .collect();

        Ok(sources_by_id)
    }

    fn source_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!("{}.aix", id.value()))
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
