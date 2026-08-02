//! Writes the `.aix`-shaped zip archive itself, reusing `shared`'s own
//! `SourceManifest`/`SourceInfo`/`SettingDefinition` types (now also
//! `Serialize`, not just `Deserialize` — see the change to
//! `backend/shared/src/source/mod.rs`) so what this writes is guaranteed to
//! deserialize the same way `LnReaderSource::from_aix_file` reads it back;
//! no hand-written JSON literal to drift out of sync with the real schema.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use shared::source::model::SettingDefinition;
use shared::source::{SourceInfo, SourceManifest};
use zip::write::SimpleFileOptions;

pub struct SourceParams {
    pub id: String,
    pub name: String,
    pub lang: Option<String>,
    /// The plugin's base URL (`site`) — mapped to `SourceInfo::url`
    /// (singular). Multi-base-URL sources (`SourceInfo::urls`, which
    /// triggers a synthetic "URL" select setting in
    /// `LnReaderSource::from_aix_file`) aren't produced here: detecting
    /// that a plugin supports more than one site isn't something the
    /// metadata probe can tell generically, and none of the three sources
    /// this phase targets (NovelBuddy, LNori, Ranobes) need it.
    pub site: Option<String>,
    pub version: usize,
}

/// Writes `output_path` as a `.aix`-shaped zip: `Payload/source.json`
/// (always), `Payload/settings.json` (only when `settings` is non-empty —
/// `LnReaderSource::from_aix_file` already treats a missing file as "no
/// settings", so there's no need to write an empty array), and
/// `Payload/main.js` (the input file, copied verbatim, untouched).
pub fn write_aix(
    params: &SourceParams,
    settings: &[SettingDefinition],
    main_js: &str,
    output_path: &Path,
) -> Result<()> {
    let manifest = SourceManifest {
        info: SourceInfo {
            id: params.id.clone(),
            lang: params.lang.clone(),
            name: params.name.clone(),
            version: params.version,
            url: params.site.clone(),
            urls: None,
            min_app_version: None,
        },
        config: None,
        source_of_source: None,
    };

    let file = std::fs::File::create(output_path)
        .with_context(|| format!("couldn't create {}", output_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    zip.start_file("Payload/source.json", options)
        .context("failed to start source.json entry")?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())
        .context("failed to write source.json")?;

    if !settings.is_empty() {
        zip.start_file("Payload/settings.json", options)
            .context("failed to start settings.json entry")?;
        zip.write_all(serde_json::to_string_pretty(settings)?.as_bytes())
            .context("failed to write settings.json")?;
    }

    zip.start_file("Payload/main.js", options)
        .context("failed to start main.js entry")?;
    zip.write_all(main_js.as_bytes())
        .context("failed to write main.js")?;

    zip.finish().context("failed to finalize .aix file")?;

    Ok(())
}
