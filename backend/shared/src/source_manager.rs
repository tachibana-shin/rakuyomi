use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

use anyhow::{bail, Context, Result};

use crate::{
    model::SourceId,
    settings::{Settings, SourceSettingValue},
    source::{Source, SourceBackend},
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
        arc_manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<()> {
        let target_path = self.lnreader_source_path(id);
        fs::write(&target_path, contents)?;

        Source::write_meta_file(&target_path, source_of_source)?;

        let source = Source::from_lnreader_file(&target_path, self, arc_manager)?;
        // Installing is an explicit user action with the network up, so the
        // probe runs right away: it writes the probe cache (later loads read
        // it and skip the JS evaluation) and the source is fully probed from
        // the start, showing its real manifest in the installed-sources list.
        if let Err(e) = source
            .probe()
            .with_context(|| format!("failed to probe LNReader plugin {}", id.value()))
        {
            // Probe failed: remove the plugin and metadata files to avoid
            // leaving a partially installed source on disk.
            let _ = fs::remove_file(&target_path);
            if let Ok(meta_path) = crate::source::BlockingSource::meta_source_path(&target_path) {
                let _ = fs::remove_file(&meta_path);
            }
            let probe_path = self.lnreader_probe_path(id);
            let _ = fs::remove_file(&probe_path);
            return Err(e);
        }
        self.sources_by_id.insert(id.clone(), source);
        #[cfg(not(feature = "all"))]
        self.file_sources.insert(
            id.value().to_owned(),
            target_path.to_string_lossy().to_string(),
        );

        Ok(())
    }

    /// Installs a MangaYomi extension: the code is stored as
    /// `<id>.mangayomi.dart` or `<id>.mangayomi.js` (per the
    /// `sourceCodeLanguage` field of the index entry: `0` Dart, `1`
    /// JavaScript) with its `index.json` entry as a `<id>.mangayomi.json`
    /// sidecar. Anime extensions (`itemType: 1`) are rejected.
    pub fn install_mangayomi_source(
        &mut self,
        id: &SourceId,
        code: impl AsRef<[u8]>,
        metadata: impl AsRef<[u8]>,
        source_of_source: String,
        arc_manager: &Arc<Mutex<SourceManager>>,
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
        // The stored metadata must carry its own `id`; `from_mangayomi_file`
        // rejects metadata without one. The install pipelines that lose the
        // key (e.g. `#[serde(flatten)]` in `install_source`) restore it before
        // calling this.
        let metadata: serde_json::Value = match metadata.get("id") {
            Some(_) => metadata,
            None => bail!(
                "MangaYomi extension metadata for '{}' is missing its `id`",
                id.value()
            ),
        };
        let is_js = metadata
            .get("sourceCodeLanguage")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            == 1;
        let target_path = if is_js {
            self.mangayomi_js_source_path(id)
        } else {
            self.mangayomi_source_path(id)
        };
        fs::write(&target_path, code)?;
        fs::write(target_path.with_extension("json"), metadata.to_string())?;

        Source::write_meta_file(&target_path, source_of_source)?;

        let source = Source::from_mangayomi_file(&target_path, self, arc_manager)?;
        // See `install_lnreader_source`: the probe runs eagerly so the probe
        // cache is written and the source is fully probed from the start.
        if let Err(e) = source
            .probe()
            .with_context(|| format!("failed to probe MangaYomi extension {}", id.value()))
        {
            // Probe failed: remove the extension, metadata, and meta files to
            // avoid leaving a partially installed source on disk.
            let _ = fs::remove_file(&target_path);
            let _ = fs::remove_file(target_path.with_extension("json"));
            if let Ok(meta_path) = crate::source::BlockingSource::meta_source_path(&target_path) {
                let _ = fs::remove_file(&meta_path);
            }
            let probe_path = self.mangayomi_probe_path(id);
            let _ = fs::remove_file(&probe_path);
            return Err(e);
        }
        self.sources_by_id.insert(id.clone(), source);
        #[cfg(not(feature = "all"))]
        self.file_sources.insert(
            id.value().to_owned(),
            target_path.to_string_lossy().to_string(),
        );

        Ok(())
    }

    /// Installs a keiyoushi extension APK: the bytes are stored as
    /// `<pkg>.keiyoushi.apk` (one per extension package), and every source
    /// bundled in the APK is registered individually (see
    /// [`Source::from_keiyoushi_file`]).
    pub fn install_keiyoushi_source(
        &mut self,
        id: &SourceId,
        contents: impl AsRef<[u8]>,
        source_of_source: String,
        arc_manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<()> {
        let target_path = self.keiyoushi_source_path(id);
        fs::write(&target_path, contents)?;

        Source::write_meta_file(&target_path, source_of_source)?;

        let sources = Source::from_keiyoushi_file(&target_path, self, arc_manager)?;
        for source in sources {
            let source_id = SourceId::new(source.manifest().info.id.clone());
            #[cfg(not(feature = "all"))]
            self.file_sources.insert(
                source_id.value().to_owned(),
                target_path.to_string_lossy().to_string(),
            );
            self.sources_by_id.insert(source_id, source);
        }

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

    /// Removes a WASM, an LNReader, a MangaYomi and a Keiyoushi source file
    /// if present. Keiyoushi sources of the same APK share one file, so the
    /// removal clears every registered source of that extension.
    pub fn uninstall_any_source(&mut self, id: &SourceId) -> Result<()> {
        let mut removed = Vec::new();
        for path in [
            self.source_path(id),
            self.lnreader_source_path(id),
            self.lnreader_probe_path(id),
            self.mangayomi_source_path(id),
            self.mangayomi_js_source_path(id),
            self.mangayomi_probe_path(id),
            self.keiyoushi_source_path(id),
            self.keiyoushi_probe_path(id),
        ] {
            if path.exists() {
                fs::remove_file(&path)?;
                removed.push(path.clone());
            }
            let meta_path = path.with_extension("json");
            if meta_path.exists() {
                fs::remove_file(&meta_path)?;
            }
            if let Ok(meta_path) = crate::source::BlockingSource::meta_source_path(&path) {
                if meta_path.exists() {
                    fs::remove_file(&meta_path)?;
                }
            }
        }
        let removed: std::collections::HashSet<std::path::PathBuf> = removed.into_iter().collect();
        // An APK/JS container can register several sources under one file
        // (keiyoushi multiple extensions): every registered source whose
        // file was removed is dropped together with the requested one, so
        // the list never keeps dangling sources.
        let doomed: Vec<SourceId> = self
            .sources_by_id
            .keys()
            .filter(|id2| {
                let candidates = [
                    self.source_path(id2),
                    self.lnreader_source_path(id2),
                    self.mangayomi_source_path(id2),
                    self.mangayomi_js_source_path(id2),
                    self.keiyoushi_source_path(id2),
                ];
                candidates.iter().any(|p| removed.contains(p))
            })
            .cloned()
            .collect();
        for d in doomed {
            self.sources_by_id.remove(&d);
            #[cfg(not(feature = "all"))]
            self.file_sources.remove(d.value());
        }
        Ok(())
    }

    pub fn update_settings(
        &mut self,
        settings: Settings,
        manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<()> {
        // Only the per-source stored settings affect the loaded sources;
        // global settings (source lists, languages, ...) must not tear down
        // every extension. Reload just the files backing the sources whose
        // settings changed, instead of re-scanning and re-probing the whole
        // collection.
        let changed = self.changed_source_ids(&settings);
        self.settings = settings;
        if changed.is_empty() {
            return Ok(());
        }

        // Several sources may share one file (a keiyoushi APK registers one
        // source per bundled `Source`), so dedupe the affected files.
        let mut files = BTreeSet::new();
        for id in &changed {
            if let Some(path) = self.source_file_for_id(id) {
                files.insert(path);
            }
        }
        for path in files {
            self.reload_source_file(&path, manager)?;
        }

        Ok(())
    }

    /// The ids of the sources whose stored settings differ between the
    /// current settings and the given one.
    fn changed_source_ids(&self, settings: &Settings) -> Vec<SourceId> {
        let old = &self.settings.source_settings;
        let new = &settings.source_settings;
        let mut keys: Vec<&String> = old.keys().collect();
        keys.extend(new.keys());
        keys.sort();
        keys.dedup();
        keys.into_iter()
            .filter(|key| old.get(*key) != new.get(*key))
            .map(|key| SourceId::new(key.clone()))
            .collect()
    }

    /// The on-disk file a registered source was loaded from, if any.
    fn source_file_for_id(&self, id: &SourceId) -> Option<PathBuf> {
        #[cfg(not(feature = "all"))]
        if let Some(path) = self.file_sources.get(id.value()) {
            return Some(PathBuf::from(path));
        }
        let candidates = match self.sources_by_id.get(id).map(|source| &source.backend) {
            Some(SourceBackend::Keiyoushi(keiyoushi)) => {
                vec![keiyoushi.apk_path().to_path_buf()]
            }
            _ => vec![],
        };
        candidates
            .into_iter()
            .chain([
                self.lnreader_source_path(id),
                self.mangayomi_source_path(id),
                self.mangayomi_js_source_path(id),
                self.keiyoushi_source_path(id),
            ])
            .find(|path| path.exists())
    }

    /// Drop every source registered from `path`, then re-register them from
    /// the file. Re-running the loader picks up the freshly saved stored
    /// settings, and dropping the old sources tears down their worker
    /// engines so the next call boots with the new values.
    fn reload_source_file(
        &mut self,
        path: &Path,
        manager: &Arc<Mutex<SourceManager>>,
    ) -> Result<()> {
        let doomed: Vec<SourceId> = self
            .sources_by_id
            .keys()
            .filter(|id| self.source_file_for_id(id).as_deref() == Some(path))
            .cloned()
            .collect();
        for id in &doomed {
            self.sources_by_id.remove(id);
            #[cfg(not(feature = "all"))]
            self.file_sources.remove(id.value());
        }

        let name = path.file_name().map(|n| n.to_string_lossy().to_string());
        let is_keiyoushi = name
            .as_deref()
            .is_some_and(|name| name.ends_with(crate::source::keiyoushi::KEIYOUSHI_FILE_SUFFIX));
        let is_lnreader = name
            .as_deref()
            .is_some_and(|name| name.ends_with(crate::source::lnreader::LNREADER_FILE_SUFFIX));
        let is_mangayomi = name.as_deref().is_some_and(|name| {
            name.ends_with(crate::source::mangayomi::MANGA_YOMI_FILE_SUFFIX)
                || name.ends_with(crate::source::mangayomi::MANGA_YOMI_JS_FILE_SUFFIX)
        });

        let sources = if is_keiyoushi {
            Source::from_keiyoushi_file(path, self, manager)?
        } else if is_lnreader {
            vec![Source::from_lnreader_file(path, self, manager)?]
        } else if is_mangayomi {
            vec![Source::from_mangayomi_file(path, self, manager)?]
        } else {
            vec![Source::from_aix_file(path, self, manager)?]
        };

        for source in sources {
            let id = source.manifest().info.id.clone();
            #[cfg(not(feature = "all"))]
            self.file_sources
                .insert(id.clone(), path.to_string_lossy().to_string());
            self.sources_by_id.insert(SourceId::new(id.clone()), source);
        }

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
            let is_mangayomi = path.file_name().is_some_and(|name| {
                let ext = name.to_string_lossy();
                ext.ends_with(crate::source::mangayomi::MANGA_YOMI_FILE_SUFFIX)
                    || ext.ends_with(crate::source::mangayomi::MANGA_YOMI_JS_FILE_SUFFIX)
            });
            let is_keiyoushi = path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .ends_with(crate::source::keiyoushi::KEIYOUSHI_FILE_SUFFIX)
            });

            let source = if is_keiyoushi {
                // A keiyoushi APK registers one source per bundled `Source`;
                // every one of them maps back to this file on disk.
                for source in Source::from_keiyoushi_file(&path, self, manager)? {
                    #[cfg(not(feature = "all"))]
                    self.file_sources.insert(
                        source.manifest().info.id.clone(),
                        path.as_path().to_string_lossy().to_string(),
                    );
                    sources_by_id.insert(SourceId::new(source.manifest().info.id.clone()), source);
                }
                continue;
            } else if is_lnreader {
                Source::from_lnreader_file(&path, self, manager)?
            } else if is_mangayomi {
                Source::from_mangayomi_file(&path, self, manager)?
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

    /// The path of the probe cache sidecar of an LNReader plugin.
    pub fn lnreader_probe_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!(
            "{}{}",
            id.value(),
            crate::source::lnreader::LNREADER_PROBE_SUFFIX
        ))
    }

    pub fn mangayomi_source_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!(
            "{}{}",
            id.value(),
            crate::source::mangayomi::MANGA_YOMI_FILE_SUFFIX
        ))
    }

    pub fn mangayomi_js_source_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!(
            "{}{}",
            id.value(),
            crate::source::mangayomi::MANGA_YOMI_JS_FILE_SUFFIX
        ))
    }

    /// The path of the probe cache sidecar of a MangaYomi extension.
    pub fn mangayomi_probe_path(&self, id: &SourceId) -> PathBuf {
        self.sources_folder.join(format!(
            "{}{}",
            id.value(),
            crate::source::mangayomi::MANGA_YOMI_PROBE_SUFFIX
        ))
    }

    /// The path of the keiyoushi extension APK of a source id. Multi-source
    /// APKs register their sources as `<pkg>:<lang>`; they all share the
    /// `<pkg>.keiyoushi.apk` file.
    pub fn keiyoushi_source_path(&self, id: &SourceId) -> PathBuf {
        let pkg = id.value().split(':').next().unwrap_or(id.value());
        self.sources_folder.join(format!(
            "{pkg}{}",
            crate::source::keiyoushi::KEIYOUSHI_FILE_SUFFIX
        ))
    }

    /// The path of the probe cache sidecar of a keiyoushi extension APK
    /// (one per extension package).
    pub fn keiyoushi_probe_path(&self, id: &SourceId) -> PathBuf {
        let pkg = id.value().split(':').next().unwrap_or(id.value());
        self.sources_folder.join(format!(
            "{pkg}{}",
            crate::source::keiyoushi::KEIYOUSHI_PROBE_SUFFIX
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
