use anyhow::Result;

use crate::{database::Database, model::MangaId};

pub async fn add_manga_to_library(db: &Database, id: MangaId) -> Result<()> {
    db.add_manga_to_library(id).await?;

    Ok(())
}
