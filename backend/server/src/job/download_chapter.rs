use shared::{
    chapter_downloader::{
        ensure_chapter_is_in_storage, DownloadError, Error as ChapterDownloaderError,
    },
    chapter_storage::ChapterStorage,
    database::Database,
    model::ChapterId,
    source_collection::SourceCollection,
    source_manager::SourceManager,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{AppError, ErrorResponse};

use super::state::{Job, JobState};

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Progress {
    Initializing,
    Downloading { processed: u32, total: u32 },
}

type JobSender = watch::Sender<Option<Result<Arc<(PathBuf, Vec<DownloadError>)>, ErrorResponse>>>;
type JobReceiver =
    watch::Receiver<Option<Result<Arc<(PathBuf, Vec<DownloadError>)>, ErrorResponse>>>;

type ProgressSender = watch::Sender<Progress>;
type ProgressReceiver = watch::Receiver<Progress>;

pub struct DownloadChapterJob {
    tx: JobSender,
    rx: JobReceiver,
    progress_rx: ProgressReceiver,
    handle: JoinHandle<()>,
    cancellation_token: CancellationToken,
}

impl DownloadChapterJob {
    pub fn spawn_new(
        source_manager: Arc<tokio::sync::Mutex<SourceManager>>,
        db: Arc<Database>,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        concurrent_requests_pages: usize,
        optimize_image: bool,
        download_semaphore: Arc<Semaphore>,
        use_ram: bool,
    ) -> Self {
        let (tx, rx) = watch::channel::<
            Option<Result<Arc<(PathBuf, Vec<DownloadError>)>, ErrorResponse>>,
        >(None);

        let (progress_tx, progress_rx) = watch::channel(Progress::Initializing);

        let cancellation_token = CancellationToken::new();
        let tx_clone = tx.clone();
        let progress_tx_clone = progress_tx.clone();
        let token_clone = cancellation_token.clone();
        let handle = tokio::spawn(async move {
            let _permit = download_semaphore.acquire().await;
            let result = Self::do_job(
                token_clone,
                source_manager,
                db,
                chapter_storage,
                chapter_id,
                concurrent_requests_pages,
                optimize_image,
                progress_tx_clone,
                use_ram,
            )
            .await
            .map(Arc::new);

            let _ = tx_clone.send_replace(Some(result));
        });

        Self {
            tx,
            rx,
            progress_rx,
            handle,
            cancellation_token,
        }
    }

    async fn do_job(
        cancellation_token: CancellationToken,
        source_manager: Arc<tokio::sync::Mutex<SourceManager>>,
        db: Arc<Database>,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        concurrent_requests_pages: usize,
        optimize_image: bool,
        progress_tx: ProgressSender,
        use_ram: bool,
    ) -> Result<(PathBuf, Vec<DownloadError>), ErrorResponse> {
        let source = {
            let mgr = source_manager.lock().await;
            mgr.get_by_id(chapter_id.source_id())
                .cloned()
                .ok_or(AppError::SourceNotFound)?
        };

        let (manga, chapter) = {
            let manga = db
                .find_cached_manga_information(chapter_id.manga_id())
                .await
                .map_err(|e| ErrorResponse {
                    message: format!("Failed to fetch manga: {e}"),
                })?
                .ok_or_else(|| ErrorResponse {
                    message: "Manga not found in database".into(),
                })?;
            let chapter = db
                .find_cached_chapter_information(&chapter_id)
                .await
                .map_err(|e| ErrorResponse {
                    message: format!("Failed to fetch chapter: {e}"),
                })?
                .ok_or_else(|| ErrorResponse {
                    message: "Chapter not found in database".into(),
                })?;
            (manga, chapter)
        };

        let progress_callback = {
            let progress_tx = progress_tx.clone();
            Arc::new(move |processed: f32, total: f32| {
                let _ = progress_tx.send(Progress::Downloading {
                    processed: processed as u32,
                    total: total as u32,
                });
            })
        };

        let (path, errors) = ensure_chapter_is_in_storage(
            &cancellation_token,
            &chapter_storage,
            &source,
            &manga,
            &chapter,
            concurrent_requests_pages,
            optimize_image,
            Some(progress_callback),
            use_ram,
        )
        .await
        .map_err(|e| {
            let app_error = match e {
                ChapterDownloaderError::DownloadError(e) => AppError::NetworkFailure(e),
                ChapterDownloaderError::Other(e) => AppError::Other(e),
            };
            ErrorResponse::from(app_error)
        })?;

        Ok((path, errors))
    }
}

impl Job for DownloadChapterJob {
    type Progress = Progress;
    type Output = Arc<(PathBuf, Vec<DownloadError>)>;
    type Error = ErrorResponse;

    async fn cancel(&self) -> Result<(), AppError> {
        self.cancellation_token.cancel();
        self.handle.abort();

        let _ = self.tx.send(Some(Err(ErrorResponse {
            message: "Download was canceled by user".into(),
        })));

        Ok(())
    }

    async fn poll(&self) -> JobState<Self::Progress, Self::Output, Self::Error> {
        match self.rx.borrow().as_ref() {
            None => JobState::InProgress(*self.progress_rx.borrow()),
            Some(Ok(path)) => JobState::Completed(path.clone()),
            Some(Err(e)) => JobState::Errored(e.clone()),
        }
    }
}
