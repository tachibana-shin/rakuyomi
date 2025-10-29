use anyhow::Result;

use crate::{database::Database, model::Manga, source_collection::SourceCollection};

pub async fn get_manga_library(
    db: &Database,
    source_collection: &impl SourceCollection,
    library_sorting_mode: &crate::settings::LibrarySortingMode,
) -> Result<Vec<Manga>> {
    db.get_manga_library_with_read_count(source_collection, library_sorting_mode)
        .await
}
