use std::sync::Arc;

use futures::lock::Mutex;
use serde::Serialize;
use shared::{
    database::Database,
    source_collection::SourceCollection,
    source_manager::SourceManager,
    usecases::{self, get_manga_library},
};
use tokio_util::sync::CancellationToken;

use crate::ErrorResponse;

use super::state::{Job, JobState};

#[derive(Default)]
enum Status {
    #[default]
    Initializing,
    Progressing {
        current: usize,
        total: usize,
        errors: Vec<String>,
    },
    Finished {
        errors: Vec<String>,
    },
    Errored(String),
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Progress {
    Initializing,
    Refreshing {
        current: usize,
        total: usize,
        errors: Vec<String>,
    },
}

pub struct RefreshLibraryChaptersJob {
    cancellation_token: CancellationToken,
    status: Arc<Mutex<Status>>,
}

impl RefreshLibraryChaptersJob {
    pub fn spawn_new(
        source_manager: Arc<tokio::sync::Mutex<SourceManager>>,
        database: Arc<tokio::sync::Mutex<Database>>,
    ) -> Self {
        let cancellation_token = CancellationToken::new();
        let cancellation_token_clone = cancellation_token.clone();

        let status: Arc<Mutex<Status>> = Default::default();
        let status_clone = status.clone();

        tokio::spawn(async move {
            let status = status_clone;
            let cancellation_token = cancellation_token_clone;

            let (mangas, source_manager) = {
                let db = database.lock().await;
                let sm = source_manager.lock().await;
                let mangas = match get_manga_library(
                    &db,
                    &*sm,
                    &shared::settings::LibrarySortingMode::TitleAsc,
                )
                .await
                {
                    Ok(m) => m,
                    Err(e) => {
                        *status.lock().await = Status::Errored(e.to_string());
                        return;
                    }
                };
                (mangas, (*sm).clone())
            };

            let total = mangas.len();
            for (i, manga) in mangas.into_iter().enumerate() {
                if cancellation_token.is_cancelled() {
                    break;
                }

                {
                    let mut status_lock = status.lock().await;
                    match &mut *status_lock {
                        Status::Initializing => {
                            *status_lock = Status::Progressing {
                                current: i,
                                total,
                                errors: vec![],
                            };
                        }
                        Status::Progressing { current, .. } => {
                            *current = i;
                        }
                        _ => {}
                    }
                }

                let manga_id = manga.information.id;
                let source = match source_manager.get_by_id(manga_id.source_id()) {
                    Some(s) => s,
                    None => continue,
                };

                let db = database.lock().await;
                if let Err(e) = usecases::refresh_manga_chapters(
                    &cancellation_token,
                    &db,
                    source,
                    &manga_id,
                    60,
                )
                .await
                {
                    let mut status_lock = status.lock().await;
                    if let Status::Progressing { errors, .. } = &mut *status_lock {
                        errors.push(format!(
                            "{}: {}",
                            manga
                                .information
                                .title
                                .clone()
                                .unwrap_or_else(|| "Unknown".to_string()),
                            e
                        ));
                    }
                }
            }

            if !cancellation_token.is_cancelled() {
                let mut status_lock = status.lock().await;
                if let Status::Progressing { errors, .. } = &*status_lock {
                    *status_lock = Status::Finished {
                        errors: errors.clone(),
                    };
                } else {
                    *status_lock = Status::Finished { errors: vec![] };
                }
            }
        });

        Self {
            cancellation_token,
            status,
        }
    }
}

impl Job for RefreshLibraryChaptersJob {
    type Progress = Progress;
    type Output = Vec<String>;
    type Error = ErrorResponse;

    async fn cancel(&self) -> Result<(), crate::AppError> {
        self.cancellation_token.cancel();

        Ok(())
    }

    async fn poll(&self) -> JobState<Self::Progress, Self::Output, Self::Error> {
        let status = &*self.status.lock().await;

        match status {
            Status::Initializing => JobState::InProgress(Progress::Initializing),
            Status::Progressing {
                current,
                total,
                errors,
            } => JobState::InProgress(Progress::Refreshing {
                current: *current,
                total: *total,
                errors: errors.clone(),
            }),
            Status::Finished { errors } => JobState::Completed(errors.clone()),
            Status::Errored(e) => {
                let error = crate::AppError::from(anyhow::anyhow!(e.to_string()));
                JobState::Errored(error.into())
            }
        }
    }
}
