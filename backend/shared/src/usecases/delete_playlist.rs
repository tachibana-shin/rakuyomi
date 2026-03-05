use anyhow::Result;

use crate::database::Database;

pub async fn delete_playlist(db: &Database, id: i64) -> Result<()> {
    db.delete_playlist(id).await
}
