use anyhow::Result;

use crate::{database::Database, model::Manga, source_collection::SourceCollection};

pub async fn get_mangas_in_playlist(
    db: &Database,
    playlist_id: i64,
    source_collection: &impl SourceCollection,
    library_sorting_mode: &crate::settings::LibrarySortingMode,
) -> Result<Vec<Manga>> {
    db.get_manga_library_in_playlist_with_read_count(
        playlist_id,
        source_collection,
        library_sorting_mode,
    )
    .await
}
