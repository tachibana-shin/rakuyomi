//! Phase 3 packaging pipeline: turns a compiled `lnreader-plugins` `.js`
//! file into a `.aix`-equivalent archive installable through Rakuyomi's
//! existing `source_lists`/`install_source` mechanism — no hardcoded
//! source, no new install path (see `docs/lnreader/PHASE3_HANDOFF.md`'s
//! non-negotiable (a)). Standalone binary, same pattern as
//! `cbz_metadata_reader`/`lnreader_worker`: depends on `shared` directly to
//! reuse its real types rather than re-deriving the `.aix`/settings schema.

mod index;
mod metadata;
mod package;
mod settings;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Package lnreader-plugins .js sources into Rakuyomi-installable .aix files")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Packages a single compiled .js file into `<sources_dir>/sources/<id>.aix`.
    /// All metadata (id/name/site/lang/version/settings) is read off the
    /// plugin itself by executing it — nothing needs to be supplied by
    /// hand, and this works on any `lnreader-plugins` source, not just a
    /// preselected sample.
    Package {
        /// Path to a compiled lnreader-plugins .js file.
        input_js: PathBuf,
        /// Directory to package into (a `sources/` subfolder is created
        /// under it).
        sources_dir: PathBuf,
    },
    /// (Re)writes `<sources_dir>/index.json` by scanning
    /// `<sources_dir>/sources/*.aix` — run after one or more `package`
    /// calls targeting the same `sources_dir`.
    Index {
        /// Directory containing a `sources/` subfolder of already-packaged
        /// `.aix` files.
        sources_dir: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Package {
            input_js,
            sources_dir,
        } => run_package(&input_js, &sources_dir),
        Command::Index { sources_dir } => run_index(&sources_dir),
    }
}

fn run_package(input_js: &Path, sources_dir: &Path) -> Result<()> {
    let main_js = std::fs::read_to_string(input_js)
        .with_context(|| format!("couldn't read {}", input_js.display()))?;

    let raw_json =
        shared::source::lnreader_extract_plugin_metadata(&main_js).with_context(|| {
            format!(
                "couldn't execute {} to read its metadata",
                input_js.display()
            )
        })?;
    let raw = metadata::RawMetadata::parse(raw_json)?;

    let (setting_definitions, skipped_filters) =
        settings::settings_from_plugin(&raw.filters, &raw.plugin_settings);
    if !skipped_filters.is_empty() {
        eprintln!(
            "warning: {}: unrecognized filter type(s), skipped: {}",
            raw.id,
            skipped_filters.join(", ")
        );
    }

    let version = metadata::encode_version(&raw.version);
    let params = package::SourceParams {
        id: raw.id.clone(),
        name: raw.name.clone(),
        lang: raw.lang.clone(),
        site: raw.site.clone(),
        version,
    };

    let output_sources_dir = sources_dir.join("sources");
    std::fs::create_dir_all(&output_sources_dir)
        .with_context(|| format!("couldn't create {}", output_sources_dir.display()))?;
    let output_path = output_sources_dir.join(format!("{}.aix", raw.id));

    package::write_aix(&params, &setting_definitions, &main_js, &output_path)?;

    println!(
        "{}",
        serde_json::json!({
            "id": raw.id,
            "name": raw.name,
            "site": raw.site,
            "lang": raw.lang,
            "version": version,
            "settings_count": setting_definitions.len(),
            "output": output_path.display().to_string(),
        })
    );

    Ok(())
}

fn run_index(sources_dir: &Path) -> Result<()> {
    let entries = index::build_index(&sources_dir.join("sources"))?;
    let index_path = sources_dir.join("index.json");
    std::fs::write(&index_path, serde_json::to_string_pretty(&entries)?)
        .with_context(|| format!("couldn't write {}", index_path.display()))?;

    println!(
        "wrote {} ({} source(s))",
        index_path.display(),
        entries.len()
    );

    Ok(())
}
