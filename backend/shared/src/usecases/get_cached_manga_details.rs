use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::{chapter_storage::ChapterStorage, database::Database, model::MangaId, source::Source};

pub async fn get_cached_manga_details(
    token: &CancellationToken,
    db: &Database,
    chapter_storage: &ChapterStorage,
    source: &Source,
    id: MangaId,
) -> Result<Option<(crate::source::model::Manga, f64)>> {
    match db.find_cached_manga_details(&id).await? {
        Some((mut details, per_read)) => {
            if let Some(url) = &details.cover_url {
                let output = chapter_storage
                    .cached_poster(token, url, || {
                        source.get_image_request(url.to_owned(), None)
                    })
                    .await?;

                details.url = match url::Url::from_file_path(output.clone()) {
                    Ok(url) => Some(url),
                    Err(_) => url::Url::from_file_path(output.canonicalize()?)
                        .map_err(|_| {
                            println!(
                                "Error converting path to URL: {:?}",
                                url::ParseError::IdnaError
                            );
                        })
                        .ok(),
                };
            }

            Ok(Some((details, per_read)))
        }
        None => Ok(None),
    }
}
