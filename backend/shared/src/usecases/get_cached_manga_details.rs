use anyhow::Result;

use crate::{chapter_storage::ChapterStorage, database::Database, model::MangaId, source::Source};

pub async fn get_cached_manga_details(
    db: &Database,
    chapter_storage: &ChapterStorage,
    source: &Source,
    id: MangaId,
) -> Result<Option<(crate::source::model::Manga, f64)>> {
    match db.find_cached_manga_details(&id).await {
        Some((mut details, per_read)) => {
            if let Some(url) = &details.cover_url {
                details.url = Some(
                    url::Url::from_file_path(
                        chapter_storage
                            .cached_poster(&url, || source.get_image_request(url.to_owned()))
                            .await?,
                    )
                    .map_err(|_| url::ParseError::IdnaError)?,
                );
            }

            Ok(Some((details, per_read)))
        }
        None => return Ok(None),
    }
}
