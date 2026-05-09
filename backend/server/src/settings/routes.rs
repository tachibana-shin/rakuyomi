use anyhow::Context;
use axum::extract::State as StateExtractor;
use axum::routing::{get, put};
use axum::{Json, Router};
use shared::usecases;
use shared::usecases::update_settings::UpdateableSettings;

use crate::state::State;
use crate::AppError;

pub fn routes() -> Router<State> {
    Router::new()
        .route("/settings", get(get_settings))
        .route("/settings", put(update_settings))
}

async fn get_settings(
    StateExtractor(State { settings, .. }): StateExtractor<State>,
) -> Json<UpdateableSettings> {
    Json(UpdateableSettings::from(&*settings.lock().await))
}

async fn update_settings(
    StateExtractor(State {
        chapter_storage,
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Json(updateable_settings): Json<UpdateableSettings>,
) -> Result<Json<UpdateableSettings>, AppError> {
    let mut chapter_storage = chapter_storage.lock().await;
    let mut settings = settings.lock().await;
    usecases::update_settings(&mut settings, &settings_path, updateable_settings)?;

    // Update the chapter storage for the new storage path
    if let Some(storage_path) = settings.storage_path.as_ref() {
        chapter_storage
            .set_downloads_folder_path(storage_path.clone())
            .with_context(|| {
                format!(
                    "Couldn't set the new storage path to {}",
                    storage_path.display()
                )
            })?;
    }

    Ok(Json(UpdateableSettings::from(&*settings)))
}
