use anyhow::Result;

use crate::{database::Database, model::MangaId};

pub async fn remove_manga_from_playlist(
    db: &Database,
    playlist_id: i64,
    manga_id: MangaId,
) -> Result<()> {
    db.remove_manga_from_playlist(playlist_id, manga_id).await
}
