use crate::model::Manga;
use axum::extract::{Path, State as StateExtractor};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use shared::model::MangaId;
use shared::usecases;

use crate::state::State;
use crate::AppError;

pub fn routes() -> Router<State> {
    Router::new()
        .route("/playlists", get(get_playlists))
        .route("/playlists", post(create_playlist))
        .route("/playlists/{id}", delete(delete_playlist))
        .route("/playlists/{id}", put(rename_playlist))
        .route("/playlists/{id}/mangas", get(get_mangas_in_playlist))
        .route("/playlists/{id}/mangas", post(add_manga_to_playlist))
        .route(
            "/playlists/{id}/mangas/{source_id}/{manga_id}",
            delete(remove_manga_from_playlist),
        )
}

async fn get_playlists(
    StateExtractor(State { database, .. }): StateExtractor<State>,
) -> Result<Json<Vec<shared::model::Playlist>>, AppError> {
    let database = database.lock().await;
    let playlists = usecases::get_playlists(&database).await?;

    Ok(Json(playlists))
}

#[derive(Deserialize)]
pub struct CreatePlaylistBody {
    pub name: String,
}

async fn create_playlist(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Json(body): Json<CreatePlaylistBody>,
) -> Result<Json<shared::model::Playlist>, AppError> {
    let database = database.lock().await;
    let playlist = usecases::create_playlist(&database, body.name).await?;

    Ok(Json(playlist))
}

#[derive(Deserialize)]
pub struct PlaylistIdPath {
    pub id: i64,
}

async fn delete_playlist(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<PlaylistIdPath>,
) -> Result<Json<()>, AppError> {
    let database = database.lock().await;
    usecases::delete_playlist(&database, params.id).await?;

    Ok(Json(()))
}

async fn rename_playlist(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<PlaylistIdPath>,
    Json(body): Json<CreatePlaylistBody>,
) -> Result<Json<()>, AppError> {
    let database = database.lock().await;
    usecases::rename_playlist(&database, params.id, body.name).await?;

    Ok(Json(()))
}

async fn get_mangas_in_playlist(
    StateExtractor(State {
        database,
        source_manager,
        settings,
        chapter_storage,
        ..
    }): StateExtractor<State>,
    Path(params): Path<PlaylistIdPath>,
) -> Result<Json<Vec<Manga>>, AppError> {
    let settings = settings.lock().await;
    let database = database.lock().await;
    let chapter_storage = chapter_storage.lock().await;
    let library_sorting_mode = &settings.library_sorting_mode;

    let mut mangas = usecases::get_mangas_in_playlist(
        &database,
        params.id,
        &*source_manager.lock().await,
        library_sorting_mode,
    )
    .await?;

    if settings.library_view_mode != shared::settings::LibraryViewMode::Base {
        for manga in mangas.iter_mut() {
            if manga.information.cover_url.is_some() {
                manga.information.cover_url = if let Some(path) =
                    chapter_storage.poster_exists(&manga.information.id)
                {
                    match url::Url::from_file_path(&path) {
                        Ok(url) => Some(url),
                        Err(_) => match path.canonicalize() {
                            Ok(canonical_path) => {
                                url::Url::from_file_path(canonical_path).ok()
                            }
                            Err(e) => {
                                println!("Error canonicalizing path: {}", e);
                                None
                            }
                        }
                    }
                } else {
                    None
                };
            }
        }
    }

    Ok(Json(
        mangas.into_iter().map(Manga::from).collect::<Vec<_>>(),
    ))
}

#[derive(Deserialize)]
pub struct AddMangaToPlaylistBody {
    pub source_id: String,
    pub manga_id: String,
}

async fn add_manga_to_playlist(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<PlaylistIdPath>,
    Json(body): Json<AddMangaToPlaylistBody>,
) -> Result<Json<()>, AppError> {
    let database = database.lock().await;
    let manga_id = MangaId::from_strings(body.source_id, body.manga_id);

    usecases::add_manga_to_playlist(&database, params.id, manga_id).await?;

    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct RemoveMangaFromPlaylistPath {
    pub id: i64,
    pub source_id: String,
    pub manga_id: String,
}

async fn remove_manga_from_playlist(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<RemoveMangaFromPlaylistPath>,
) -> Result<Json<()>, AppError> {
    let database = database.lock().await;
    let manga_id = MangaId::from_strings(params.source_id, params.manga_id);

    usecases::remove_manga_from_playlist(&database, params.id, manga_id).await?;

    Ok(Json(()))
}
