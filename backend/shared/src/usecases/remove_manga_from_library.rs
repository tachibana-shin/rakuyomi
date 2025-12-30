use anyhow::Result;

use crate::{database::Database, model::MangaId};

pub async fn remove_manga_from_library(db: &Database, id: MangaId) -> Result<()> {
    db.remove_manga_from_library(id).await?;

    Ok(())
}
