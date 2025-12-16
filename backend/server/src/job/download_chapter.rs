use shared::{
    chapter_downloader::DownloadError, chapter_storage::ChapterStorage, database::Database, model::ChapterId, source_collection::SourceCollection, source_manager::SourceManager, usecases
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::{AppError, ErrorResponse};

use super::state::{Job, JobState};

// FIXME this is kinda ugly, maybe some type aliases would help here
pub struct DownloadChapterJob {
    tx: watch::Sender<Option<Result<Arc<(PathBuf, Vec<DownloadError>)>, ErrorResponse>>>,
    rx: watch::Receiver<Option<Result<Arc<(PathBuf, Vec<DownloadError>)>, ErrorResponse>>>,
    handle: JoinHandle<()>,
}

impl DownloadChapterJob {
    pub fn spawn_new(
        source_manager: Arc<tokio::sync::Mutex<SourceManager>>,
        db: Arc<tokio::sync::Mutex<Database>>,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        concurrent_requests_pages: usize,
    ) -> Self {
        let (tx, rx) = watch::channel::<Option<Result<Arc<(PathBuf, Vec<DownloadError>)>, ErrorResponse>>>(None);

        let tx_clone = tx.clone();
        let handle = tokio::spawn(async move {
            let result = Self::do_job(
                source_manager,
                db,
                chapter_storage,
                chapter_id,
                concurrent_requests_pages,
            )
            .await
            .map(|p| Arc::new(p));

            let _ = tx_clone.send_replace(Some(result));
        });

        Self { tx, rx, handle }
    }

    async fn do_job(
        source_manager: Arc<tokio::sync::Mutex<SourceManager>>,
        db: Arc<tokio::sync::Mutex<Database>>,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        concurrent_requests_pages: usize,
    ) -> Result<(PathBuf, Vec<DownloadError>), ErrorResponse> {
        let source = {
            let mgr = source_manager.lock().await;
            mgr.get_by_id(chapter_id.source_id())
                .cloned()
                .ok_or(AppError::SourceNotFound)?
        };
        let db = db.lock().await;

        Ok(usecases::fetch_manga_chapter(
            &db,
            &source,
            &chapter_storage,
            &chapter_id,
            concurrent_requests_pages,
        )
        .await
        .map_err(AppError::from)?)
    }
}

impl Job for DownloadChapterJob {
    type Progress = ();
    type Output = Arc<(PathBuf, Vec<DownloadError>)>;
    type Error = ErrorResponse;

    async fn cancel(&self) -> Result<(), AppError> {
        self.handle.abort();

        let _ = self.tx.send(Some(Err(ErrorResponse {
            message: "Download was canceled by user".into(),
        })));

        Ok(())
    }

    async fn poll(&self) -> JobState<Self::Progress, Self::Output, Self::Error> {
        match self.rx.borrow().as_ref() {
            None => JobState::InProgress(()),
            Some(Ok(path)) => JobState::Completed(path.clone()),
            Some(Err(e)) => JobState::Errored(e.clone()),
        }
    }
}
