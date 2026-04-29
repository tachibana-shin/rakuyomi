mod job;
mod manga;
mod model;
mod playlists;
mod settings;
mod source;
mod source_extractor;
mod state;
mod update;

use anyhow::Context;
use log::{error, info, warn};
use state::State;
use std::collections::HashMap;
use std::env::current_exe;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use serde::Serialize;
use shared::chapter_storage::ChapterStorage;
use shared::database::Database;
use shared::settings::Settings;
use shared::source_manager::SourceManager;
use shared::usecases::{
    fetch_manga_chapter::Error as FetchMangaChaptersError,
    search_mangas::Error as SearchMangasError,
};
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
struct Args {
    home_path: PathBuf,
}

const SOCKET_PATH: &str = "/tmp/rakuyomi.sock";

const DEFAULT_SETTINGS_JSON: &str = include_str!("../assets/default-settings.json");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .init();

    info!(
        "starting rakuyomi, version: {}",
        get_build_info()
            .map(|info| info.format_display())
            .unwrap_or_else(|| "unknown".into())
    );

    let args = Args::parse();
    fs::create_dir_all(&args.home_path)
        .context("while trying to ensure rakuyomi's home folder exists")?;

    let sources_path = args.home_path.join("sources");
    let database_path = args.home_path.join("database.db");
    let default_downloads_folder_path = args.home_path.join("downloads");
    let settings_path = args.home_path.join("settings.json");

    let database = Database::new(&database_path)
        .await
        .context("couldn't open database file")?;
    seed_default_settings(&settings_path)
        .with_context(|| format!("seeding default settings at {}", settings_path.display()))?;
    let settings = Settings::from_file(&settings_path)
        .with_context(|| format!("couldn't read settings file at {}", settings_path.display()))?;
    let source_manager = SourceManager::from_folder(sources_path, settings.clone())
        .context("couldn't create source manager")?;

    let downloads_folder_path = settings
        .storage_path
        .clone()
        .unwrap_or(default_downloads_folder_path);

    let chapter_storage = ChapterStorage::new(downloads_folder_path, settings.storage_size_limit.0)
        .context("couldn't initialize chapter storage")?;

    if settings.enabled_cron_check_mangas_update {
        let db_clone = database.clone();
        let sm_clone = source_manager.clone();
        let cs_clone = chapter_storage.clone();
        let settings = settings.clone();

        tokio::spawn(async move {
            shared::usecases::run_manga_cron(&db_clone, &cs_clone, &sm_clone, &settings).await;
        });
    }

    let state = State {
        source_manager: Arc::new(Mutex::new(source_manager)),
        database: Arc::new(Mutex::new(database)),
        chapter_storage: Arc::new(Mutex::new(chapter_storage)),
        settings: Arc::new(Mutex::new(settings)),
        settings_path,
        job_state: Default::default(),
        cancel_token_store: Arc::new(Mutex::new(HashMap::new())),
    };

    {
        let mut source_manager = state.source_manager.lock().await;
        source_manager.sources_by_id = source_manager
            .load_all_sources(&state.source_manager)
            .context("couldn't load sources")?;
    }

    let app = Router::new()
        .route("/health-check", get(health_check))
        .merge(manga::routes())
        .merge(playlists::routes())
        .merge(job::routes())
        .merge(settings::routes())
        .merge(source::routes())
        .merge(update::routes())
        .with_state(state);

    // run our app with hyper, listening on the domain socket
    let _ = std::fs::remove_file(SOCKET_PATH)
        .inspect_err(|e| warn!("could not remove existing socket path: {}", e));
    let listener = tokio::net::UnixListener::bind(SOCKET_PATH).unwrap();

    axum::serve(listener, app).await?;

    Ok(())
}

// Atomically creates `settings_path` with the default JSON if it doesn't already
// exist. Writes to a sibling temp file, restricts it to 0600 (settings.json may
// later contain credentials), then renames into place. A concurrent first-run
// that wins the rename is treated as success.
fn seed_default_settings(settings_path: &Path) -> anyhow::Result<()> {
    if settings_path.exists() {
        return Ok(());
    }
    info!(
        "settings file not found at {}, creating default",
        settings_path.display()
    );

    let parent = settings_path
        .parent()
        .expect("settings_path is built by joining onto home_path, so it always has a parent");
    let mut tmp = tempfile::Builder::new()
        .prefix(".settings-")
        .suffix(".tmp")
        .tempfile_in(parent)?;

    tmp.write_all(DEFAULT_SETTINGS_JSON.as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort: filesystems without POSIX permissions (FAT on Kindle's
        // /mnt/us, exFAT, …) reject the chmod. Log and move on rather than
        // failing startup — defaults are world-readable on those FSes anyway.
        if let Err(e) = tmp
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
        {
            warn!(
                "couldn't restrict permissions on default settings file: {} \
                (filesystem likely doesn't support POSIX modes)",
                e
            );
        }
    }

    tmp.as_file().sync_all()?;

    // We already returned early when `settings_path` existed, so a plain
    // atomic rename is enough. (Avoids `persist_noclobber`, which is
    // implemented via `hard_link` + `unlink` and fails with EPERM on
    // filesystems without hard-link support, e.g. FAT on Kindle's /mnt/us.)
    tmp.persist(settings_path).map_err(|e| e.error)?;
    Ok(())
}

async fn health_check() -> Json<()> {
    Json(())
}

#[derive(serde::Deserialize, Debug, Clone)]
struct BuildInfo {
    version: String,
    build: String,
}

impl BuildInfo {
    fn format_display(&self) -> String {
        format!("{} ({})", self.version, self.build)
    }
}

fn get_build_info() -> Option<BuildInfo> {
    let build_info_path = current_exe().ok()?.with_file_name("BUILD_INFO.json");
    let contents = fs::read_to_string(build_info_path).ok()?;
    let build_info: BuildInfo = serde_json::from_str(&contents).ok()?;

    Some(build_info)
}

// Make our own error that wraps `anyhow::Error`.
pub enum AppError {
    SourceNotFound,
    NotFound,
    DownloadAllChaptersProgressNotFound,
    NetworkFailure(anyhow::Error),
    Other(anyhow::Error),
}

#[derive(Serialize, Clone)]
pub struct ErrorResponse {
    message: String,
}

impl AppError {
    fn from_search_mangas_error(value: SearchMangasError) -> Self {
        match value {
            SearchMangasError::SourceError(e) => Self::NetworkFailure(e),
        }
    }

    fn from_fetch_manga_chapters_error(value: FetchMangaChaptersError) -> Self {
        match value {
            FetchMangaChaptersError::DownloadError(e) => Self::NetworkFailure(e),
            FetchMangaChaptersError::Other(e) => Self::Other(e),
        }
    }
}

impl From<&AppError> for StatusCode {
    fn from(value: &AppError) -> Self {
        match &value {
            AppError::SourceNotFound
            | AppError::NotFound
            | AppError::DownloadAllChaptersProgressNotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<&AppError> for ErrorResponse {
    fn from(value: &AppError) -> Self {
        let message = match value {
            AppError::SourceNotFound => "Source was not found".to_string(),
            AppError::NotFound => "Requested item was not found".to_string(),
            AppError::DownloadAllChaptersProgressNotFound => {
                "No download is in progress.".to_string()
            }
            AppError::NetworkFailure(_) => {
                "There was a network error. Check your connection and try again.".to_string()
            }
            AppError::Other(ref e) => {
                eprintln!("Unexpected error: {:?}", e);

                format!("Something went wrong: {}", e)
            }
        };

        Self { message }
    }
}

impl From<AppError> for ErrorResponse {
    fn from(value: AppError) -> Self {
        Self::from(&value)
    }
}

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status_code = StatusCode::from(&self);
        let error_response = ErrorResponse::from(&self);

        let inner_exception = match self {
            Self::NetworkFailure(ref e) => Some(e),
            Self::Other(ref e) => Some(e),
            _ => None,
        };

        if let Some(e) = inner_exception {
            error!("Error caused by: {:?}", e);
        }

        (status_code, Json(error_response)).into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, AppError>`. That way you don't need to do that manually.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self::Other(err.into())
    }
}
