//! Scans a directory of already-packaged `.aix` files and rebuilds an
//! `index.json` entry per file — the "generate a source-list index" half of
//! Phase 3 (§3.2).

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use shared::model::SourceId;
use shared::source::SourceManifest;
use zip::ZipArchive;

/// One entry in the generated `index.json`. Deliberately includes both what
/// `list_available_sources` deserializes (`id`/`name`/`version`, see
/// `shared::model::SourceInformation`) and what `install_source` needs
/// (`file`, matched there against `SourceListItem::file`/its `downloadURL`
/// alias) in one flat shape — both usecases parse the very same
/// URL/document today (see
/// `backend/shared/src/usecases/{list_available_sources,install_source}.rs`),
/// just into two different structs that each ignore the other's extra
/// field.
#[derive(Serialize)]
pub struct IndexEntry {
    pub id: SourceId,
    pub name: String,
    pub version: usize,
    /// Bare filename, resolved by `install_source.rs` as
    /// `<index-url-dir>/sources/<file>` — so this index is only valid
    /// served from the same directory that a sibling `sources/` folder
    /// (containing these very `.aix` files) lives in.
    pub file: String,
}

/// Reads every `*.aix` in `sources_dir` back through the exact same
/// `SourceManifest` deserialization the runtime itself uses (rather than
/// trusting whatever a `package` invocation happened to be given), so the
/// generated index can never drift from what a `.aix` file actually
/// contains.
pub fn build_index(sources_dir: &Path) -> Result<Vec<IndexEntry>> {
    let mut paths: Vec<_> = std::fs::read_dir(sources_dir)
        .with_context(|| format!("couldn't read {}", sources_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("aix"))
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let manifest = read_manifest(&path)
                .with_context(|| format!("couldn't read source.json from {}", path.display()))?;
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .with_context(|| format!("{} has no valid file name", path.display()))?
                .to_string();

            Ok(IndexEntry {
                id: SourceId::new(manifest.info.id),
                name: manifest.info.name,
                version: manifest.info.version,
                file: file_name,
            })
        })
        .collect()
}

fn read_manifest(path: &Path) -> Result<SourceManifest> {
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let manifest_file = archive
        .by_name("Payload/source.json")
        .context("archive has no Payload/source.json")?;
    serde_json::from_reader(manifest_file).context("Payload/source.json didn't parse")
}
