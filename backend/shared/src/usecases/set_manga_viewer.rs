use crate::{database::Database, model::MangaId};
use anyhow::{anyhow, Result};

pub async fn set_manga_viewer(db: &Database, manga_id: MangaId, viewer: Option<i64>) -> Result<()> {
    // Validate viewer is within valid MangaViewer range (0..=4) if provided
    if let Some(v) = viewer {
        if v < 0 || v > 4 {
            return Err(anyhow!(
                "Invalid viewer value: {}. Must be between 0 and 4.",
                v
            ));
        }
    }

    db.upsert_manga_viewer(&manga_id, viewer).await?;

    Ok(())
}
