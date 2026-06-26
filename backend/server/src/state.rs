use std::{collections::HashMap, path::PathBuf, sync::Arc};

use shared::{
    chapter_storage::ChapterStorage, database::Database, settings::Settings,
    source_manager::SourceManager,
};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use axum::extract::FromRef;

use crate::job::State as JobState;

/// A shared log of startup warnings/errors to be displayed to the user via Lua.
#[derive(Clone)]
pub struct StartupLog {
    inner: Arc<Mutex<Vec<String>>>,
}

impl StartupLog {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn push(&self, msg: String) {
        self.inner.lock().await.push(msg);
    }

    pub async fn drain(&self) -> Vec<String> {
        std::mem::take(&mut *self.inner.lock().await)
    }
}

impl Default for StartupLog {
    fn default() -> Self {
        Self::new()
    }
}

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
    pub startup_log: StartupLog,
}

impl FromRef<State> for JobState {
    fn from_ref(state: &State) -> Self {
        state.job_state.clone()
    }
}
