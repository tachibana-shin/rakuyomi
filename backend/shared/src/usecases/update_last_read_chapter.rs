use anyhow::Result;

use crate::{database::Database, model::ChapterId};

pub async fn update_last_read_chapter(db: &Database, id: &ChapterId) -> Result<()> {
    db.update_last_read_chapter(id).await?;

    Ok(())
}
