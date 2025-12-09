use std::{collections::HashMap, path::PathBuf, sync::Arc};

use axum_macros::FromRef;
use shared::{
    chapter_storage::ChapterStorage, database::Database, settings::Settings,
    source_manager::SourceManager,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::job::State as JobState;

#[derive(Clone, FromRef)]
pub struct State {
    pub source_manager: Arc<Mutex<SourceManager>>,
    pub database: Arc<Mutex<Database>>,
    pub chapter_storage: Arc<Mutex<ChapterStorage>>,
    pub settings: Arc<Mutex<Settings>>,
    pub settings_path: PathBuf,
    pub job_state: JobState,
    pub cancel_token_store: Arc<Mutex<HashMap<usize, CancellationToken>>>,
}
