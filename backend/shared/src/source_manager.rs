use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

use anyhow::{Context, Result};

use crate::{
    model::SourceId,
    settings::{Settings, SourceSettingValue},
    source::{self, Source},
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

    pub fn uninstall_source(&mut self, id: &SourceId) -> Result<()> {
        let source_path = self.source_path(id);
        fs::remove_file(&source_path)?;

        self.sources_by_id.remove(&id.clone());
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
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("aix"))
            {
                continue;
            }

            // An already-installed LNReader source with the mode currently
            // off would otherwise make `Source::from_aix_file` return an
            // `Err` here — since this loop's result feeds `?` up to the
            // caller, that one archive would abort loading *every* other
            // installed source (Aidoku ones included) alongside it.
            if source::should_skip_disabled_lnreader_source(&path, self.settings.lnreader_enabled)
            {
                continue;
            }

            let source = Source::from_aix_file(&path, self, manager)?;
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
}

impl SourceCollection for SourceManager {
    fn get_by_id(&self, id: &SourceId) -> Option<&Source> {
        self.sources_by_id.get(id)
    }

    fn sources(&self) -> Vec<&Source> {
        self.sources_by_id.values().collect()
    }
}

/// Exercises the `lnreader_enabled` config toggle (§3.5.7,
/// `docs/lnreader/REFERENCE.md`) — independent of the `lnreader` Cargo
/// feature, which is why [`build_lnreader_aix`] and
/// [`disabled_toggle_rejects_install`]/[`disabled_toggle_is_skipped_on_load`]
/// don't need `#[cfg(feature = "lnreader")]`: the toggle defaults to `false`
/// regardless of the feature, so an LNReader-shaped archive must be rejected
/// the same way whether the mode is merely disabled or not compiled in at
/// all. Only the "it actually loads once enabled" test needs the feature.
#[cfg(test)]
mod lnreader_toggle_tests {
    use std::io::Write;

    use tokio::sync::Mutex;
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::model::SourceId;

    /// A minimal LNReader-shaped `.aix` — `main.js`'s content is never
    /// executed by `from_aix_file` (only read as text, see
    /// `sdk_lnreader::LnReaderSource::from_aix_file`), so it doesn't need to
    /// be a real plugin.
    fn build_lnreader_aix() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = SimpleFileOptions::default();
            zip.start_file("Payload/source.json", options).unwrap();
            zip.write_all(br#"{"info":{"id":"toggle-test","name":"Toggle Test","version":1}}"#)
                .unwrap();
            zip.start_file("Payload/main.js", options).unwrap();
            zip.write_all(b"exports.default = { id: 'toggle-test', name: 'Toggle Test', site: 'https://example.com' };")
                .unwrap();
            zip.finish().unwrap();
        }
        buf
    }

    fn manager_pair(
        sources_folder: std::path::PathBuf,
        settings: Settings,
    ) -> Arc<Mutex<SourceManager>> {
        Arc::new(Mutex::new(SourceManager::new(
            sources_folder,
            HashMap::new(),
            settings,
        )))
    }

    /// `install_source` must fail loudly and immediately when
    /// `lnreader_enabled` is left at its default (`false`) — never silently
    /// register an LNReader source.
    #[test]
    fn disabled_toggle_rejects_install() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let arc_manager = manager_pair(tmp_dir.path().to_path_buf(), Settings::default());
        let mut manager = SourceManager::new(
            tmp_dir.path().to_path_buf(),
            HashMap::new(),
            Settings::default(),
        );

        let id = SourceId::new("toggle-test".to_string());
        let result =
            manager.install_source(&id, build_lnreader_aix(), "test".to_string(), &arc_manager);

        assert!(
            result.is_err(),
            "installing an LNReader source with lnreader_enabled=false (the default) should fail"
        );
        assert!(!manager.sources_by_id.contains_key(&id));
    }

    /// A previously-installed LNReader source sitting in the sources folder
    /// (e.g. the toggle was on, got flipped off, then the server restarted)
    /// must be skipped by `load_all_sources`, not abort loading every other
    /// installed source alongside it (see that function's own doc comment).
    #[test]
    fn disabled_toggle_is_skipped_on_load() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let aix_path = tmp_dir.path().join("toggle-test.aix");
        fs::write(&aix_path, build_lnreader_aix()).unwrap();
        Source::write_meta_file(&aix_path, "test".to_string()).unwrap();

        let mut manager = SourceManager::new(
            tmp_dir.path().to_path_buf(),
            HashMap::new(),
            Settings::default(),
        );
        let arc_manager = manager_pair(tmp_dir.path().to_path_buf(), Settings::default());

        let sources = manager.load_all_sources(&arc_manager).expect(
            "load_all_sources should not fail just because a disabled LNReader source is present",
        );
        assert!(sources.is_empty());
    }

    /// The other half: once both gates are open (feature compiled in, and
    /// `lnreader_enabled: true`), the same archive that §disabled tests
    /// reject installs and loads normally.
    #[cfg(feature = "lnreader")]
    #[test]
    fn enabled_toggle_allows_install() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            lnreader_enabled: true,
            ..Settings::default()
        };
        let arc_manager = manager_pair(tmp_dir.path().to_path_buf(), settings.clone());
        let mut manager =
            SourceManager::new(tmp_dir.path().to_path_buf(), HashMap::new(), settings);

        let id = SourceId::new("toggle-test".to_string());
        manager
            .install_source(&id, build_lnreader_aix(), "test".to_string(), &arc_manager)
            .expect("installing an LNReader source with lnreader_enabled=true should succeed");

        assert!(manager.sources_by_id.contains_key(&id));
    }
}
