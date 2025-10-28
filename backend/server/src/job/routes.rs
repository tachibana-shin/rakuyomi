use crate::{job::State, AppError};
use anyhow::anyhow;
use axum::{
    extract::{Path, State as StateExtractor},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use shared::{
    model::{ChapterId, MangaId},
    source_collection::SourceCollection,
    usecases::fetch_manga_chapters_in_batch::Filter as ChaptersToDownloadFilter,
};
use uuid::Uuid;

use crate::job::dto::JobDetail;
use crate::job::state::RunningJob;
use crate::state::State as AppState;

use super::{
    download_chapter::DownloadChapterJob,
    download_scanlator_chapters::{DownloadScanlatorChaptersJob, ScanlatorFilter},
    download_unread_chapters::DownloadUnreadChaptersJob,
    state::Job,
};

pub fn routes() -> Router<AppState> {
    Router::<AppState>::new()
        .route("/jobs/download-chapter", post(create_download_chapter_job))
        .route(
            "/jobs/download-unread-chapters",
            post(create_download_unread_chapters_job),
        )
        .route(
            "/jobs/download-scanlator-chapters",
            post(create_download_scanlator_chapters_job),
        )
        .route("/jobs/{id}", get(get_job))
        .route("/jobs/{id}", delete(cancel_job))
}

#[derive(Deserialize)]
struct CreateDownloadChapterJobBody {
    source_id: String,
    manga_id: String,
    chapter_id: String,
    // chapter_num: Option<f64>,
}

impl From<CreateDownloadChapterJobBody> for ChapterId {
    fn from(value: CreateDownloadChapterJobBody) -> Self {
        ChapterId::from_strings(value.source_id, value.manga_id, value.chapter_id)
    }
}

async fn create_download_chapter_job(
    StateExtractor(AppState {
        database,
        settings,
        source_manager,
        chapter_storage,
        ..
    }): StateExtractor<AppState>,
    StateExtractor(State { job_registry }): StateExtractor<State>,
    Json(body): Json<CreateDownloadChapterJobBody>,
) -> Result<Json<Uuid>, AppError> {
    let id = Uuid::new_v4();
    // let chapter_num = body.chapter_num;
    let chapter_storage = chapter_storage.lock().await.clone();
    let settings = settings.lock().await;
    let job = DownloadChapterJob::spawn_new(
        source_manager,
        database,
        chapter_storage,
        body.into(),
        settings.concurrent_requests_pages.unwrap(),
    );

    job_registry
        .lock()
        .await
        .insert(id, RunningJob::DownloadChapter(job));

    Ok(Json(id))
}

#[derive(Deserialize)]
struct CreateDownloadUnreadChaptersJobBody {
    source_id: String,
    manga_id: String,
    amount: Option<usize>,
}

impl From<CreateDownloadUnreadChaptersJobBody> for MangaId {
    fn from(value: CreateDownloadUnreadChaptersJobBody) -> Self {
        MangaId::from_strings(value.source_id, value.manga_id)
    }
}

async fn create_download_unread_chapters_job(
    StateExtractor(AppState {
        source_manager,
        database,
        chapter_storage,
        settings,
        ..
    }): StateExtractor<AppState>,
    StateExtractor(State { job_registry }): StateExtractor<State>,
    Json(body): Json<CreateDownloadUnreadChaptersJobBody>,
) -> Result<Json<Uuid>, AppError> {
    let filter = match body.amount {
        Some(amount) => ChaptersToDownloadFilter::NextUnreadChapters(amount),
        None => ChaptersToDownloadFilter::AllUnreadChapters,
    };
    let manga_id = MangaId::from(body);

    let source_manager = source_manager.lock().await;
    let source = source_manager
        .get_by_id(manga_id.source_id())
        .ok_or(AppError::SourceNotFound)?
        .clone();

    let id = Uuid::new_v4();
    let chapter_storage = chapter_storage.lock().await.clone();
    let settings = settings.lock().await;
    let job = DownloadUnreadChaptersJob::spawn_new(
        source,
        database,
        chapter_storage,
        manga_id,
        filter,
        settings.concurrent_requests_pages.unwrap(),
    );

    job_registry
        .lock()
        .await
        .insert(id, RunningJob::DownloadUnreadChapters(job));

    Ok(Json(id))
}

#[derive(Deserialize, Clone)]
struct CreateDownloadScanlatorChaptersJobBody {
    source_id: String,
    manga_id: String,
    scanlator: String,
    amount: Option<usize>,
}

impl From<CreateDownloadScanlatorChaptersJobBody> for MangaId {
    fn from(value: CreateDownloadScanlatorChaptersJobBody) -> Self {
        MangaId::from_strings(value.source_id, value.manga_id)
    }
}

async fn create_download_scanlator_chapters_job(
    StateExtractor(AppState {
        source_manager,
        database,
        chapter_storage,
        settings,
        ..
    }): StateExtractor<AppState>,
    StateExtractor(State { job_registry }): StateExtractor<State>,
    Json(body): Json<CreateDownloadScanlatorChaptersJobBody>,
) -> Result<Json<Uuid>, AppError> {
    let manga_id = MangaId::from(body.clone());

    let source_manager = source_manager.lock().await;
    let source = source_manager
        .get_by_id(manga_id.source_id())
        .ok_or(AppError::SourceNotFound)?
        .clone();

    let scanlator_filter = ScanlatorFilter {
        scanlator: body.scanlator,
        amount: body.amount,
    };

    let id = Uuid::new_v4();
    let chapter_storage = chapter_storage.lock().await.clone();
    let settings = settings.lock().await;
    let job = DownloadScanlatorChaptersJob::spawn_new(
        source,
        database,
        chapter_storage,
        manga_id,
        scanlator_filter,
        settings.concurrent_requests_pages.unwrap(),
    );

    job_registry
        .lock()
        .await
        .insert(id, RunningJob::DownloadScanlatorChapters(job));

    Ok(Json(id))
}

#[derive(Deserialize)]
struct GetJobParams {
    id: Uuid,
}

async fn get_job(
    StateExtractor(State { job_registry }): StateExtractor<State>,
    Path(GetJobParams { id }): Path<GetJobParams>,
) -> Result<Json<JobDetail>, AppError> {
    let job = job_registry
        .lock()
        .await
        .remove(&id)
        .ok_or_else(|| anyhow!("couldn't find job"))?;

    let (detail, incomplete_job) = JobDetail::from_job(job).await;

    if let Some(incomplete_job) = incomplete_job {
        job_registry.lock().await.insert(id, incomplete_job);
    }

    Ok(Json(detail))
}

async fn cancel_job(
    StateExtractor(State { job_registry }): StateExtractor<State>,
    Path(GetJobParams { id }): Path<GetJobParams>,
) -> Result<Json<()>, AppError> {
    let job_registry = job_registry.lock().await;
    let job = job_registry
        .get(&id)
        .ok_or_else(|| anyhow!("couldn't find job"))?;

    match job {
        RunningJob::DownloadUnreadChapters(job) => job.cancel().await?,
        RunningJob::DownloadScanlatorChapters(job) => job.cancel().await?,
        _ => Err(anyhow!("job is not cancellable"))?,
    };

    Ok(Json(()))
}
