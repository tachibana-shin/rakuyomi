//! Third `Source` execution mode: LNReader plugins (raw JS, cheerio-based
//! scraping) instead of WASM/Aidoku. See `docs/lnreader/faisabilite-v2-adapter-rakuyomi.md`
//! §1-3 for the mapping this module implements, and §10 for what changed
//! once this was built and tested against real sources.
//!
//! [`LnReaderSource`] implements the same 7 base operations
//! `WasmBlockingSource` does, but — unlike `WasmBlockingSource`, which keeps
//! its WASM instance alive in-process for the source's whole lifetime —
//! every operation that runs plugin JS is delegated to a disposable
//! subprocess (see [`worker`]) instead of an in-process
//! [`js_runtime::JsRuntime`]. This is deliberate, not an accident of
//! layering: a source with a large catalog crashes the process running its
//! JS (confirmed via debugger to be a native memory issue inside
//! `boa_engine` itself, §10.3), and a native crash takes down the whole OS
//! process — a worker *thread* would not have contained it. `LnReaderSource`
//! only holds plain data (`main_js`, `SourceSettings`) plus a small cache
//! for `imageRequestInit`, and spawns a fresh worker per top-level call.

mod cheerio;
mod convert;
mod dayjs;
mod htmlparser2;
mod js_runtime;
pub mod metadata;
mod net;
pub mod worker;

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use reqwest::Request;
use tokio_util::sync::CancellationToken;
use url::Url;
use zip::ZipArchive;

use crate::settings::SourceSettingValue;
use crate::source::model::{Chapter, Manga, Page, SettingDefinition};
use crate::source::source_settings::SourceSettings;
use crate::source::wasm_imports::net::DEFAULT_USER_AGENT;
use crate::source::wasm_store::RequestBuildingState;
use crate::source::{SourceFeatures, SourceManifest};
use crate::source_manager::SourceManager;

/// How long [`LnReaderSource::run_worker`] waits for a response line before
/// deciding the persistent worker has hung (as opposed to crashed, which is
/// detected separately via a closed pipe — see [`read_line_with_timeout`]).
/// Generous on purpose: a single real `parseNovel()` call chaining several
/// network requests (e.g. `Promise.all` over many volumes) legitimately took
/// up to ~31s against a real, non-hung source during testing — this needs to
/// never trip on a legitimately slow-but-working source, only on a genuine
/// infinite loop/deadlock.
const WORKER_READ_TIMEOUT: Duration = Duration::from_secs(120);

/// A worker subprocess kept alive across every call for one loaded LNReader
/// source — same lifecycle as a WASM instance's `Store` (see
/// `WasmBlockingSource`), not spawned fresh per call. `stdin`/`stdout` are
/// `Option` (never `None` except transiently, mid-call, while temporarily
/// taken out for a write/read — `run_worker` always restores them) rather
/// than plain fields, since `WorkerProcess` implements [`Drop`]: Rust
/// forbids partially moving fields out of a type with a `Drop` impl, but
/// `Option::take()` on one field at a time is fine.
struct WorkerProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl Drop for WorkerProcess {
    /// A source can now outlive many calls (reload, uninstall, or the whole
    /// backend shutting down) instead of the old one-shot worker exiting
    /// right after its single call — so unlike before, something has to
    /// actually tell a still-running worker to stop. Dropping `stdin` first
    /// closes that pipe, which is the exact EOF signal `worker::run()`'s
    /// loop already exits cleanly on (see its doc comment) — no need to
    /// kill a healthy worker. `wait()` then reaps it so it doesn't linger as
    /// a zombie for the rest of this backend process's life. Best-effort:
    /// if the child is already gone (killed after a prior crash/timeout),
    /// both operations are cheap no-ops.
    fn drop(&mut self) {
        self.stdin.take(); // dropped here -> pipe closes -> child sees EOF
        let _ = self.child.wait();
    }
}

pub(super) struct LnReaderSource {
    id: String,
    manifest: SourceManifest,
    setting_definitions: Vec<SettingDefinition>,
    features: SourceFeatures,
    main_js: String,
    source_settings: SourceSettings,
    /// `imageRequestInit.headers` is a static property (doesn't depend on
    /// the image URL) but `get_image_request` is called once per `<img>` in
    /// a chapter — fetched once via the worker on first use and cached here
    /// instead of hitting the worker per image.
    image_request_init_headers: Option<HashMap<String, String>>,
    /// The persistent worker process for this source, `Some` for its entire
    /// lifetime except during the brief window between a detected crash/hang
    /// and the next call's respawn — see [`LnReaderSource::run_worker`].
    worker: Option<WorkerProcess>,
}

impl LnReaderSource {
    /// Loads a `.aix`-shaped archive containing `Payload/main.js` instead of
    /// `Payload/main.wasm`. Reads `Payload/source.json` and
    /// `Payload/settings.json` itself (same schema/logic as
    /// `WasmBlockingSource::from_aix_file`, just not shared code with it —
    /// keeping that function untouched was the point of wrapping
    /// `BlockingSource` in an enum, see the session plan). No
    /// `is_next_sdk`/meta-file caching here: unlike the Aidoku legacy/next
    /// ABI guess, detecting this mode is unambiguous and cheap (presence of
    /// `Payload/main.js`), so there's nothing to cache.
    pub(super) fn from_aix_file(
        path: &Path,
        manager: &SourceManager,
        arc_manager: &Arc<tokio::sync::Mutex<SourceManager>>,
    ) -> Result<Self> {
        let file =
            fs::File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("couldn't open source archive {}", path.display()))?;

        let manifest_file = archive
            .by_name("Payload/source.json")
            .context("while loading source.json")?;
        let manifest: SourceManifest = serde_json::from_reader(manifest_file)?;

        let url_settings = {
            let manifest = manifest.clone();
            manifest.info.urls.map(|urls| SettingDefinition::Select {
                title: "URL".to_owned(),
                key: "url".to_owned(),
                default: Some(urls.first().unwrap_or(&"".to_owned()).to_string()),
                values: urls,
                titles: None,
            })
        };
        let url_settings_support = url_settings.is_some();

        let mut setting_definitions: Vec<SettingDefinition> =
            if let Ok(file) = archive.by_name("Payload/settings.json") {
                serde_json::from_reader(file).context("while reading Payload/settings.json")?
            } else {
                Vec::new()
            };
        if let Some(url) = url_settings {
            setting_definitions.insert(0, url);
        }

        let stored_source_settings = manager
            .settings
            .source_settings
            .get(&manifest.info.id)
            .cloned()
            .unwrap_or_default();

        let id = manifest.info.id.clone();

        let source_settings = SourceSettings::new(
            id.clone(),
            &setting_definitions,
            &stored_source_settings,
            arc_manager,
        )?;
        if !url_settings_support && source_settings.get(&"url".to_string()).is_none() {
            if let Some(url) = manifest.info.url.clone() {
                source_settings.set("url", SourceSettingValue::String(url));
            }
        }

        let mut main_js = String::new();
        archive
            .by_name("Payload/main.js")
            .context("while loading main.js")?
            .read_to_string(&mut main_js)
            .with_context(|| format!("failed reading main.js from zip entry {}", path.display()))?;

        // Eager, like `blocking_source.start()` for a WASM next-SDK source
        // (`Source::from_aix_file`): the same tradeoff is accepted here on
        // purpose (a worker process alive for every installed source, even
        // ones that are never actually opened, rather than guessing at a
        // lazy-init optimization the project hasn't confirmed it needs) —
        // revisit only if idle-worker memory turns out to matter in
        // practice.
        let worker = Some(spawn_worker()?);

        Ok(Self {
            id,
            manifest,
            setting_definitions,
            // No WASM export to probe for this mode, and no LNReader concept
            // of post-processing a page image.
            features: SourceFeatures {
                process_page_image: false,
            },
            main_js,
            source_settings,
            image_request_init_headers: None,
            worker,
        })
    }

    pub(super) fn manifest(&self) -> SourceManifest {
        self.manifest.clone()
    }

    pub(super) fn setting_definitions(&self) -> Vec<SettingDefinition> {
        self.setting_definitions.clone()
    }

    pub(super) fn features(&self) -> SourceFeatures {
        self.features.clone()
    }

    /// Not reachable from the real app today: `Backend.lua` only exposes
    /// `Backend.searchMangas`, no browse/popular screen — `get_manga_list`
    /// exists solely because it's part of the `Source` trait shape (nothing
    /// in `server/src` calls it either, confirmed by grep). Mapped onto
    /// `search_mangas` with an empty query, same as the existing
    /// Aidoku-legacy path already does (`search_mangas_by_filters_inner(vec![])`
    /// in `WasmBlockingSource::get_manga_list`) — not worth a real
    /// `popularNovels()` call/UI-shaped implementation for a path nothing
    /// user-facing exercises.
    pub(super) fn get_manga_list(
        &mut self,
        cancellation_token: CancellationToken,
        _listing: aidoku::Listing,
    ) -> Result<Vec<Manga>> {
        let (mangas, _has_next_page) = self.search_mangas(cancellation_token, String::new(), 1)?;
        Ok(mangas)
    }

    pub(super) fn search_mangas(
        &mut self,
        _cancellation_token: CancellationToken,
        query: String,
        page: i32,
    ) -> Result<(Vec<Manga>, bool)> {
        let response = self.run_worker(worker::Operation::SearchMangas { query, page })?;
        let mangas = response
            .mangas
            .unwrap_or_default()
            .into_iter()
            .map(worker::MangaDto::into_manga)
            .collect();
        let has_next_page = response.has_next_page.unwrap_or(false);
        Ok((mangas, has_next_page))
    }

    pub(super) fn get_manga_details(
        &mut self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Manga> {
        let response = self.run_worker(worker::Operation::GetMangaDetails { manga_id })?;
        response
            .mangas
            .and_then(|mangas| mangas.into_iter().next())
            .map(worker::MangaDto::into_manga)
            .context("LNReader worker returned no manga for get_manga_details")
    }

    pub(super) fn get_chapter_list(
        &mut self,
        _cancellation_token: CancellationToken,
        manga_id: String,
    ) -> Result<Vec<Chapter>> {
        let response = self.run_worker(worker::Operation::GetChapterList { manga_id })?;
        Ok(response
            .chapters
            .unwrap_or_default()
            .into_iter()
            .map(worker::ChapterDto::into_chapter)
            .collect())
    }

    pub(super) fn get_page_list(
        &mut self,
        _cancellation_token: CancellationToken,
        _manga_id: String,
        chapter_id: String,
        chapter_num: Option<f32>,
    ) -> Result<Vec<Page>> {
        let response = self.run_worker(worker::Operation::GetPageList {
            chapter_id: chapter_id.clone(),
        })?;
        let html = response
            .page_html
            .context("LNReader worker returned no page HTML for get_page_list")?;

        // No chapter title is available at this call site (same limitation
        // as the existing WASM `get_page_list_inner`, which only has
        // `id`/`mangaId`/`chapterNum` to work with) — fall back to the
        // chapter number if we have one; `chapter_downloader.rs` falls back
        // to "Page N" on its own if `base64` is `None`.
        let title = chapter_num.map(|n| format!("Chapter {n}"));

        Ok(vec![convert::page_from_chapter_html(
            html,
            &self.id,
            &chapter_id,
            title,
        )])
    }

    pub(super) fn get_image_request(
        &mut self,
        url: Url,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Request> {
        if self.image_request_init_headers.is_none() {
            let response = self.run_worker(worker::Operation::GetImageRequestInitHeaders)?;
            self.image_request_init_headers =
                Some(response.image_request_init_headers.unwrap_or_default());
        }

        let mut headers = self.image_request_init_headers.clone().unwrap_or_default();
        headers
            .entry("User-Agent".to_string())
            .or_insert_with(|| DEFAULT_USER_AGENT.to_string());

        let building_state = RequestBuildingState {
            url: Some(url),
            method: Some(reqwest::Method::GET),
            body: None,
            headers,
            timeout: None,
        };

        (&building_state).try_into()
    }

    pub(super) fn process_page_image(
        &mut self,
        _cancellation_token: CancellationToken,
        _request: (Url, reqwest::header::HeaderMap),
        _response: (reqwest::StatusCode, reqwest::header::HeaderMap),
        _bytes: tokio_util::bytes::Bytes,
        _ctx: Option<aidoku::PageContext>,
    ) -> Result<Vec<u8>> {
        // No LNReader concept of post-processing a page image, and
        // `SourceFeatures::process_page_image` is hardcoded `false` for this
        // mode (see `source/mod.rs`), so nothing should ever reach here.
        bail!("process_page_image is not supported for LNReader sources")
    }

    pub(super) fn handle_notification_next(
        &mut self,
        _cancellation_token: CancellationToken,
        _key: String,
    ) -> Result<()> {
        // No LNReader equivalent of Aidoku's notification hook; a no-op
        // (rather than an error) since this is reachable from a generic
        // server route for any installed source.
        Ok(())
    }

    /// Sends one request to this source's persistent worker process (see
    /// [`worker`]'s doc comment for the crash-containment rationale),
    /// waits for its response (with a hang timeout, see
    /// [`read_line_with_timeout`]), and applies any settings it wrote back
    /// (`storage.set(...)`, see `js_runtime::register_storage`) via
    /// `SourceSettings::save` — this process has the live `SourceManager`
    /// handle the worker doesn't.
    ///
    /// Respawns transparently: if `self.worker` is `None` (the very first
    /// call, or the previous call detected the worker was dead/hung), a
    /// fresh one is spawned first. A crash or hang detected THIS call leaves
    /// `self.worker` as `None` again for the NEXT call to respawn — the
    /// current call still fails with a normal, catchable `Err` either way.
    /// An ordinary plugin-level failure (`response.ok == false`, e.g. a
    /// rejected promise) is different: the worker itself answered fine, so
    /// it's kept alive for the next call, exactly like a failed WASM call
    /// doesn't invalidate the WASM instance either.
    fn run_worker(&mut self, operation: worker::Operation) -> Result<worker::WorkerResponse> {
        self.run_worker_with_timeout(operation, WORKER_READ_TIMEOUT)
    }

    /// The actual logic behind [`run_worker`], with the hang timeout
    /// injectable — production always goes through `run_worker`'s fixed
    /// [`WORKER_READ_TIMEOUT`]; tests use a short one to exercise the
    /// timeout/respawn path without a 120s wait (see
    /// `hung_worker_times_out_and_respawns`).
    fn run_worker_with_timeout(
        &mut self,
        operation: worker::Operation,
        timeout: Duration,
    ) -> Result<worker::WorkerResponse> {
        if self.worker.is_none() {
            self.worker = Some(spawn_worker()?);
        }
        // Taken out (not just borrowed) so a crash/timeout path below can
        // simply let it drop -- its `Drop` impl reaps the process -- rather
        // than juggling `self.worker` across an early-return. Only put back
        // into `self.worker` on the healthy paths.
        let mut proc = self.worker.take().expect("just ensured Some above");

        let settings_snapshot = self.source_settings.snapshot();
        let request = worker::WorkerRequest {
            main_js: self.main_js.clone(),
            settings_snapshot,
            source_id: self.id.clone(),
            operation,
        };
        let mut request_line = serde_json::to_string(&request)
            .context("failed to serialize LNReader worker request")?;
        request_line.push('\n');

        let mut stdin = proc
            .stdin
            .take()
            .expect("only None mid-call, never observable at the start of one");
        let write_result = stdin
            .write_all(request_line.as_bytes())
            .and_then(|()| stdin.flush());
        proc.stdin = Some(stdin);

        if let Err(e) = write_result {
            // `proc` drops at the end of this arm (never restored into
            // `self.worker`), reaping the process via `Drop for
            // WorkerProcess` -- the next call respawns.
            bail!(
                "LNReader worker subprocess for {} is unusable (failed to write request: {e}); will respawn on next call",
                self.id
            );
        }

        let stdout = proc
            .stdout
            .take()
            .expect("only None mid-call, never observable at the start of one");

        match read_line_with_timeout(stdout, timeout) {
            ReadOutcome::Response(stdout, response) => {
                // The worker answered -- healthy, keep it for the next call
                // regardless of whether THIS call's result was itself ok.
                proc.stdout = Some(stdout);
                self.worker = Some(proc);

                if !response.ok {
                    bail!(
                        "{}",
                        response.error.unwrap_or_else(|| {
                            "LNReader worker reported failure with no message".to_string()
                        })
                    );
                }

                for (key, value) in &response.storage_writes {
                    // Best-effort: the plugin already got its result either
                    // way, and a failed settings write shouldn't fail the
                    // whole operation.
                    let _ = self.source_settings.save(key, value.clone());
                }

                Ok(response)
            }
            ReadOutcome::Dead(reason) => {
                let status = describe_wait_result(proc.child.wait());
                // `proc` drops here (never restored into `self.worker`) --
                // its `Drop` impl's own `wait()` afterward is a harmless
                // redundant no-op at that point.
                bail!(
                    "LNReader worker subprocess for {} crashed or exited abnormally ({status}): {reason}; will respawn on next call",
                    self.id
                );
            }
            ReadOutcome::Timeout => {
                // The reader thread is still blocked on the (now orphaned)
                // `stdout` handle -- killing the process unblocks it (the
                // pipe closes, the blocked read returns), so that thread
                // exits on its own; nothing here needs to join it.
                let _ = proc.child.kill();
                let status = describe_wait_result(proc.child.wait());
                bail!(
                    "LNReader worker subprocess for {} did not respond within {timeout:?} ({status}, presumed hung); killed, will respawn on next call",
                    self.id
                );
            }
        }
    }
}

/// Spawns a fresh `lnreader_worker` subprocess with its pipes wired up, but
/// sends it nothing yet — the first [`worker::WorkerRequest`] written to its
/// stdin (by `run_worker`) is what actually builds its `JsRuntime` (see
/// `worker::run`'s doc comment). Stderr is inherited (not piped): unlike the
/// old one-shot design, this process can live for a long time, and
/// continuously draining a piped stderr would need its own always-running
/// reader thread just to stop the pipe buffer from ever filling up and
/// blocking the child — inheriting sends any panic/log output straight to
/// the backend's own stderr/log stream instead, which is simpler and loses
/// nothing (it's still visible, just not embedded in the returned error
/// string anymore).
fn spawn_worker() -> Result<WorkerProcess> {
    let exe = worker_binary_path()?;
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn LNReader worker subprocess")?;
    let stdin = child
        .stdin
        .take()
        .context("LNReader worker subprocess has no stdin")?;
    let stdout = child
        .stdout
        .take()
        .context("LNReader worker subprocess has no stdout")?;
    Ok(WorkerProcess {
        child,
        stdin: Some(stdin),
        stdout: Some(BufReader::new(stdout)),
    })
}

enum ReadOutcome {
    /// A full response line arrived in time — carries `stdout` back so the
    /// caller can keep using the same reader for the next call.
    Response(BufReader<ChildStdout>, worker::WorkerResponse),
    /// The pipe closed (EOF, i.e. the process exited) or errored, or the
    /// line wasn't valid JSON, all detected within the timeout window — the
    /// worker is unusable either way, `stdout` isn't returned since there's
    /// nothing more to read from it.
    Dead(String),
    /// No response within the timeout — the worker is presumed hung. The
    /// reader thread is still blocked at this point (see the caller).
    Timeout,
}

/// Reads one line from `stdout` on a separate thread and waits for it with a
/// timeout, since `std::process`'s pipes have no built-in per-read timeout
/// (unlike a `TcpStream`). Ownership of `stdout` moves into that thread; on
/// anything other than [`ReadOutcome::Timeout`] it comes back out through
/// the channel so the caller can keep it for the next call. On a timeout,
/// the caller kills the child process (see `run_worker`), which unblocks the
/// thread's blocking read (the pipe closes under it) so it exits and drops
/// `stdout` on its own — nothing here needs to join it.
fn read_line_with_timeout(mut stdout: BufReader<ChildStdout>, timeout: Duration) -> ReadOutcome {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let result = stdout.read_line(&mut line);
        let _ = tx.send((result, line, stdout));
    });

    match rx.recv_timeout(timeout) {
        Ok((Ok(0), _line, _stdout)) => ReadOutcome::Dead("worker closed its output".to_string()),
        Ok((Ok(_), line, stdout)) => match serde_json::from_str(line.trim()) {
            Ok(response) => ReadOutcome::Response(stdout, response),
            Err(e) => ReadOutcome::Dead(format!("malformed response ({e}): {line:?}")),
        },
        Ok((Err(e), _line, _stdout)) => ReadOutcome::Dead(e.to_string()),
        Err(mpsc::RecvTimeoutError::Timeout) => ReadOutcome::Timeout,
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            ReadOutcome::Dead("worker reader thread panicked".to_string())
        }
    }
}

fn describe_wait_result(result: std::io::Result<std::process::ExitStatus>) -> String {
    match result {
        Ok(status) => describe_exit_status(&status),
        Err(e) => format!("failed to reap worker process: {e}"),
    }
}

/// Path to the `lnreader_worker` binary — deployed as a sibling of `server`
/// (same pattern as `uds_http_request`/`cbz_metadata_reader`, see that
/// crate). Resolved relative to the current executable's directory rather
/// than assumed to be on `PATH`, so it works the same way in a packaged
/// install and in a plain `cargo run`/`cargo test` from this workspace.
/// Test/bench binaries live one level deeper than regular bin targets
/// (`target/debug/deps/shared-<hash>`, vs. `target/debug/lnreader_worker`),
/// so both the executable's own directory and its parent are tried.
fn worker_binary_path() -> Result<std::path::PathBuf> {
    let current = std::env::current_exe()
        .context("failed to determine current executable path for LNReader worker")?;
    let dir = current
        .parent()
        .context("current executable has no parent directory")?;
    let name = if cfg!(windows) {
        "lnreader_worker.exe"
    } else {
        "lnreader_worker"
    };

    let same_dir = dir.join(name);
    if same_dir.is_file() {
        return Ok(same_dir);
    }
    if let Some(parent_dir) = dir.parent() {
        let sibling = parent_dir.join(name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    bail!(
        "couldn't find the lnreader_worker binary next to {} (looked in {} and its parent)",
        current.display(),
        dir.display()
    );
}

#[cfg(unix)]
fn describe_exit_status(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match status.signal() {
        Some(sig) => format!("killed by signal {sig}"),
        None => format!("exit code {:?}", status.code()),
    }
}

#[cfg(not(unix))]
fn describe_exit_status(status: &std::process::ExitStatus) -> String {
    format!("{status:?}")
}

/// End-to-end tests against real, vendored LNReader plugins
/// (`test_fixtures/`, see their header comments for provenance). These hit
/// real sites over the network, so they're `#[ignore]`d by default — run
/// explicitly with:
/// `cargo test -p shared --features all -- --ignored sdk_lnreader`.
///
/// These go through the real `run_worker`/`worker_binary_path` path, same as
/// production — `cargo test` builds the `lnreader_worker` bin target into
/// the same `target/<profile>/` tree as everything else, and
/// `worker_binary_path` already checks both the test binary's own directory
/// (`target/debug/deps/`) and its parent (`target/debug/`, where bin targets
/// actually land) to find it. Nothing test-specific needed here.
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::Arc;
    use std::time::Duration;

    use zip::write::SimpleFileOptions;

    use super::LnReaderSource;

    use crate::settings::Settings;
    use crate::source::Source;
    use crate::source_manager::SourceManager;
    use tokio_util::sync::CancellationToken;

    /// Builds a minimal `.aix`-shaped zip (`Payload/source.json` +
    /// `Payload/main.js`) in a temp dir, returning everything
    /// `Source::from_aix_file`/`LnReaderSource::from_aix_file` need. Shared
    /// by [`load_test_source`] (the real `Source` facade, for end-to-end
    /// tests) and [`load_test_lnreader_source`] (bypasses `Source` to reach
    /// `LnReaderSource`'s own test-only hooks, e.g.
    /// `run_worker_with_timeout` — see [`hung_worker_times_out_and_respawns`]).
    fn build_test_aix(
        fixture_id: &str,
        main_js: &str,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        SourceManager,
        Arc<tokio::sync::Mutex<SourceManager>>,
    ) {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let aix_path = tmp_dir.path().join(format!("{fixture_id}.aix"));

        let file = std::fs::File::create(&aix_path).expect("failed to create test .aix file");
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        zip.start_file("Payload/source.json", options)
            .expect("failed to start source.json entry");
        zip.write_all(
            format!(r#"{{"info":{{"id":"{fixture_id}","name":"{fixture_id}","version":1}}}}"#)
                .as_bytes(),
        )
        .expect("failed to write source.json");

        zip.start_file("Payload/main.js", options)
            .expect("failed to start main.js entry");
        zip.write_all(main_js.as_bytes())
            .expect("failed to write main.js");

        zip.finish().expect("failed to finalize test .aix file");

        let manager = SourceManager::new(
            tmp_dir.path().to_path_buf(),
            HashMap::new(),
            Settings::default(),
        );
        let arc_manager = Arc::new(tokio::sync::Mutex::new(SourceManager::new(
            tmp_dir.path().to_path_buf(),
            HashMap::new(),
            Settings::default(),
        )));

        (tmp_dir, aix_path, manager, arc_manager)
    }

    /// Loads a test source through the real `Source::from_aix_file` entry
    /// point — the same path a real install goes through, not a shortcut
    /// into `LnReaderSource` directly.
    fn load_test_source(fixture_id: &str, main_js: &str) -> Source {
        let (_tmp_dir, aix_path, manager, arc_manager) = build_test_aix(fixture_id, main_js);
        Source::from_aix_file(&aix_path, &manager, &arc_manager)
            .expect("failed to load test LNReader source")
    }

    /// Loads a test source as a bare `LnReaderSource`, bypassing `Source`'s
    /// `Arc<Mutex<..>>`/`spawn_blocking` wrapping — needed to reach
    /// `LnReaderSource`'s own private test hooks (`run_worker_with_timeout`)
    /// that aren't part of the `Source` trait surface.
    fn load_test_lnreader_source(fixture_id: &str, main_js: &str) -> LnReaderSource {
        let (_tmp_dir, aix_path, manager, arc_manager) = build_test_aix(fixture_id, main_js);
        LnReaderSource::from_aix_file(&aix_path, &manager, &arc_manager)
            .expect("failed to load test LNReader source")
    }

    /// Exercises `search_mangas` -> `get_chapter_list` -> `get_page_list`
    /// against a real site, asserting non-empty/sane results at each step —
    /// the acceptance bar from the session plan's step 2.5. Uses
    /// `search_mangas` (not `get_manga_list`) because that's the only
    /// discovery path the real app actually calls (`Backend.searchMangas`);
    /// `get_manga_list` exists solely to satisfy the `Source` trait shape.
    async fn assert_source_works_end_to_end(fixture_id: &str, main_js: &str) {
        let source = load_test_source(fixture_id, main_js);
        let token = CancellationToken::new();

        let (mangas, _has_next_page) = source
            .search_mangas(token.clone(), "the".to_string(), 1)
            .await
            .unwrap_or_else(|e| panic!("[{fixture_id}] search_mangas failed: {e:?}"));
        assert!(
            !mangas.is_empty(),
            "[{fixture_id}] search_mangas returned no manga"
        );
        let manga = &mangas[0];
        assert!(
            !manga.id.is_empty(),
            "[{fixture_id}] first manga has an empty id"
        );

        let chapters = source
            .get_chapter_list(token.clone(), manga.id.clone())
            .await
            .unwrap_or_else(|e| panic!("[{fixture_id}] get_chapter_list failed: {e:?}"));
        assert!(
            !chapters.is_empty(),
            "[{fixture_id}] get_chapter_list returned no chapters for manga {:?}",
            manga.id
        );
        let chapter = &chapters[0];

        let pages = source
            .get_page_list(
                token,
                manga.id.clone(),
                chapter.id.clone(),
                chapter.chapter_num,
            )
            .await
            .unwrap_or_else(|e| panic!("[{fixture_id}] get_page_list failed: {e:?}"));
        assert_eq!(
            pages.len(),
            1,
            "[{fixture_id}] expected exactly one Page (1 chapter = 1 Page strategy)"
        );
        assert!(
            pages[0].text.as_deref().is_some_and(|t| !t.is_empty()),
            "[{fixture_id}] page has no text content"
        );
    }

    #[tokio::test]
    #[ignore] // requires network
    async fn lnori_end_to_end() {
        assert_source_works_end_to_end("lnori", include_str!("test_fixtures/lnori.js")).await;
    }

    #[tokio::test]
    #[ignore] // requires network
    async fn novelupdates_end_to_end() {
        assert_source_works_end_to_end(
            "novelupdates",
            include_str!("test_fixtures/novelupdates.js"),
        )
        .await;
    }

    #[tokio::test]
    #[ignore] // requires network
    async fn novelbuddy_end_to_end() {
        assert_source_works_end_to_end("novelbuddy", include_str!("test_fixtures/novelbuddy.js"))
            .await;
    }

    #[tokio::test]
    #[ignore] // requires network
    async fn ranobes_end_to_end() {
        assert_source_works_end_to_end("ranobes", include_str!("test_fixtures/ranobes.js")).await;
    }

    #[tokio::test]
    #[ignore] // requires network
    async fn freewebnovel_end_to_end() {
        assert_source_works_end_to_end(
            "freewebnovel",
            include_str!("test_fixtures/freewebnovel.js"),
        )
        .await;
    }

    /// A hand-written synthetic plugin (not a vendored real source, same
    /// spirit as `tls.rs`'s plumbing-only network test) whose `searchNovels`
    /// busy-loops forever — simulates a worker that's alive but stuck, as
    /// opposed to `worker.rs`'s existing crash-path coverage (a worker that
    /// exits/dies, caught via a closed pipe). Exercises the other half of
    /// the persistent-worker design: a read timeout has to catch a hang
    /// too, not just a dead process (see `WORKER_READ_TIMEOUT`'s doc
    /// comment), and the source must recover via a fresh respawned worker
    /// on the next call rather than staying wedged forever.
    const HANG_PLUGIN_JS: &str = r#"
        Object.defineProperty(exports, "__esModule", { value: true });
        function HangPlugin() {
            this.id = "hangtest";
            this.name = "Hang Test";
            this.site = "https://example.com";
        }
        HangPlugin.prototype.searchNovels = function () {
            while (true) {}
        };
        HangPlugin.prototype.popularNovels = function () {
            return Promise.resolve([]);
        };
        HangPlugin.prototype.parseNovel = function () {
            return Promise.resolve({ path: "x" });
        };
        HangPlugin.prototype.parseChapter = function () {
            return Promise.resolve("<p></p>");
        };
        exports.default = new HangPlugin();
    "#;

    #[test]
    fn hung_worker_times_out_and_respawns() {
        let mut source = load_test_lnreader_source("hangtest", HANG_PLUGIN_JS);
        let token = CancellationToken::new();

        // Short timeout so this test doesn't take the real 120s
        // (`WORKER_READ_TIMEOUT`) to prove the hang path works.
        let err = source
            .run_worker_with_timeout(
                super::worker::Operation::SearchMangas {
                    query: "x".to_string(),
                    page: 1,
                },
                Duration::from_millis(500),
            )
            .expect_err("a busy-looping plugin should time out, not hang the test forever");
        assert!(
            err.to_string().contains("did not respond"),
            "expected a timeout error, got: {err}"
        );

        // Recovery check: a normal call right after, through the real
        // production path (the real `WORKER_READ_TIMEOUT`, not the short
        // test one), must succeed against a freshly respawned worker --
        // proving the timed-out one didn't wedge the source permanently.
        // Deliberately NOT `search_mangas` again: that maps to the same
        // busy-looping `searchNovels`, which would just hang a second time
        // and prove nothing about recovery. `get_chapter_list` maps to
        // `parseNovel`, a distinct method that resolves immediately in
        // `HangPlugin`.
        let chapters = source
            .get_chapter_list(token, "x".to_string())
            .expect("a call after a timed-out one should succeed via a fresh respawned worker");
        assert!(
            chapters.is_empty(),
            "HangPlugin's parseNovel() has no `chapters` field"
        );
    }
}
