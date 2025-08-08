use std::{path::PathBuf, sync::Arc};

use shared::{
    chapter_storage::ChapterStorage, model::ChapterId, source_collection::SourceCollection,
    source_manager::SourceManager, usecases,
};
use tokio::sync::Mutex;

use crate::{AppError, ErrorResponse, database::Database};

use super::state::{Job, JobState};

// FIXME this is kinda ugly, maybe some type aliases would help here
pub struct DownloadChapterJob(Arc<Mutex<Option<Result<PathBuf, ErrorResponse>>>>);

impl DownloadChapterJob {
    pub fn spawn_new(
        source_manager: Arc<Mutex<SourceManager>>,
        database: Database,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        chapter_title: &str,
        chapter_num: Option<f64>,
    ) -> Self {
        let output: Arc<Mutex<Option<Result<PathBuf, ErrorResponse>>>> = Default::default();
        let output_clone = output.clone();

        tokio::spawn(async move {
            *output_clone.lock().await =
                Some(Self::do_job(source_manager, database, chapter_storage, chapter_id, chapter_title, chapter_num).await);
        });

        Self(output)
    }

    async fn do_job(
        source_manager: Arc<Mutex<SourceManager>>,
        database: Database,
        chapter_storage: ChapterStorage,
        chapter_id: ChapterId,
        chapter_title: &str,
        chapter_num: Option<f64>,
    ) -> Result<PathBuf, ErrorResponse> {
        let source_manager = source_manager.lock().await;
        let source = source_manager
            .get_by_id(chapter_id.source_id())
            .ok_or(AppError::SourceNotFound)?;

        Ok(
            usecases::fetch_manga_chapter(source, &database, &chapter_storage, &chapter_id, &chapter_title, chapter_num)
                .await
                .map_err(AppError::from)?,
        )
    }
}

impl Job for DownloadChapterJob {
    type Progress = ();
    type Output = PathBuf;
    type Error = ErrorResponse;

    async fn cancel(&self) -> Result<(), AppError> {
        todo!()
    }

    async fn poll(&self) -> JobState<Self::Progress, Self::Output, Self::Error> {
        match &*self.0.lock().await {
            None => JobState::InProgress(()),
            Some(result) => match result {
                Ok(path) => JobState::Completed(path.clone()),
                Err(e) => JobState::Errored(e.clone()),
            },
        }
    }
}
