use std::time::Duration;

use axum::extract::{Path, Query, State as StateExtractor};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::Future;
use log::warn;
use serde::{Deserialize, Serialize};
use shared::model::{ChapterId, MangaId};
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
        .route("/mangas", get(get_mangas))
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
    q: String,
}

async fn get_mangas(
    StateExtractor(State {
        database,
        source_manager,
        ..
    }): StateExtractor<State>,
    Query(GetMangasQuery { q }): Query<GetMangasQuery>,
) -> Result<Json<Vec<Manga>>, AppError> {
    let source_manager = &*source_manager.lock().await;
    let results = cancel_after(Duration::from_secs(120), |token| {
        usecases::search_mangas(source_manager, &database, token, q)
    })
    .await
    .map_err(AppError::from_search_mangas_error)?
    .into_iter()
    .map(Manga::from)
    .collect();

    Ok(Json(results))
}

#[derive(Deserialize)]
struct MangaChaptersPathParams {
    source_id: String,
    manga_id: String,
}

impl From<MangaChaptersPathParams> for MangaId {
    fn from(value: MangaChaptersPathParams) -> Self {
        MangaId::from_strings(value.source_id, value.manga_id)
    }
}

async fn add_manga_to_library(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);

    usecases::add_manga_to_library(&database, manga_id).await?;

    Ok(Json(()))
}

async fn remove_manga_from_library(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);

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
    let chapters =
        usecases::get_cached_manga_chapters(&database, chapter_storage, manga_id).await?;

    let chapters = chapters.into_iter().map(Chapter::from).collect();

    Ok(Json(chapters))
}

async fn refresh_manga_chapters(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<MangaChaptersPathParams>,
) -> Result<Json<()>, AppError> {
    let manga_id = MangaId::from(params);
    let timeout_result = tokio::time::timeout(
        Duration::from_secs(60),
        usecases::refresh_manga_chapters(&database, &source, manga_id.clone()),
    )
    .await;

    match timeout_result {
        Ok(result) => result?,
        Err(_) => {
            return Err(AppError::Other(anyhow::anyhow!(
                "Refresh chapters timed out"
            )));
        }
    }

    Ok(Json(()))
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
        ..
    }): StateExtractor<State>,
    SourceExtractor(source): SourceExtractor,
    Path(params): Path<DownloadMangaChapterParams>,
) -> Result<Json<String>, AppError> {
    let settings = settings.lock().await;

    let chapter_id = ChapterId::from(params);
    let chapter_storage = &*chapter_storage.lock().await;
    let concurrent_requests_pages = settings.concurrent_requests_pages.unwrap();
    let output_path = usecases::fetch_manga_chapter(
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

    usecases::mark_chapter_as_read(&database, &chapter_id).await;

    Json(())
}

async fn update_last_read(
    StateExtractor(State { database, .. }): StateExtractor<State>,
    SourceExtractor(_source): SourceExtractor,
    Path(params): Path<DownloadMangaChapterParams>,
) -> Json<()> {
    let chapter_id = ChapterId::from(params);

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

    usecases::set_manga_preferred_scanlator(&database, manga_id, body.preferred_scanlator).await?;

    Ok(Json(()))
}

async fn cancel_after<F, Fut>(duration: Duration, f: F) -> Fut::Output
where
    Fut: Future,
    F: FnOnce(CancellationToken) -> Fut + Send,
{
    let token = CancellationToken::new();
    let future = f(token.clone());

    let request_cancellation_handle = tokio::spawn(async move {
        tokio::time::sleep(duration).await;

        warn!("cancellation requested!");
        token.cancel();
    });

    let result = future.await;

    request_cancellation_handle.abort();

    result
}
