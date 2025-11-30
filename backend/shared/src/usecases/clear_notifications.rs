use anyhow::Result;

use crate::database::Database;

pub async fn clear_notifications(db: &Database) -> Result<()> {
    db.clear_notifications().await;
    Ok(())
}
