use anyhow::{anyhow, Result};
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    database::Database,
    model::{ChapterInformation, MangaId},
    source::Source,
};

pub async fn refresh_manga_chapters<'a>(
    token: &CancellationToken,
    db: &'a Database,
    source: &'a Source,
    id: &'a MangaId,
    seconds: u64,
) -> Result<Vec<ChapterInformation>> {
    let duration = Duration::from_secs(seconds);

    let fetch_task = async {
        source
            .get_chapter_list(token.clone(), id.value().clone())
            .await
    };

    let fresh_chapter_informations = match timeout(duration, fetch_task).await {
        Ok(Ok(list)) => list.into_iter().map(From::from).collect::<Vec<_>>(),

        Ok(Err(e)) => return Err(anyhow!("source error: {}", e)),

        Err(_) => {
            // Cancel the operation
            token.cancel();
            return Err(anyhow!("timeout when refreshing chapters"));
        }
    };

    let _ = db
        .upsert_cached_chapter_informations(id, &fresh_chapter_informations)
        .await;

    Ok(fresh_chapter_informations)
}
