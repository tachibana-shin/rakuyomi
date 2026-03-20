use anyhow::Result;

use crate::{
    database::Database,
    model::{MangaId, TrackingCandidate},
};

pub async fn link_tracking_binding(
    db: &Database,
    manga_id: &MangaId,
    candidate: &TrackingCandidate,
) -> Result<()> {
    db.upsert_tracking_binding(manga_id, candidate).await
}
