use crate::{database::Database, model::MangaId};
use anyhow::Result;

pub async fn get_manga_preferred_scanlator(
    db: &Database,
    manga_id: &MangaId,
) -> Result<Option<String>> {
    let state = db.find_manga_state(manga_id).await?;
    Ok(state.and_then(|s| s.preferred_scanlator))
}
