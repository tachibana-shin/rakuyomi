use std::collections::HashMap;

use axum::extract::{Path, State as StateExtractor};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use shared::model::SourceId;
use shared::settings::SourceSettingValue;
use shared::source::model::SettingDefinition;
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
) -> Result<Json<()>, AppError> {
    usecases::install_source(
        &mut *source_manager.lock().await,
        &settings.lock().await.source_lists,
        SourceId::new(source_id),
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
        &SourceId::new(source_id),
        stored_settings,
    )?;

    Ok(Json(()))
}
