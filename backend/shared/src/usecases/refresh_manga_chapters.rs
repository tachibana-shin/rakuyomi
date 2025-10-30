use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::{database::Database, model::MangaId, source::Source};

pub async fn refresh_manga_chapters(db: &Database, source: &Source, id: MangaId) -> Result<()> {
    let fresh_chapter_informations = source
        .get_chapter_list(CancellationToken::new(), id.value().clone())
        .await?
        .into_iter()
        .map(From::from)
        .collect::<Vec<_>>();

    let _ = db.upsert_cached_chapter_informations(&id, &fresh_chapter_informations)
        .await;

    Ok(())
}
