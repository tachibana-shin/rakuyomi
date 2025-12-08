use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    model::SourceId, settings::Settings, source::Source, source_collection::SourceCollection,
};

#[derive(Clone)]
pub struct SourceManager {
    sources_folder: PathBuf,
    sources_by_id: HashMap<SourceId, Source>,
    settings: Settings,
}

impl SourceManager {
    pub fn from_folder(path: PathBuf, settings: Settings) -> Result<Self> {
        fs::create_dir_all(&path).context("while trying to ensure sources folder exists")?;
        let sources_by_id =
            Self::load_all_sources(&path, &settings).context("couldn't load sources")?;

        Ok(Self {
            sources_folder: path,
            sources_by_id,
            settings,
        })
    }

    pub fn install_source(
        &mut self,
        id: &SourceId,
        contents: impl AsRef<[u8]>,
        source_of_source: String,
    ) -> Result<()> {
        let target_path = self.source_path(id);
        fs::write(&target_path, contents)?;

        Source::write_meta_file(&target_path, source_of_source)?;

        self.sources_by_id.insert(
            id.clone(),
            Source::from_aix_file(&target_path, self.settings.clone())?,
        );

        Ok(())
    }

    pub fn uninstall_source(&mut self, id: &SourceId) -> Result<()> {
        let source_path = self.source_path(id);
        fs::remove_file(&source_path)?;

        self.sources_by_id.remove(&id.clone());

        Ok(())
    }

    pub fn update_settings(&mut self, settings: Settings) -> Result<()> {
        self.sources_by_id = Self::load_all_sources(&self.sources_folder, &settings)?;
        self.settings = settings;

        Ok(())
    }

    fn load_all_sources(path: &Path, settings: &Settings) -> Result<HashMap<SourceId, Source>> {
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
            .map(|path| Source::from_aix_file(&path, settings.clone()))
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
