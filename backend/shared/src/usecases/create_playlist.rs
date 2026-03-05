use anyhow::Result;

use crate::{database::Database, model::Playlist};

pub async fn create_playlist(db: &Database, name: String) -> Result<Playlist> {
    db.create_playlist(name).await
}
