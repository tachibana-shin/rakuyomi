use anyhow::Result;

use crate::{database::Database, model::MangaId};

pub async fn add_manga_to_playlist(
    db: &Database,
    playlist_id: i64,
    manga_id: MangaId,
) -> Result<()> {
    db.add_manga_to_playlist(playlist_id, manga_id).await
}
