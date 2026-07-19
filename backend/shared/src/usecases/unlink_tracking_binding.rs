use anyhow::Result;

use crate::{
    database::Database,
    model::{MangaId, TrackingService},
};

pub async fn unlink_tracking_binding(
    db: &Database,
    manga_id: &MangaId,
    service: TrackingService,
) -> Result<()> {
    db.delete_tracking_binding(manga_id, service).await
}
