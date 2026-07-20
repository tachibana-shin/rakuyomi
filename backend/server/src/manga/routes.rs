use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use axum::extract::{Path, Query, State as StateExtractor};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use futures::Future;
use log::warn;
use serde::{Deserialize, Serialize};
use shared::model::{
    ChapterId, MangaId, NotificationInformation, TrackingCandidate, TrackingService,
    TrackingSyncDirection, TrackingSyncResult,
};
use shared::usecases;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::model::{Chapter, Manga};
use crate::source_extractor::SourceExtractor;
use crate::state::State;
use crate::AppError;

fn path_to_file_url(path: &std::path::Path) -> Option<url::Url> {
    match url::Url::from_file_path(&path) {
        Ok(url) => Some(url),
        Err(_) => match path.canonicalize() {
            Ok(canonical_path) => url::Url::from_file_path(canonical_path).ok(),
            Err(e) => {
                println!("Error canonicalizing path: {}", e);
                None
            }
        },
    }
}

pub fn routes() -> Router<State> {
    Router::new()
        .route("/library", get(get_manga_library))
        .route("/storage-stats", get(get_storage_stats))
        .route("/find-orphan-or-read-files", get(find_orphan_or_read_files))
        .route("/delete-file", post(delete_file))
        .route("/sync-database", post(sync_database))
        .route("/check-mangas-update", post(check_mangas_update))
        .route("/count-notifications", get(get_count_notifications))
        .route("/notifications", get(get_notifications))
        .route("/notifications/{id}", delete(delete_notification))
        .route("/clear-notifications", post(clear_notifications))
        .route(
            "/{source_id}/handle-source-notification/{key}",
            post(handle_source_notification),
        )
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
            "/mangas/{source_id}/{manga_id}/chapters/{chapter_id}/revoke",
            post(revoke_manga_chapter),
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
        .route(
            "/mangas/{source_id}/{manga_id}/tracking",
            get(list_tracking_bindings),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/tracking/search",
            post(search_tracking_candidates),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/tracking/link",
            post(link_tracking_binding),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/tracking/sync",
            post(sync_tracking_bindings),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/tracking/{service}",
            delete(unlink_tracking_binding),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/tracking/{service}/dates",
            patch(set_tracking_dates),
        )
        .route(
            "/mangas/{source_id}/{manga_id}/viewer",
            post(set_manga_viewer),
        )
}

async fn get_manga_library(
    StateExtractor(State {
        database,
        source_manager,
        settings,
        chapter_storage,
        ..
    }): StateExtractor<State>,
) -> Result<Json<Vec<Manga>>, AppError> {
    let chapter_storage = chapter_storage.lock().await;
    let settings = settings.lock().await;
    let source_manager = source_manager.lock().await;
    let library_sorting_mode = &settings.library_sorting_mode;

    let mut mangas =
        usecases::get_manga_library(&database, &*source_manager, library_sorting_mode).await?;

    if settings.library_view_mode != shared::settings::LibraryViewMode::Base {
        for manga in mangas.iter_mut() {
            if manga.information.cover_url.is_some() {
                manga.information.cover_url = chapter_storage
                    .poster_exists(&manga.information.id)
                    .and_then(|path| path_to_file_url(&path));
            }
        }
    }

    Ok(Json(
        mangas.into_iter().map(Manga::from).collect::<Vec<_>>(),
    ))
}

async fn get_storage_stats(
    StateExtractor(State {
        chapter_storage, ..
    }): StateExtractor<State>,
) -> Json<usecases::get_storage_stats::StorageStats> {
    let chapter_storage = chapter_storage.lock().await;

    Json(usecases::get_storage_stats(&chapter_storage))
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

    let _ = chapter_storage.delete_filename(filename, false).await;

    Ok(Json(()))
}
async fn sync_database(
    StateExtractor(State {
        database, settings, ..
    }): StateExtractor<State>,
    Json(args): Json<Vec<bool>>,
) -> Result<Json<usecases::sync_database::SyncResult>, AppError> {
    let accept_migrate_local = args.first().cloned().unwrap_or(false);
    let accept_replace_remote = args.get(1).cloned().unwrap_or(false);

    let mut settings = settings.lock().await;

    let state = usecases::sync_database(
        &*database,
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
    let chapter_storage = chapter_storage.lock().await;
    let source_manager = source_manager.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let _ = usecases::check_mangas_update(&token.0, &database, &chapter_storage, &*source_manager)
        .await;

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
    exclude: Option<String>,
    q: String,
    page: Option<i32>,
    sort: Option<String>,
}

async fn get_mangas(
    StateExtractor(State {
        database,
        source_manager,
        cancel_token_store,
        chapter_storage,
        settings,
        ..
    }): StateExtractor<State>,
    Query(GetMangasQuery {
        cancel_id,
        exclude,
        q,
        page,
        sort,
    }): Query<GetMangasQuery>,
) -> Result<Json<(Vec<Manga>, Vec<usecases::search_mangas::SearchError>, bool)>, AppError> {
    let chapter_storage = chapter_storage.lock().await;
    let settings = settings.lock().await;
    let source_manager = source_manager.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let exclude = exclude.map(|v| {
        v.split(",")
            .map(|x| x.trim().to_string())
            .collect::<Vec<_>>()
    });

    let page = page.unwrap_or(1).max(1);
    let sort_bucket = sort.and_then(|s| shared::source::SortBucket::try_from(s.as_str()).ok());

    let (mut mangas, errors, has_next_page) =
        cancel_after(&token.0, Duration::from_secs(59), |token| {
            usecases::search_mangas(
                &*source_manager,
                &database,
                &chapter_storage,
                &settings,
                token,
                q,
                &exclude,
                page,
                30,
                sort_bucket,
            )
        })
        .await
        .map_err(AppError::from_search_mangas_error)?;

    if settings.search_view_mode != shared::settings::SearchViewMode::Base {
        for manga in mangas.iter_mut() {
            if manga.information.cover_url.is_some() {
                manga.information.cover_url = chapter_storage
                    .poster_exists(&manga.information.id)
                    .and_then(|path| path_to_file_url(&path));
            }
        }
    }

    let results = mangas.into_iter().map(Manga::from).collect();

    Ok(Json((results, errors, has_next_page)))
}

async fn post_cancel_request(
    StateExtractor(State {
        cancel_token_store, ..
    }): StateExtractor<State>,
    Json(cancel_id): Json<usize>,
) -> Result<Json<()>, AppError> {
    let mut store = cancel_token_store.lock().await;
    if let Some(token) = store.remove(&cancel_id) {
        if !token.is_cancelled() {
            token.cancel();
        }
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
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);

    let token = tokio_util::sync::CancellationToken::new();

    usecases::add_manga_to_library(&token, &database, &source, manga_id, 30).await?;

    let settings = settings.lock().await;

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
    let chapter_storage = chapter_storage.lock().await;

    let rows = usecases::get_notifications(&database, &chapter_storage).await?;

    Ok(Json(rows))
}

async fn delete_notification(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<NotificationParams>,
) -> Result<Json<()>, AppError> {
    usecases::delete_notification(&database, params.id).await?;

    Ok(Json(()))
}

async fn clear_notifications(
    StateExtractor(State { database, .. }): StateExtractor<State>,
) -> Result<Json<()>, AppError> {
    usecases::clear_notifications(&database).await?;

    Ok(Json(()))
}

#[derive(Deserialize)]
struct HandleSourceNotificationParams {
    key: String,
}
async fn handle_source_notification(
    StateExtractor(State {
        cancel_token_store, ..
    }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<HandleSourceNotificationParams>,
    Query(GetCheckMangasUpdate { cancel_id }): Query<GetCheckMangasUpdate>,
) -> Result<Json<()>, AppError> {
    let token = create_token(cancel_token_store, cancel_id).await;

    cancel_after(&token.0, Duration::from_secs(120), |token| {
        source.handle_notification_next(token, params.key)
    })
    .await?;

    Ok(Json(()))
}

async fn remove_manga_from_library(
    StateExtractor(State {
        database,
        chapter_storage,
        settings,
        ..
    }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);
    // Copy the flag and release the settings lock before the long-running
    // removal work, so concurrent settings access isn't blocked.
    let delete_downloaded_on_remove = settings.lock().await.delete_downloaded_on_remove;
    let chapter_storage = chapter_storage.lock().await;

    usecases::remove_manga_from_library(
        &database,
        &chapter_storage,
        delete_downloaded_on_remove,
        manga_id,
    )
    .await?;

    Ok(Json(()))
}

async fn get_cached_manga_chapters(
    StateExtractor(State {
        database,
        chapter_storage,
        settings,
        ..
    }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<Vec<Chapter>>, AppError> {
    let manga_id = MangaId::from(params);

    let chapter_storage = &*chapter_storage.lock().await;
    let chapters = usecases::get_cached_manga_chapters(
        &database,
        chapter_storage,
        &manga_id,
        settings.lock().await.ram_storage_enabled,
    )
    .await?;

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

    let token = create_token(cancel_token_store, cancel_id).await;

    let _ = usecases::refresh_manga_chapters(&token.0, &database, &source, &manga_id, 60).await;

    Ok(Json(()))
}

async fn get_cached_manga_details(
    StateExtractor(State {
        database,
        chapter_storage,
        cancel_token_store,
        ..
    }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
    Query(GetCheckMangasUpdate { cancel_id }): Query<GetCheckMangasUpdate>,
) -> Result<Json<(shared::source::model::Manga, f64)>, AppError> {
    let manga_id = MangaId::from(params);

    let chapter_storage = &*chapter_storage.lock().await;

    let token = create_token(cancel_token_store, cancel_id).await;

    let manga =
        usecases::get_cached_manga_details(&token.0, &database, chapter_storage, &source, manga_id)
            .await?;

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

    let chapter_storage = &*chapter_storage.lock().await;
    let token = create_token(cancel_token_store, cancel_id).await;

    let _ = usecases::refresh_manga_details(
        &token.0,
        &database,
        chapter_storage,
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
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
    Json(MangaMarkChaptersAsRead { range, state }): Json<MangaMarkChaptersAsRead>,
) -> Result<Json<Option<usize>>, AppError> {
    let manga_id = MangaId::from(params);

    let (delete_downloaded_after_read, tracking_auto_sync) = {
        let settings = settings.lock().await;
        (
            settings.delete_downloaded_after_read,
            settings.tracking_auto_sync,
        )
    };
    let chapter_storage = &*chapter_storage.lock().await;

    let count = usecases::mark_chapters_as_read(
        &database,
        chapter_storage,
        delete_downloaded_after_read,
        &manga_id,
        &range,
        state,
    )
    .await?;
    if tracking_auto_sync {
        spawn_tracking_sync_after_local_update(database, settings, settings_path, manga_id);
    }

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

#[derive(Deserialize, Default)]
struct DownloadQuery {
    offline: Option<bool>,
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
    Query(query): Query<DownloadQuery>,
    Json(cancel_id): Json<Option<usize>>,
) -> Result<Json<(String, Vec<shared::chapter_downloader::DownloadError>)>, AppError> {
    let token = create_token(cancel_token_store, cancel_id).await;
    let (db, cs, use_ram, concurrent_requests_pages, optimize_image, chapter_title_format) = {
        let cs = chapter_storage.lock().await;
        let settings = settings.lock().await;
        (
            database.clone(),
            cs.clone(),
            !query.offline.unwrap_or_default() && settings.ram_storage_enabled,
            settings.concurrent_requests_pages.unwrap_or(4),
            settings.optimize_image,
            settings.chapter_title_format,
        )
    };

    let chapter_id = ChapterId::from(params);
    let output_path = usecases::fetch_manga_chapter(
        &token.0,
        &db,
        &source,
        &cs,
        &chapter_id,
        concurrent_requests_pages,
        optimize_image,
        None,
        use_ram,
        chapter_title_format,
    )
    .await
    .map_err(AppError::from_fetch_manga_chapters_error)?;

    Ok(Json((
        output_path.0.to_string_lossy().into(),
        output_path.1,
    )))
}

#[derive(Deserialize)]
struct RevokeMangaChapterQuery {
    use_ram: Option<bool>,
}
async fn revoke_manga_chapter(
    StateExtractor(State {
        chapter_storage, ..
    }): StateExtractor<State>,
    Path(params): Path<DownloadMangaChapterParams>,
    Query(query): Query<RevokeMangaChapterQuery>,
) -> Result<Json<bool>, AppError> {
    let chapter_id = ChapterId::from(params);
    let chapter_storage = &*chapter_storage.lock().await;

    let result = usecases::revoke_manga_chapter(
        chapter_storage,
        &chapter_id,
        query.use_ram.unwrap_or(false),
    )
    .await?;

    Ok(Json(result))
}

#[derive(Deserialize)]
struct MarkChapterAsReadBody {
    state: Option<bool>,
}
async fn mark_chapter_as_read(
    StateExtractor(State {
        database,
        settings,
        settings_path,
        chapter_storage,
        ..
    }): StateExtractor<State>,
    Path(params): Path<DownloadMangaChapterParams>,
    Json(MarkChapterAsReadBody { state }): Json<MarkChapterAsReadBody>,
) -> Result<Json<()>, AppError> {
    let chapter_id = ChapterId::from(params);

    let (delete_downloaded_after_read, tracking_auto_sync) = {
        let settings = settings.lock().await;
        (
            settings.delete_downloaded_after_read,
            settings.tracking_auto_sync,
        )
    };
    let chapter_storage = chapter_storage.lock().await;

    usecases::mark_chapter_as_read(
        &database,
        &chapter_storage,
        delete_downloaded_after_read,
        &chapter_id,
        state,
    )
    .await?;
    if tracking_auto_sync {
        spawn_tracking_sync_after_local_update(
            database,
            settings,
            settings_path,
            chapter_id.manga_id().clone(),
        );
    }

    Ok(Json(()))
}

async fn update_last_read(
    StateExtractor(State {
        database,
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Path(params): Path<DownloadMangaChapterParams>,
) -> Result<Json<()>, AppError> {
    let chapter_id = ChapterId::from(params);

    let tracking_auto_sync = {
        let settings = settings.lock().await;

        settings.tracking_auto_sync
    };

    usecases::update_last_read_chapter(&database, &chapter_id).await?;
    if tracking_auto_sync {
        spawn_tracking_sync_after_local_update(
            database,
            settings,
            settings_path,
            chapter_id.manga_id().clone(),
        );
    }

    Ok(Json(()))
}

// Scanlator preference handlers
#[derive(Deserialize)]
struct SetPreferredScanlatorBody {
    preferred_scanlator: Option<String>,
}

async fn get_manga_preferred_scanlator(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<Option<String>>, AppError> {
    let manga_id = MangaId::from(params);

    let preferred_scanlator = usecases::get_manga_preferred_scanlator(&database, &manga_id).await?;

    Ok(Json(preferred_scanlator))
}

async fn set_manga_preferred_scanlator(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
    Json(body): Json<SetPreferredScanlatorBody>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);

    usecases::set_manga_preferred_scanlator(&database, manga_id, body.preferred_scanlator).await?;

    Ok(Json(()))
}

async fn list_tracking_bindings(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<Vec<shared::model::TrackingBinding>>, AppError> {
    let manga_id = MangaId::from(params);

    Ok(Json(
        usecases::list_tracking_bindings(&database, &manga_id).await?,
    ))
}

#[derive(Deserialize)]
struct SearchTrackingCandidatesBody {
    service: TrackingService,
    query: String,
}

async fn search_tracking_candidates(
    StateExtractor(State {
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Json(body): Json<SearchTrackingCandidatesBody>,
) -> Result<Json<Vec<TrackingCandidate>>, AppError> {
    let mut settings = settings.lock().await;
    let results =
        usecases::search_tracking_candidates(&mut settings, body.service, &body.query).await?;

    settings.save_to_file(&settings_path)?;

    Ok(Json(results))
}

async fn link_tracking_binding(
    StateExtractor(State {
        database,
        chapter_storage,
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
    Json(candidate): Json<TrackingCandidate>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);

    usecases::link_tracking_binding(&database, &manga_id, &candidate).await?;

    // Automatically pull progress from the newly linked service.
    let mut settings = settings.lock().await;
    let chapter_storage = chapter_storage.lock().await;
    let _ = usecases::sync_manga_tracking(
        &database,
        &chapter_storage,
        &mut settings,
        &manga_id,
        Some(candidate.service),
        TrackingSyncDirection::Pull,
    )
    .await;
    let _ = settings.save_to_file(&settings_path);

    Ok(Json(()))
}

// Viewer preference handlers
#[derive(Deserialize)]
struct SetViewerBody {
    viewer: Option<i64>,
}

async fn set_manga_viewer(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
    Json(body): Json<SetViewerBody>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);

    if let Some(viewer) = body.viewer {
        if viewer < 0 || viewer > 4 {
            return Err(AppError::Other(anyhow::anyhow!(
                "Invalid viewer value: {}. Must be between 0 and 4.",
                viewer
            )));
        }
    }

    usecases::set_manga_viewer(&database, manga_id, body.viewer).await?;

    Ok(Json(()))
}

#[derive(Deserialize)]
struct SyncTrackingBindingsBody {
    service: Option<TrackingService>,
    direction: TrackingSyncDirection,
}

async fn sync_tracking_bindings(
    StateExtractor(State {
        database,
        chapter_storage,
        settings,
        settings_path,
        ..
    }): StateExtractor<State>,
    Path(params): Path<MangaChaptersPathParams>,
    Json(body): Json<SyncTrackingBindingsBody>,
) -> Result<Json<Vec<TrackingSyncResult>>, AppError> {
    let manga_id = MangaId::from(params);
    let mut settings = settings.lock().await;
    let chapter_storage = chapter_storage.lock().await;

    let results = usecases::sync_manga_tracking(
        &database,
        &chapter_storage,
        &mut settings,
        &manga_id,
        body.service,
        body.direction,
    )
    .await?;

    settings.save_to_file(&settings_path)?;

    Ok(Json(results))
}

#[derive(Deserialize)]
struct TrackingBindingPathParams {
    source_id: String,
    manga_id: String,
    service: String,
}

async fn unlink_tracking_binding(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<TrackingBindingPathParams>,
) -> Result<Json<()>, AppError> {
    let service = TrackingService::try_from(params.service.as_str())?;
    let manga_id = MangaId::from_strings(params.source_id, params.manga_id);

    usecases::unlink_tracking_binding(&database, &manga_id, service).await?;

    Ok(Json(()))
}

#[derive(Deserialize)]
struct SetTrackingDatesBody {
    started_at: Option<i64>,
    completed_at: Option<i64>,
}

async fn set_tracking_dates(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    Path(params): Path<TrackingBindingPathParams>,
    Json(body): Json<SetTrackingDatesBody>,
) -> Result<Json<()>, AppError> {
    let service = TrackingService::try_from(params.service.as_str())?;
    let manga_id = MangaId::from_strings(params.source_id, params.manga_id);

    database
        .set_tracking_dates(&manga_id, service, body.started_at, body.completed_at)
        .await?;

    Ok(Json(()))
}

static TRACKING_SYNC_TOKENS: LazyLock<Arc<Mutex<HashMap<String, CancellationToken>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

fn spawn_tracking_sync_after_local_update(
    database: Arc<shared::database::Database>,
    settings: Arc<Mutex<shared::settings::Settings>>,
    settings_path: std::path::PathBuf,
    manga_id: MangaId,
) {
    let manga_key = format!("{}:{}", manga_id.source_id().value(), manga_id.value());

    tokio::spawn(async move {
        // Cancel any previous pending sync for this manga
        let tokens = TRACKING_SYNC_TOKENS.clone();
        let mut map = tokens.lock().await;
        if let Some(old_token) = map.get(&manga_key) {
            old_token.cancel();
        }
        let token = CancellationToken::new();
        map.insert(manga_key.clone(), token.clone());
        drop(map);

        // Small delay to debounce rapid successive calls
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_millis(500)) => {},
        }

        let mut settings = settings.lock().await;
        if !settings.tracking_auto_sync {
            let mut map = tokens.lock().await;
            map.remove(&manga_key);
            return;
        }

        if let Err(err) =
            usecases::sync_manga_tracking_push(&database, &mut settings, &manga_id).await
        {
            warn!(
                "tracking auto-sync failed for {:?}: {err:#}",
                manga_id.value()
            );
        } else {
            let _ = settings.save_to_file(&settings_path);
        }

        let mut map = tokens.lock().await;
        map.remove(&manga_key);
    });
}

type CancelTokenStore =
    std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<usize, CancellationToken>>>;
struct TokenGuard(CancellationToken, CancelTokenStore, Option<usize>);

impl Drop for TokenGuard {
    fn drop(&mut self) {
        if let Some(cancel_id) = self.2 {
            let store = self.1.clone();

            tokio::spawn(async move {
                let mut store = store.lock().await;
                store.remove(&cancel_id);
            });
        }
    }
}

async fn create_token(
    cancel_token_store: CancelTokenStore,
    cancel_id: Option<usize>,
) -> TokenGuard {
    let token = CancellationToken::new();

    if let Some(cancel_id) = cancel_id {
        {
            let mut store = cancel_token_store.lock().await;
            let old = store.insert(cancel_id, token.clone());
            if old.is_some() {
                warn!("cancel token already in use: {}", cancel_id);
            }
        }
    }

    TokenGuard(token, cancel_token_store, cancel_id)
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
