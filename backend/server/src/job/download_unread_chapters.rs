use std::sync::Arc;

use anyhow::anyhow;
use futures::{lock::Mutex, pin_mut, StreamExt};
use serde::Serialize;
use shared::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::MangaId,
    source::Source,
    usecases::{
        self,
        fetch_manga_chapters_in_batch::{Filter as ChapterToDownloadFilter, ProgressReport},
    },
};
use tokio_util::sync::CancellationToken;

use crate::{AppError, ErrorResponse};

use super::state::{Job, JobState};

#[derive(Default)]
enum Status {
    #[default]
    Initializing,
    Initialized(ProgressReport),
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Progress {
    Initializing,
    Downloading { downloaded: usize, total: usize },
}

pub struct DownloadUnreadChaptersJob {
    cancellation_token: CancellationToken,
    status: Arc<Mutex<Status>>,
}

impl DownloadUnreadChaptersJob {
    pub fn spawn_new(
        source: Source,
        database: Arc<tokio::sync::Mutex<Database>>,
        chapter_storage: ChapterStorage,
        manga_id: MangaId,
        filter: ChapterToDownloadFilter,
        langs: Vec<String>,
        concurrent_requests_pages: usize,
    ) -> Self {
        let cancellation_token = CancellationToken::new();
        let cancellation_token_clone = cancellation_token.clone();

        let status: Arc<Mutex<Status>> = Default::default();
        let status_clone = status.clone();

        tokio::spawn(async move {
            let status = status_clone;
            let cancellation_token = cancellation_token_clone;
            let database = { database.lock().await };
            let lang_refs: Vec<&str> = langs.iter().map(|s| s.as_str()).collect();

            let progress_report_stream = usecases::fetch_manga_chapters_in_batch(
                cancellation_token.clone(),
                &source,
                &database,
                &chapter_storage,
                manga_id,
                filter,
                &lang_refs,
                concurrent_requests_pages,
            );

            pin_mut!(progress_report_stream);

            while let Some(progress_report) = progress_report_stream.next().await {
                let terminated = matches!(
                    &progress_report,
                    ProgressReport::Finished
                        | ProgressReport::Errored(_)
                        | ProgressReport::Cancelled
                );

                *status.lock().await = Status::Initialized(progress_report);

                if terminated {
                    break;
                }
            }
        });

        Self {
            cancellation_token,
            status,
        }
    }
}

impl Job for DownloadUnreadChaptersJob {
    type Progress = Progress;
    type Output = ();
    type Error = ErrorResponse;

    async fn cancel(&self) -> Result<(), crate::AppError> {
        self.cancellation_token.cancel();

        Ok(())
    }

    async fn poll(&self) -> JobState<Self::Progress, Self::Output, Self::Error> {
        let status = &*self.status.lock().await;

        match status {
            Status::Initializing => JobState::InProgress(Progress::Initializing),
            Status::Initialized(report) => match report {
                ProgressReport::Progressing { downloaded, total } => {
                    JobState::InProgress(Progress::Downloading {
                        downloaded: *downloaded,
                        total: *total,
                    })
                }
                ProgressReport::Finished => JobState::Completed(()),
                // FIXME this is weird as fuck
                ProgressReport::Errored(e) => {
                    // FIXME THIS IS SO WRONG
                    // We don't properly report download errors from the "fetch_manga_chapters_in_batch"
                    // function as a NetworkError. This is _some_ kind of reporting, but it's inconsistent
                    // with how it's done on other parts of the application.
                    let error = AppError::from(anyhow!(e.to_string()));

                    JobState::Errored(error.into())
                }
                // FIXME i mean we should report cancellation decently however as this is requested by the user
                // i think it's fine for it to be considered as a completion maybe..?
                ProgressReport::Cancelled => JobState::Completed(()),
            },
        }
    }
}
