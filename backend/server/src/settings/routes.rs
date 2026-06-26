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
        .route("/settings/mount-tmpfs", post(mount_tmpfs))
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

#[derive(serde::Deserialize)]
struct MountTmpFSBody {
    enabled: bool,
    ram_storage_size_mb: usize,
}

async fn mount_tmpfs(
    StateExtractor(State {
        chapter_storage,
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Json(MountTmpFSBody {
        enabled,
        ram_storage_size_mb,
    }): Json<MountTmpFSBody>,
) -> Result<Json<()>, AppError> {
    let mut chapter_storage = chapter_storage.lock().await;
    let mut settings = settings.lock().await;

    if enabled {
        chapter_storage
            .enable_ram(ram_storage_size_mb)
            .map_err(AppError::MountTmpFs)?;

        let mut updated_settings = settings.clone();
        updated_settings.ram_storage_enabled = enabled;
        updated_settings.ram_storage_size_mb = ram_storage_size_mb;
        updated_settings.save_to_file(&settings_path)?;

        *settings = updated_settings;
    } else {
        chapter_storage.disable_ram();
    }

    Ok(Json(()))
}
