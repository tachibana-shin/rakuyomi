use anyhow::Result;

use crate::{database::Database, model::Playlist};

pub async fn get_playlists(db: &Database) -> Result<Vec<Playlist>> {
    db.get_playlists().await
}
