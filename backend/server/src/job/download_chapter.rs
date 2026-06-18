use shared::{
    chapter_downloader::DownloadError, chapter_storage::ChapterStorage, database::Database,
    model::ChapterId, source_collection::SourceCollection, source_manager::SourceManager, usecases,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::watch;
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
        db: Arc<tokio::sync::Mutex<Database>>,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        concurrent_requests_pages: usize,
        optimize_image: bool,
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
            let result = Self::do_job(
                token_clone,
                source_manager,
                db,
                chapter_storage,
                chapter_id,
                concurrent_requests_pages,
                optimize_image,
                progress_tx_clone,
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
        db: Arc<tokio::sync::Mutex<Database>>,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        concurrent_requests_pages: usize,
        optimize_image: bool,
        progress_tx: ProgressSender,
    ) -> Result<(PathBuf, Vec<DownloadError>), ErrorResponse> {
        let source = {
            let mgr = source_manager.lock().await;
            mgr.get_by_id(chapter_id.source_id())
                .cloned()
                .ok_or(AppError::SourceNotFound)?
        };
        let db: tokio::sync::MutexGuard<'_, Database> = { db.lock().await };

        let progress_callback = {
            let progress_tx = progress_tx.clone();
            Arc::new(move |processed: f32, total: f32| {
                let _ = progress_tx.send(Progress::Downloading {
                    processed: processed as u32,
                    total: total as u32,
                });
            })
        };

        Ok(usecases::fetch_manga_chapter(
            &cancellation_token,
            &db,
            &source,
            &chapter_storage,
            &chapter_id,
            concurrent_requests_pages,
            optimize_image,
            Some(progress_callback),
        )
        .await
        .map_err(AppError::from)?)
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
