use anyhow::Context;
use axum::extract::State as StateExtractor;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use shared::usecases;
use shared::usecases::update_settings::UpdateableSettings;

use crate::state::State;
use crate::AppError;

pub fn routes() -> Router<State> {
    Router::new()
        .route("/settings", get(get_settings))
        .route("/settings", put(update_settings))
        .route("/settings/tracking/validate", post(validate_tracking_settings))
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
    let mut settings = settings.lock().await;
    usecases::update_settings(&mut settings, &settings_path, updateable_settings)?;

    // Update the chapter storage for the new storage path
    if let Some(storage_path) = settings.storage_path.as_ref() {
        chapter_storage
            .lock()
            .await
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
#[derive(serde::Deserialize)]
struct ValidateTrackingRequest {
    service: shared::model::TrackingService,
}

async fn validate_tracking_settings(
    StateExtractor(State { settings, settings_path, .. }): StateExtractor<State>,
    Json(body): Json<ValidateTrackingRequest>,
) -> Result<Json<()>, AppError> {
    let mut settings = settings.lock().await;
    usecases::validate_tracking_settings(&mut settings, body.service).await?;

    settings.save_to_file(&settings_path)?;

    Ok(Json(()))
}
