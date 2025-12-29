use crate::{
    database::Database,
    model::{MangaId, MangaState},
};
use anyhow::Result;

pub async fn set_manga_preferred_scanlator(
    db: &Database,
    manga_id: MangaId,
    preferred_scanlator: Option<String>,
) -> Result<()> {
    let updated_manga_state = MangaState {
        preferred_scanlator,
    };

    db.upsert_manga_state(&manga_id, updated_manga_state).await;

    Ok(())
}
