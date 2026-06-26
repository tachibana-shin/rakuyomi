use std::{collections::HashMap, path::PathBuf, sync::Arc};

use shared::{
    chapter_storage::ChapterStorage, database::Database, settings::Settings,
    source_manager::SourceManager,
};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use axum::extract::FromRef;

use crate::job::State as JobState;

#[derive(Clone)]
pub struct State {
    pub source_manager: Arc<Mutex<SourceManager>>,
    pub database: Arc<Database>,
    pub chapter_storage: Arc<Mutex<ChapterStorage>>,
    pub settings: Arc<Mutex<Settings>>,
    pub settings_path: PathBuf,
    pub job_state: JobState,
    pub cancel_token_store: Arc<Mutex<HashMap<usize, CancellationToken>>>,
    pub download_semaphore: Arc<Semaphore>,
}

impl FromRef<State> for JobState {
    fn from_ref(state: &State) -> Self {
        state.job_state.clone()
    }
}
