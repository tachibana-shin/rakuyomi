use std::collections::HashMap;

use axum::extract::{Path, State as StateExtractor};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use shared::model::SourceId;
use shared::resource_usage::SourceUsage;
use shared::settings::SourceSettingValue;
use shared::source::model::SettingDefinition;
use shared::source::SourceBackend;
use shared::usecases;

use crate::model::SourceInformation;
use crate::source_extractor::{SourceExtractor, SourceParams};
use crate::state::State;
use crate::AppError;

pub fn routes() -> Router<State> {
    Router::new()
        .route("/available-sources", get(list_available_sources))
        .route(
            "/available-sources/{source_id}/install",
            post(install_source),
        )
        .route("/installed-sources", get(list_installed_sources))
        .route("/installed-sources/{source_id}", delete(uninstall_source))
        .route(
            "/installed-sources/{source_id}/setting-definitions",
            get(get_source_setting_definitions),
        )
        .route(
            "/installed-sources/{source_id}/stored-settings",
            get(get_source_stored_settings),
        )
        .route(
            "/installed-sources/{source_id}/stored-settings",
            post(set_source_stored_settings),
        )
        .route("/installed-sources/usage", get(get_all_source_usage))
}

async fn list_available_sources(
    StateExtractor(State { settings, .. }): StateExtractor<State>,
) -> Result<Json<Vec<SourceInformation>>, AppError> {
    let source_lists = settings.lock().await.source_lists.clone();
    let available_sources = usecases::list_available_sources(source_lists)
        .await?
        .into_iter()
        .map(SourceInformation::from)
        .collect();

    Ok(Json(available_sources))
}

#[derive(Deserialize)]
struct InstallSourceParams {
    source_id: String,
}

async fn install_source(
    StateExtractor(State {
        source_manager,
        settings,
        ..
    }): StateExtractor<State>,
    Path(InstallSourceParams { source_id }): Path<InstallSourceParams>,
    Json(source_of_source): Json<String>,
) -> Result<Json<()>, AppError> {
    usecases::install_source(
        &mut *source_manager.lock().await,
        &source_manager,
        &settings.lock().await.source_lists,
        SourceId::new(source_id),
        source_of_source,
    )
    .await?;

    Ok(Json(()))
}

async fn list_installed_sources(
    StateExtractor(State { source_manager, .. }): StateExtractor<State>,
) -> Json<Vec<SourceInformation>> {
    let installed_sources = usecases::list_installed_sources(&*source_manager.lock().await)
        .into_iter()
        .map(SourceInformation::from)
        .collect();

    Json(installed_sources)
}

async fn uninstall_source(
    StateExtractor(State { source_manager, .. }): StateExtractor<State>,
    Path(SourceParams { source_id }): Path<SourceParams>,
) -> Result<Json<()>, AppError> {
    usecases::uninstall_source(&mut *source_manager.lock().await, SourceId::new(source_id))?;

    Ok(Json(()))
}

async fn get_source_setting_definitions(
    SourceExtractor(source): SourceExtractor,
) -> Json<Vec<SettingDefinition>> {
    Json(usecases::get_source_setting_definitions(&source))
}

async fn get_source_stored_settings(
    StateExtractor(State { settings, .. }): StateExtractor<State>,
    Path(SourceParams { source_id }): Path<SourceParams>,
) -> Json<HashMap<String, SourceSettingValue>> {
    Json(usecases::get_source_stored_settings(
        &*settings.lock().await,
        &SourceId::new(source_id),
    ))
}

async fn set_source_stored_settings(
    StateExtractor(State {
        settings,
        settings_path,
        source_manager,
        ..
    }): StateExtractor<State>,
    Path(SourceParams { source_id }): Path<SourceParams>,
    Json(stored_settings): Json<HashMap<String, SourceSettingValue>>,
) -> Result<Json<()>, AppError> {
    usecases::set_source_stored_settings(
        &mut *settings.lock().await,
        &settings_path,
        &mut *source_manager.lock().await,
        &source_manager,
        &SourceId::new(source_id),
        stored_settings,
    )?;

    Ok(Json(()))
}

#[derive(serde::Serialize)]
struct SourceUsageResponse {
    #[serde(flatten)]
    usage: SourceUsage,
    disk_bytes: u64,
}

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// The on-disk files backing a source, without touching the filesystem.
fn source_files_of(
    source_manager: &shared::source_manager::SourceManager,
    source: &shared::source::Source,
) -> Vec<std::path::PathBuf> {
    let source_id = SourceId::new(source.manifest().info.id.clone());
    match &source.backend {
        SourceBackend::Aidoku(_) => vec![source_manager.source_path(&source_id)],
        SourceBackend::LnReader(_) => vec![
            source_manager.lnreader_source_path(&source_id),
            source_manager.lnreader_probe_path(&source_id),
        ],
        SourceBackend::Mangayomi(_) => vec![
            source_manager.mangayomi_source_path(&source_id),
            source_manager.mangayomi_js_source_path(&source_id),
            source_manager.mangayomi_probe_path(&source_id),
        ],
        SourceBackend::Keiyoushi(_) => vec![
            source_manager.keiyoushi_source_path(&source_id),
            source_manager.keiyoushi_probe_path(&source_id),
        ],
    }
}

fn disk_bytes_of(files: &[std::path::PathBuf]) -> u64 {
    files.iter().map(|p| file_size(p)).sum()
}

/// Returns the runtime usage of every installed source in one response,
/// keyed by source id. Polling this endpoint keeps the demand-driven VM
/// memory tracking alive (see [`ResourceRegistry::mark_active`]).
async fn get_all_source_usage(
    StateExtractor(State { source_manager, .. }): StateExtractor<State>,
) -> Json<HashMap<String, SourceUsageResponse>> {
    let source_manager = source_manager.lock().await;
    let entries = source_manager
        .sources_by_id
        .iter()
        .map(|(source_id, source)| {
            // Read first: the first poll of a reopened view discards any
            // stale memory data left from the previous session, then the
            // poll itself restarts the demand-driven tracking.
            let usage = source.usage.usage(source_id.value()).unwrap_or_default();
            source.usage.mark_active();
            let files = source_files_of(&source_manager, source);
            (source_id.value().to_string(), usage, files)
        })
        .collect::<Vec<_>>();
    drop(source_manager);

    let out = tokio::task::spawn_blocking(move || {
        entries
            .into_iter()
            .map(|(source_id, usage, files)| {
                (
                    source_id,
                    SourceUsageResponse {
                        usage,
                        disk_bytes: disk_bytes_of(&files),
                    },
                )
            })
            .collect::<HashMap<_, _>>()
    })
    .await
    .unwrap_or_default();

    Json(out)
}
