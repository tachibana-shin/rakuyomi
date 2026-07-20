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

    let child_token = token.child_token();

    let fetch_task = async {
        source
            .get_manga_details(child_token.clone(), id.value().clone())
            .await
    };

    let manga_details = match timeout(duration, fetch_task).await {
        Ok(Ok(manga)) => manga,

        Ok(Err(e)) => return Err(anyhow!("source error: {}", e)),

        Err(_) => {
            child_token.cancel();
            return Err(anyhow!("timeout when refreshing manga details"));
        }
    };

    if db.find_cached_manga_information(id).await?.is_some() {
        db.upsert_cached_manga_details(id, &manga_details).await?;

        if let Some(url) = &manga_details.cover_url {
            chapter_storage
                .cached_poster(token, id, || source.get_image_request(url.to_owned(), None))
                .await?;
        }
    }

    Ok(manga_details.status)
}
