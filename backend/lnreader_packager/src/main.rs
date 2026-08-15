//! Phase 3 packaging pipeline: turns a compiled `lnreader-plugins` `.js`
//! file into a `.aix`-equivalent archive installable through Rakuyomi's
//! existing `source_lists`/`install_source` mechanism — no hardcoded
//! source, no new install path. Standalone binary, same pattern as
//! `cbz_metadata_reader`/`lnreader_worker`: depends on `shared` directly to
//! reuse its real types rather than re-deriving the `.aix`/settings schema.
//!
//! `fetch` (Phase 3.5 follow-up, see `docs/lnreader/REFERENCE.md` §5) adds
//! bulk packaging straight from the upstream `plugins.min.json` index, on
//! top of the same `package_and_write` core `package` uses for one file.
//!
//! RECONSTRUCTED after an accidental `git checkout` discarded this file's
//! uncommitted content — see `docs/lnreader/REFERENCE.md`'s "File-loss
//! incident" section. Confidence is mixed by section: the `package`/
//! `index` commands are adapted with high confidence from this file's own
//! last-committed (Phase 3) version, updated only to call
//! `shared::source::packaging::package_plugin_js` (which absorbed this
//! crate's own now-deleted `metadata`/`settings`/`package` modules — see
//! REFERENCE.md §5.1) instead of those. The `fetch` command's overall shape
//! (required `--index-url`, best-effort per-plugin, doesn't abort the
//! batch) is well-documented in REFERENCE.md §3.3/§5.1/§5.3, but its exact
//! original code was not recovered — this is a genuine reconstruction, not
//! a byte-for-byte recovery, revalidated afterward against the real live
//! index rather than assumed correct.

mod index;
mod plugins_index;

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
    /// Downloads and packages every plugin listed in the upstream
    /// `plugins.min.json` index in one pass, then rebuilds `index.json` --
    /// the bulk equivalent of running `package` once per entry followed by
    /// `index`. Best-effort: one plugin failing to download or package
    /// (e.g. a missing Web API polyfill) is logged and skipped, not fatal
    /// to the whole batch -- see `docs/lnreader/REFERENCE.md` §5.3 for the
    /// last full run's real success rate.
    Fetch {
        /// URL of the upstream `plugins.min.json` index. Required, with no
        /// built-in default -- this tool never hardcodes an upstream
        /// repo/branch URL (see REFERENCE.md §5.1/§5.4).
        #[arg(long)]
        index_url: String,
        /// Directory to package into (a `sources/` subfolder is created
        /// under it, same as `package`).
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
        Command::Fetch {
            index_url,
            sources_dir,
        } => run_fetch(&index_url, &sources_dir),
    }
}

/// Runs `package_plugin_js` (which executes the plugin's top-level JS
/// in-process to read its metadata) on a separate OS thread with a timeout,
/// rather than calling it directly on the CLI's own thread. This binary has
/// no Tokio runtime to hand the timed-out call off to (unlike the server's
/// equivalent guard around the same function, see
/// `sdk_lnreader::packaging::install_from_url`), so a plain
/// `thread::spawn` + `recv_timeout` is used instead: a plugin with a
/// pathological/infinite top-level loop makes `run()`'s caller get back a
/// timeout error instead of hanging the whole `fetch` batch (which already
/// treats a per-plugin error as skip-and-continue, see `run_fetch`) forever
/// on one bad entry. The spawned thread itself is not cancelled on
/// timeout -- same caveat as the server-side guard -- but unlike a
/// long-lived server process, this one dies with the CLI process at the end
/// of the command, so a leaked thread here is bounded by that lifetime, not
/// permanent.
fn package_plugin_js_with_timeout(
    main_js: &str,
    index_url: Option<&str>,
) -> Result<shared::source::packaging::PackagedPlugin> {
    const METADATA_EXTRACTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    let main_js = main_js.to_string();
    let index_url = index_url.map(str::to_string);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = shared::source::packaging::package_plugin_js(&main_js, index_url.as_deref());
        let _ = tx.send(result);
    });

    match rx.recv_timeout(METADATA_EXTRACTION_TIMEOUT) {
        Ok(result) => result,
        Err(_) => anyhow::bail!(
            "timed out packaging plugin after {METADATA_EXTRACTION_TIMEOUT:?} \
             (plugin's top-level JS likely hung)"
        ),
    }
}

/// Packages one already-fetched plugin's `main_js` into
/// `<sources_dir>/sources/<id>.aix`, printing the same progress summary
/// `package` always has, shared with `fetch`'s per-entry loop below.
/// `index_url`, when given, only ever affects the `lang` fallback inside
/// `package_plugin_js` (see `docs/lnreader/REFERENCE.md` §5.3) -- never
/// used to derive a download URL here, `main_js` is always already in hand.
fn package_and_write(main_js: &str, index_url: Option<&str>, sources_dir: &Path) -> Result<()> {
    let packaged =
        package_plugin_js_with_timeout(main_js, index_url).context("couldn't package plugin")?;

    if !packaged.skipped_plugin_settings.is_empty() {
        eprintln!(
            "warning: {}: unsupported pluginSetting type(s), skipped: {}",
            packaged.id,
            packaged.skipped_plugin_settings.join(", ")
        );
    }

    if packaged.id.is_empty() || packaged.id.contains(['/', '\\']) {
        anyhow::bail!(
            "plugin declared an empty id or one containing a path separator, refusing to package: {:?}",
            packaged.id
        );
    }

    let output_sources_dir = sources_dir.join("sources");
    std::fs::create_dir_all(&output_sources_dir)
        .with_context(|| format!("couldn't create {}", output_sources_dir.display()))?;
    let output_path = output_sources_dir.join(format!("{}.aix", packaged.id));
    std::fs::write(&output_path, &packaged.bytes)
        .with_context(|| format!("couldn't write {}", output_path.display()))?;

    println!(
        "{}",
        serde_json::json!({
            "id": packaged.id,
            "name": packaged.name,
            "site": packaged.site,
            "lang": packaged.lang,
            "version": packaged.version,
            "settings_count": packaged.settings_count,
            "output": output_path.display().to_string(),
        })
    );

    Ok(())
}

fn run_package(input_js: &Path, sources_dir: &Path) -> Result<()> {
    let main_js = std::fs::read_to_string(input_js)
        .with_context(|| format!("couldn't read {}", input_js.display()))?;

    package_and_write(&main_js, None, sources_dir)
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

fn run_fetch(index_url: &str, sources_dir: &Path) -> Result<()> {
    let entries = plugins_index::fetch_index(index_url)
        .with_context(|| format!("couldn't fetch index at {index_url}"))?;

    let total = entries.len();
    let mut succeeded = 0usize;
    let mut failed: Vec<String> = Vec::new();

    for entry in &entries {
        match plugins_index::fetch_plugin_js(&entry.url) {
            Ok(main_js) => match package_and_write(&main_js, Some(&entry.url), sources_dir) {
                Ok(()) => succeeded += 1,
                Err(e) => {
                    eprintln!("warning: {}: {e:#}", entry.id);
                    failed.push(entry.id.clone());
                }
            },
            Err(e) => {
                eprintln!("warning: {}: {e:#}", entry.id);
                failed.push(entry.id.clone());
            }
        }
    }

    run_index(sources_dir)?;

    println!(
        "fetched {succeeded}/{total} plugin(s) from {index_url} ({} failed{})",
        failed.len(),
        if failed.is_empty() {
            String::new()
        } else {
            format!(": {}", failed.join(", "))
        }
    );

    Ok(())
}
