use anyhow::Result;

use crate::{
    database::Database,
    model::{MangaId, TrackingBinding},
};

pub async fn list_tracking_bindings(db: &Database, manga_id: &MangaId) -> Result<Vec<TrackingBinding>> {
    db.list_tracking_bindings(manga_id).await
}
