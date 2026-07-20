use anyhow::Result;
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    database::Database,
    model::{MangaId, MangaInformation},
    source::Source,
};

pub async fn add_manga_to_library(
    token: &CancellationToken,
    db: &Database,
    source: &Source,
    id: MangaId,
    seconds: u64,
) -> Result<()> {
    if db.find_cached_manga_information(&id).await?.is_none() {
        let child_token = token.child_token();
        let fetch_task = source.get_manga_details(child_token.clone(), id.value().clone());

        if let Ok(Ok(manga)) = timeout(Duration::from_secs(seconds), fetch_task).await {
            let _ = db
                .upsert_cached_manga_information(&[MangaInformation::from(manga)])
                .await;
        }
    }

    db.add_manga_to_library(id).await?;

    Ok(())
}
