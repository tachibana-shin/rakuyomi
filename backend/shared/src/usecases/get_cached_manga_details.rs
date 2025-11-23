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
                let req = source.get_image_request(url.to_owned()).await?;

                let output = chapter_storage
                    .cached_poster(&url, || source.get_image_request(url.to_owned()))
                    .await?
                    .canonicalize()?;

                details.url = url::Url::from_file_path(output)
                    .map_err(|_| {
                        println!(
                            "Error converting path to URL: {:?}",
                            url::ParseError::IdnaError
                        );
                    })
                    .ok();
            }

            Ok(Some((details, per_read)))
        }
        None => return Ok(None),
    }
}
