use anyhow::Result;

use crate::database::Database;

pub async fn delete_notification(db: &Database, id: i32) -> Result<()> {
    db.delete_notification(id).await;
    Ok(())
}
