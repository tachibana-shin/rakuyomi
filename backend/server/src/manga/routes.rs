use std::time::Duration;

use axum::extract::{Path, Query, State as StateExtractor};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures::Future;
use log::warn;
use serde::{Deserialize, Serialize};
use shared::model::{ChapterId, MangaId, NotificationInformation};
use shared::usecases;
use tokio_util::sync::CancellationToken;

use crate::model::{Chapter, Manga};
use crate::source_extractor::SourceExtractor;
use crate::state::State;
use crate::AppError;

pub fn routes() -> Router<State> {
    Router::new()
        .route("/library", get(get_manga_library))
        .route("/find-orphan-or-read-files", get(find_orphan_or_read_files))
        .route("/delete-file", post(delete_file))
        .route("/sync-database", post(sync_database))
        .route("/check-mangas-update", post(check_mangas_update))
        .route("/count-notifications", get(get_count_notifications))
        .route("/notifications", get(get_notifications))
        .route("/notifications/{id}", delete(delete_notification))
        .route("/clear-notifications", post(clear_notifications))
        .route("/mangas", get(get_mangas))
        .route("/cancel-request", post(post_cancel_request))
        .route(
            "/mangas/{source_id}/{manga_id}/add-to-library",
            post(add_manga_to_library),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/remove-from-library",
            post(remove_manga_from_library),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/chapters",
            get(get_cached_manga_chapters),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/refresh-chapters",
            post(refresh_manga_chapters),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/details",
            get(get_cached_manga_details),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/refresh-details",
            post(refresh_manga_details),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/mark-as-read",
            post(mark_chapters_as_read),
        )
        // FIXME i dont think the route should be named download because it doesnt
        // always download the file...
        .route(
            "/mangas/{source_id}/{manga_id}/chapters/{chapter_id}/download",
            post(download_manga_chapter),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/chapters/{chapter_id}/mark-as-read",
            post(mark_chapter_as_read),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/chapters/{chapter_id}/update-last-read",
            post(update_last_read),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/preferred-scanlator",
            get(get_manga_preferred_scanlator),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/preferred-scanlator",
            post(set_manga_preferred_scanlator),
        )
}

async fn get_manga_library(
    StateExtractor(State {
        database,
        source_manager,
        settings,
        ..
    }): StateExtractor<State>,
) -> Result<Json<Vec<Manga>>, AppError> {
    let settings = settings.lock().await;
    let database = database.lock().await;
    let library_sorting_mode = &settings.library_sorting_mode;

    let mangas = usecases::get_manga_library(
        &database,
        &*source_manager.lock().await,
        library_sorting_mode,
    )
    .await?
    .into_iter()
    .map(Manga::from)
    .collect::<Vec<_>>();

    Ok(Json(mangas))
}

async fn find_orphan_or_read_files(
    StateExtractor(State {
        database,
        chapter_storage,
        ..
    }): StateExtractor<State>,
    Query(GetCleanerQuery { invalid }): Query<GetCleanerQuery>,
) -> Result<Json<FileSummary>, AppError> {
    let chapter_storage = chapter_storage.lock().await;
    let database = database.lock().await;

    let paths =
        usecases::find_orphan_or_read_files(&database, &chapter_storage, invalid == "true").await?;

    let filenames: Vec<String> = paths
        .iter()
        .filter_map(|p| p.file_name()?.to_str().map(|s| s.to_string()))
        .collect();

    let mut total_size = 0u64;
    for p in paths {
        if let Ok(meta) = tokio::fs::metadata(p).await {
            total_size += meta.len();
        }
    }

    let total_text = humansize::format_size(total_size, humansize::DECIMAL);

    let summary = FileSummary {
        filenames,
        total_size,
        total_text,
    };

    Ok(Json(summary))
}

async fn delete_file(
    StateExtractor(State {
        chapter_storage, ..
    }): StateExtractor<State>,
    Json(filename): Json<String>,
) -> Result<Json<()>, AppError> {
    let chapter_storage = chapter_storage.lock().await;

    let _ = chapter_storage.delete_filename(filename).await;

    Ok(Json(()))
}
async fn sync_database(
    StateExtractor(State {
        database, settings, ..
    }): StateExtractor<State>,
    Json(args): Json<Vec<bool>>,
) -> Result<Json<usecases::sync_database::SyncResult>, AppError> {
    let accept_migrate_local = args.get(0).cloned().unwrap_or(false);
    let accept_replace_remote = args.get(1).cloned().unwrap_or(false);

    let mut settings = settings.lock().await;
    let mut database = database.lock().await;

    let state = usecases::sync_database(
        &mut database,
        &mut settings,
        accept_migrate_local,
        accept_replace_remote,
    )
    .await?;

    Ok(Json(state))
}

#[derive(Deserialize)]
struct GetCheckMangasUpdate {
    cancel_id: Option<usize>,
}
async fn check_mangas_update(
    StateExtractor(State {
        database,
        chapter_storage,
        source_manager,
        cancel_token_store,
        ..
    }): StateExtractor<State>,
    Query(GetCheckMangasUpdate { cancel_id }): Query<GetCheckMangasUpdate>,
) -> Result<Json<()>, AppError> {
    let database = database.lock().await;
    let chapter_storage = chapter_storage.lock().await;
    let source_manager = source_manager.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let _ =
        usecases::check_mangas_update(&token, &database, &chapter_storage, &source_manager).await;

    Ok(Json(()))
}

#[derive(Deserialize)]
struct GetCleanerQuery {
    invalid: String,
}

#[derive(Serialize)]
struct FileSummary {
    filenames: Vec<String>,
    total_size: u64,
    total_text: String,
}

#[derive(Deserialize)]
struct GetMangasQuery {
    cancel_id: Option<usize>,
    q: String,
}

async fn get_mangas(
    StateExtractor(State {
        database,
        source_manager,
        cancel_token_store,
        ..
    }): StateExtractor<State>,
    Query(GetMangasQuery { cancel_id, q }): Query<GetMangasQuery>,
) -> Result<Json<Vec<Manga>>, AppError> {
    let source_manager = &*source_manager.lock().await;
    let database = database.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let results = cancel_after(&token, Duration::from_secs(120), |token| {
        usecases::search_mangas(source_manager, &database, token, q)
    })
    .await
    .map_err(AppError::from_search_mangas_error)?
    .into_iter()
    .map(Manga::from)
    .collect();

    Ok(Json(results))
}

async fn post_cancel_request(
    StateExtractor(State {
        cancel_token_store, ..
    }): StateExtractor<State>,
    Json(cancel_id): Json<usize>,
) -> Result<Json<()>, AppError> {
   let token = {
        let store = cancel_token_store.lock().await;
        store.get(&cancel_id).cloned()
    };

    if let Some(token) = token {
        if !token.is_cancelled() {
            token.cancel();
        }

        cancel_token_store.lock().await.remove(&cancel_id);
    }

    Ok(Json(()))
}

#[derive(Deserialize)]
struct NotificationParams {
    id: i32,
}

#[derive(Deserialize)]
struct MangaChaptersPathParams {
    source_id: String,
    manga_id: String,
}

#[derive(Deserialize)]
struct MangaMarkChaptersAsRead {
    range: String,
    state: bool,
}

impl From<MangaChaptersPathParams> for MangaId {
    fn from(value: MangaChaptersPathParams) -> Self {
        MangaId::from_strings(value.source_id, value.manga_id)
    }
}

async fn add_manga_to_library(
    StateExtractor(State {
        database,
        chapter_storage,
        source_manager,
        settings,
        ..
    }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);

    let settings = settings.lock().await;

    let database = database.lock().await;

    usecases::add_manga_to_library(&database, manga_id).await?;

    if settings.enabled_cron_check_mangas_update {
        let db = database.clone();
        let cs = chapter_storage.lock().await.clone();
        let sm = source_manager.lock().await.clone();
        let settings = settings.clone();

        tokio::spawn(async move {
            shared::usecases::run_manga_cron(&db, &cs, &sm, &settings).await;
        });
    }

    Ok(Json(()))
}

async fn get_count_notifications(
    StateExtractor(State { database, .. }): StateExtractor<State>,
) -> Result<Json<i32>, AppError> {
    let database = database.lock().await;

    let count = usecases::get_count_notifications(&database).await?;

    Ok(Json(count))
}

async fn get_notifications(
    StateExtractor(State {
        database,
        chapter_storage,
        ..
    }): StateExtractor<State>,
) -> Result<Json<Vec<NotificationInformation>>, AppError> {
    let database = database.lock().await;
    let chapter_storage = chapter_storage.lock().await;

    let rows = usecases::get_notifications(&database, &chapter_storage).await?;

    Ok(Json(rows))
}

async fn delete_notification(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<NotificationParams>,
) -> Result<Json<()>, AppError> {
    let database = database.lock().await;

    let _ = usecases::delete_notification(&database, params.id).await?;

    Ok(Json(()))
}

async fn clear_notifications(
    StateExtractor(State { database, .. }): StateExtractor<State>,
) -> Result<Json<()>, AppError> {
    let database = database.lock().await;

    let _ = usecases::clear_notifications(&database).await?;

    Ok(Json(()))
}

async fn remove_manga_from_library(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);
    let database = database.lock().await;

    usecases::remove_manga_from_library(&database, manga_id).await?;

    Ok(Json(()))
}

async fn get_cached_manga_chapters(
    StateExtractor(State {
        database,
        chapter_storage,
        ..
    }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<Vec<Chapter>>, AppError> {
    let manga_id = MangaId::from(params);
    let chapter_storage = &*chapter_storage.lock().await;
    let database = database.lock().await;
    let chapters =
        usecases::get_cached_manga_chapters(&database, chapter_storage, &manga_id).await?;

    let chapters = chapters.into_iter().map(Chapter::from).collect();

    Ok(Json(chapters))
}

async fn refresh_manga_chapters(
    StateExtractor(State {
        database,
        cancel_token_store,
        ..
    }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
    Json(cancel_id): Json<Option<usize>>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);
    let database = database.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let _ = usecases::refresh_manga_chapters(&token, &database, &source, &manga_id, 60).await;

    Ok(Json(()))
}

async fn get_cached_manga_details(
    StateExtractor(State {
        database,
        chapter_storage,
        ..
    }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<(shared::source::model::Manga, f64)>, AppError> {
    let manga_id = MangaId::from(params);
    let chapter_storage = &*chapter_storage.lock().await;
    let database = database.lock().await;

    let manga =
        usecases::get_cached_manga_details(&database, chapter_storage, &source, manga_id).await?;

    if let Some(manga) = manga {
        Ok(Json(manga))
    } else {
        Err(AppError::NotFound)
    }
}

async fn refresh_manga_details(
    StateExtractor(State {
        database,
        chapter_storage,
        cancel_token_store,
        ..
    }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
    Json(cancel_id): Json<Option<usize>>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);
    let database = database.lock().await;
    let chapter_storage = &*chapter_storage.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let _ = usecases::refresh_manga_details(
        &token,
        &database,
        &chapter_storage,
        &source,
        &manga_id,
        60,
    )
    .await;

    Ok(Json(()))
}

async fn mark_chapters_as_read(
    StateExtractor(State {
        database,
        chapter_storage,
        ..
    }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
    Json(MangaMarkChaptersAsRead { range, state }): Json<MangaMarkChaptersAsRead>,
) -> Result<Json<Option<usize>>, AppError> {
    let manga_id = MangaId::from(params);
    let database = database.lock().await;
    let chapter_storage = &*chapter_storage.lock().await;

    let count =
        usecases::mark_chapters_as_read(&database, &chapter_storage, manga_id, &range, state)
            .await?;

    Ok(Json(count))
}

#[derive(Deserialize)]
struct DownloadMangaChapterParams {
    source_id: String,
    manga_id: String,
    chapter_id: String,
}

impl From<DownloadMangaChapterParams> for ChapterId {
    fn from(value: DownloadMangaChapterParams) -> Self {
        ChapterId::from_strings(value.source_id, value.manga_id, value.chapter_id)
    }
}

async fn download_manga_chapter(
    StateExtractor(State {
        database,
        chapter_storage,
        settings,
        cancel_token_store,
        ..
    }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<DownloadMangaChapterParams>,
    Json(cancel_id): Json<Option<usize>>,
) -> Result<Json<String>, AppError> {
    let settings = settings.lock().await;
    let database = database.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let chapter_id = ChapterId::from(params);
    let chapter_storage = &*chapter_storage.lock().await;
    let concurrent_requests_pages = settings.concurrent_requests_pages.unwrap();
    let output_path = usecases::fetch_manga_chapter(
        &token,
        &database,
        &source,
        chapter_storage,
        &chapter_id,
        concurrent_requests_pages,
    )
    .await
    .map_err(AppError::from_fetch_manga_chapters_error)?;

    Ok(Json(output_path.to_string_lossy().into()))
}

async fn mark_chapter_as_read(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<DownloadMangaChapterParams>,
) -> Json<()> {
    let chapter_id = ChapterId::from(params);
    let database = database.lock().await;

    usecases::mark_chapter_as_read(&database, &chapter_id).await;

    Json(())
}

async fn update_last_read(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<DownloadMangaChapterParams>,
) -> Json<()> {
    let chapter_id = ChapterId::from(params);
    let database = database.lock().await;

    usecases::update_last_read_chapter(&database, &chapter_id).await;

    Json(())
}

// Scanlator preference handlers
#[derive(Deserialize)]
struct SetPreferredScanlatorBody {
    preferred_scanlator: Option<String>,
}

async fn get_manga_preferred_scanlator(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<Option<String>>, AppError> {
    let manga_id = MangaId::from(params);
    let database = database.lock().await;

    let preferred_scanlator = usecases::get_manga_preferred_scanlator(&database, &manga_id).await?;

    Ok(Json(preferred_scanlator))
}

async fn set_manga_preferred_scanlator(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
    Json(body): Json<SetPreferredScanlatorBody>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);
    let database = database.lock().await;

    usecases::set_manga_preferred_scanlator(&database, manga_id, body.preferred_scanlator).await?;

    Ok(Json(()))
}

async fn create_token(
    cancel_token_store: std::sync::Arc<
        tokio::sync::Mutex<std::collections::HashMap<usize, CancellationToken>>,
    >,
    cancel_id: Option<usize>,
) -> CancellationToken {
    let token = CancellationToken::new();

    if let Some(cancel_id) = cancel_id {
        {
            let mut store = cancel_token_store.lock().await;
            let old = store.insert(cancel_id, token.clone());
            if old.is_some() {
                warn!("cancel token already in use: {}", cancel_id);
            }
        }

        let store_clone = cancel_token_store.clone();
        let token_clone = token.clone();

        tokio::spawn(async move {
            token_clone.cancelled().await;

            let mut store = store_clone.lock().await;
            store.remove(&cancel_id);
        });
    }

    token
}

async fn cancel_after<F, Fut>(token: &CancellationToken, duration: Duration, f: F) -> Fut::Output
where
    Fut: Future,
    F: FnOnce(CancellationToken) -> Fut + Send,
{
    let future = f(token.clone());

    let token = token.clone();
    let request_cancellation_handle = tokio::spawn(async move {
        tokio::time::sleep(duration).await;

        warn!("cancellation requested!");
        token.cancel();
    });

    let result = future.await;

    request_cancellation_handle.abort();

    result
}
