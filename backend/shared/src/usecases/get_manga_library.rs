use anyhow::Result;

use crate::{database::Database, model::Manga, source_collection::SourceCollection};

pub async fn get_manga_library(
    db: &Database,
    source_collection: &impl SourceCollection,
) -> Result<Vec<Manga>> {
    db.get_manga_library_with_read_count(source_collection)
        .await
}
