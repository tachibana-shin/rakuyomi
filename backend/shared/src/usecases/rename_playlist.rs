use anyhow::Result;

use crate::database::Database;

pub async fn rename_playlist(db: &Database, id: i64, name: String) -> Result<()> {
    db.rename_playlist(id, name).await
}
