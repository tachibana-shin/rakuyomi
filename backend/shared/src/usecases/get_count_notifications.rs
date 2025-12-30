use anyhow::Result;

use crate::database::Database;

pub async fn get_count_notifications(db: &Database) -> Result<i32> {
    db.get_count_notifications().await
}
