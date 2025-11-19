use serde::Serialize;
use shared::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::MangaId,
    source::Source,
    usecases::fetch_manga_chapters_in_batch::{Filter, ProgressReport},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::state::{Job, JobState};
use crate::{AppError, ErrorResponse};

// Create a serializable progress type
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum SerializableProgress {
    Initializing,
    Downloading { downloaded: usize, total: usize },
}

pub struct DownloadScanlatorChaptersJob {
    cancellation_token: CancellationToken,
    output: Arc<Mutex<Option<Result<(), ErrorResponse>>>>,
    progress: Arc<Mutex<SerializableProgress>>,
}

#[derive(Debug, Clone)]
pub struct ScanlatorFilter {
    pub scanlator: String,
    pub amount: Option<usize>,
}

impl DownloadScanlatorChaptersJob {
    pub fn spawn_new(
        source: Source,
        database: Arc<tokio::sync::Mutex<Database>>,
        chapter_storage: ChapterStorage,
        manga_id: MangaId,
        scanlator_filter: ScanlatorFilter,
        concurrent_requests_pages: usize,
    ) -> Self {
        let cancellation_token = CancellationToken::new();
        let output: Arc<Mutex<Option<Result<(), ErrorResponse>>>> = Default::default();
        let progress: Arc<Mutex<SerializableProgress>> =
            Arc::new(Mutex::new(SerializableProgress::Initializing));

        let output_clone = output.clone();
        let progress_clone = progress.clone();
        let cancellation_token_clone = cancellation_token.clone();

        tokio::spawn(async move {
            // Create the filter for the batch fetch function
            let filter = Filter::ScanlatorChapters {
                scanlator: scanlator_filter.scanlator,
                amount: scanlator_filter.amount,
            };
            let database = database.lock().await;

            let stream =
                shared::usecases::fetch_manga_chapters_in_batch::fetch_manga_chapters_in_batch(
                    cancellation_token_clone,
                    &source,
                    &database,
                    &chapter_storage,
                    manga_id,
                    filter,
                    concurrent_requests_pages,
                );

            use futures::StreamExt;

            // Use Box::pin to handle the stream properly
            let mut pinned_stream = Box::pin(stream);

            while let Some(progress_report) = pinned_stream.next().await {
                match progress_report {
                    ProgressReport::Progressing { downloaded, total } => {
                        *progress_clone.lock().await =
                            SerializableProgress::Downloading { downloaded, total };
                    }
                    ProgressReport::Finished => {
                        *output_clone.lock().await = Some(Ok(()));
                        break;
                    }
                    ProgressReport::Cancelled => {
                        *output_clone.lock().await = Some(Ok(()));
                        break;
                    }
                    ProgressReport::Errored(error) => {
                        *output_clone.lock().await = Some(Err(ErrorResponse {
                            message: error.to_string(),
                        }));
                        break;
                    }
                }
            }
        });

        Self {
            cancellation_token,
            output,
            progress,
        }
    }
}

impl Job for DownloadScanlatorChaptersJob {
    type Progress = SerializableProgress;
    type Output = ();
    type Error = ErrorResponse;

    async fn cancel(&self) -> Result<(), AppError> {
        self.cancellation_token.cancel();
        Ok(())
    }

    async fn poll(&self) -> JobState<Self::Progress, Self::Output, Self::Error> {
        match &*self.output.lock().await {
            None => {
                let progress = self.progress.lock().await.clone();
                JobState::InProgress(progress)
            }
            Some(result) => match result {
                Ok(_) => JobState::Completed(()),
                Err(e) => JobState::Errored(e.clone()),
            },
        }
    }
}
