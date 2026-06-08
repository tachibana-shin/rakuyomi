use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use log::info;
use tokio::sync::Mutex;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[cfg(feature = "ffi")]
use axum::extract::Request;
#[cfg(feature = "ffi")]
use axum::middleware::{self, Next};
#[cfg(feature = "ffi")]
use axum::response::Response;
use axum::{routing::get, Json, Router};
use shared::chapter_storage::ChapterStorage;
use shared::database::Database;
use shared::settings::Settings;
use shared::source_manager::SourceManager;

use crate::build_info::{get_build_info, DEFAULT_SETTINGS_JSON};
use crate::listener::{pick_listener, ResolvedListener};
use crate::state::State;
use crate::{job, manga, playlists, settings, source, update};

/// Initialize logging. Safe to call multiple times; only the first invocation
/// actually installs the subscriber.
pub fn init_logging() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .try_init();
}

/// Build the full axum router with the given state. Exposed so both the
/// binary entry point and the JNI bridge can share it.
pub fn build_router(state: State) -> Router {
    let router = Router::new()
        .route("/health-check", get(health_check))
        .merge(manga::routes())
        .merge(playlists::routes())
        .merge(job::routes())
        .merge(settings::routes())
        .merge(source::routes())
        .merge(update::routes());
    #[cfg(feature = "ffi")]
    let router = router
        .layer(middleware::from_fn(request_logger))
        .with_state(state);
    #[cfg(not(feature = "ffi"))]
    let router = router.with_state(state);
    router
}

#[cfg(feature = "ffi")]
async fn request_logger(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = std::time::Instant::now();

    let response = next.run(req).await;

    let duration = start.elapsed().as_millis();
    let status = response.status().as_u16();

    #[cfg(all(target_os = "android", feature = "ffi"))]
    {
        // Format as CSV-like string for Kotlin to parse easily
        // method|path|status|duration
        let log_entry = format!("{}|{}|{}|{}", method, path, status, duration);
        crate::jni::push_log(log_entry);
    }

    response
}

async fn health_check() -> Json<()> {
    Json(())
}

/// Construct the [`State`] from a home path. This performs all the
/// filesystem bootstrapping (creating directories, writing default
/// settings, opening the database) and source loading.
pub async fn build_state(home_path: PathBuf) -> Result<State> {
    fs::create_dir_all(&home_path)
        .context("while trying to ensure rakuyomi's home folder exists")?;

    let sources_path = home_path.join("sources");
    let database_path = home_path.join("database.db");
    let default_downloads_folder_path = home_path.join("downloads");
    let settings_path = home_path.join("settings.json");

    let database = Database::new(&database_path)
        .await
        .context("couldn't open database file")?;
    if !settings_path.exists() {
        info!(
            "settings file not found at {}, creating default",
            settings_path.display()
        );
        fs::write(&settings_path, DEFAULT_SETTINGS_JSON).with_context(|| {
            format!(
                "couldn't write default settings file at {}",
                settings_path.display()
            )
        })?;
    }
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

    Ok(state)
}

/// Run the server on a transport chosen by [`pick_listener`].
///
/// This is the main entry point used by the standalone binary. Blocks
/// until the listener is closed or an error occurs.
pub async fn run(home_path: PathBuf) -> Result<()> {
    let listener = pick_listener().await?;
    let state = build_state(home_path).await?;
    let version = get_build_info()
        .map(|info| info.format_display())
        .unwrap_or_else(|| "unknown".into());
    info!("starting rakuyomi {} on {}", version, listener.describe());
    serve(listener, state, None).await
}

/// Serve the router on the given listener, optionally listening for a
/// shutdown signal. Returns when the server stops (either due to an
/// error or the shutdown signal firing).
pub async fn serve(
    listener: ResolvedListener,
    state: State,
    shutdown: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<()> {
    let app = build_router(state);
    let shutdown_fut = shutdown.map(|rx| async move {
        let _ = rx.await;
    });
    match (listener, shutdown_fut) {
        (ResolvedListener::Tcp(l), Some(s)) => {
            axum::serve(l, app).with_graceful_shutdown(s).await?
        }
        (ResolvedListener::Tcp(l), None) => axum::serve(l, app).await?,
        (ResolvedListener::Unix(l, _), Some(s)) => {
            axum::serve(l, app).with_graceful_shutdown(s).await?
        }
        (ResolvedListener::Unix(l, _), None) => axum::serve(l, app).await?,
    }
    Ok(())
}

/// Print the build info line if available.
pub fn log_startup() {
    init_logging();
    info!(
        "starting rakuyomi, version: {}",
        get_build_info()
            .map(|info| info.format_display())
            .unwrap_or_else(|| "unknown".into())
    );
}
