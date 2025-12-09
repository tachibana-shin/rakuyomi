use anyhow::{anyhow, Result};
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::MangaId,
    source::{model::PublishingStatus, Source},
};

pub async fn refresh_manga_details(
    token: &CancellationToken,
    db: &Database,
    chapter_storage: &ChapterStorage,
    source: &Source,
    id: &MangaId,
    seconds: u64,
) -> Result<PublishingStatus> {
    let duration = Duration::from_secs(seconds);

    let fetch_task = async {
        source
            .get_manga_details(token.clone(), id.value().clone())
            .await
    };

    let manga_details = match timeout(duration, fetch_task).await {
        Ok(Ok(manga)) => manga,

        Ok(Err(e)) => return Err(anyhow!("source error: {}", e)),

        Err(_) => {
            // Cancel the operation
            token.cancel();
            return Err(anyhow!("timeout when refreshing manga details"));
        }
    };

    let _ = db.upsert_cached_manga_details(&id, &manga_details).await?;

    // source.manifest().

    if let Some(url) = &manga_details.cover_url {
        chapter_storage
            .cached_poster(url, || source.get_image_request(url.to_owned(), None))
            .await?;
    }

    Ok(manga_details.status)
}
