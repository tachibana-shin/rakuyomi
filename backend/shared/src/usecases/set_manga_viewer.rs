use crate::{database::Database, model::MangaId};
use anyhow::Result;

pub async fn set_manga_viewer(
    db: &Database,
    manga_id: MangaId,
    viewer: Option<i64>,
) -> Result<()> {
    db.upsert_manga_viewer(&manga_id, viewer).await?;

    Ok(())
}
