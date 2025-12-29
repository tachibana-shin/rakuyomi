use serde::Serialize;
use serde_json::Value;

use super::{
    download_chapter::DownloadChapterJob,
    download_scanlator_chapters::DownloadScanlatorChaptersJob,
    download_unread_chapters::DownloadUnreadChaptersJob,
    state::{Job, JobState, RunningJob},
};

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type", content = "data")]
pub enum JobDetail {
    Pending(Value),
    Completed(Value),
    Error(Value),
}

impl JobDetail {
    pub async fn from_job(job: RunningJob) -> (Self, Option<RunningJob>) {
        match job {
            RunningJob::Chapter(job) => Self::from_download_chapter_job(job).await,
            RunningJob::UnreadChapters(job) => Self::from_download_unread_chapters_job(job).await,
            RunningJob::ScanlatorChapters(job) => {
                Self::from_download_scanlator_chapters_job(job).await
            }
        }
    }

    async fn from_download_chapter_job(job: DownloadChapterJob) -> (Self, Option<RunningJob>) {
        match job.poll().await {
            JobState::InProgress(v) => (
                JobDetail::Pending(serde_json::to_value(v).unwrap()),
                Some(RunningJob::Chapter(job)),
            ),
            JobState::Completed(v) => {
                (JobDetail::Completed(serde_json::to_value(v).unwrap()), None)
            }
            JobState::Errored(v) => (JobDetail::Error(serde_json::to_value(v).unwrap()), None),
        }
    }

    async fn from_download_unread_chapters_job(
        job: DownloadUnreadChaptersJob,
    ) -> (Self, Option<RunningJob>) {
        match job.poll().await {
            JobState::InProgress(v) => (
                JobDetail::Pending(serde_json::to_value(v).unwrap()),
                Some(RunningJob::UnreadChapters(job)),
            ),
            JobState::Completed(_) => (JobDetail::Completed(().into()), None),
            JobState::Errored(v) => (JobDetail::Error(serde_json::to_value(v).unwrap()), None),
        }
    }

    async fn from_download_scanlator_chapters_job(
        job: DownloadScanlatorChaptersJob,
    ) -> (Self, Option<RunningJob>) {
        match job.poll().await {
            JobState::InProgress(v) => (
                JobDetail::Pending(serde_json::to_value(v).unwrap()),
                Some(RunningJob::ScanlatorChapters(job)),
            ),
            JobState::Completed(_) => (JobDetail::Completed(().into()), None),
            JobState::Errored(v) => (JobDetail::Error(serde_json::to_value(v).unwrap()), None),
        }
    }
}
