use crate::{
    database::Database,
    model::{Manga, MangaInformation, MangaState, SourceInformation},
    source_collection::SourceCollection,
};
use futures::{stream, StreamExt};
use log::warn;
use tokio::time::timeout;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use unicode_normalization::UnicodeNormalization;

const CONCURRENT_SEARCH_REQUESTS: usize = 5;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SearchError {
    pub source_id: String,
    pub reason: String,
}

pub async fn search_mangas(
    source_collection: &impl SourceCollection,
    db: &Database,
    cancellation_token: CancellationToken,
    query: String,
    exclude: &Option<Vec<String>>,
    seconds: u64,
) -> Result<(Vec<Manga>, Vec<SearchError>), Error> {
    // FIXME this looks awful
    let query = &query;

    // FIXME this kinda of works because cloning a source is cheap
    // (it has internal mutability yadda yadda).
    // we can't keep `source_collection` alive across async await points
    // because lifetimes fuckery
    let sources = source_collection
        .sources()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();

    let source_results: Vec<(SourceMangaSearchResults, Option<SearchError>)> =
        stream::iter(sources)
            .map(|source| {
                let cancellation_token = cancellation_token.clone();
                let query = query.to_string();

                async move {
                    if exclude
                        .as_ref()
                        .map(|exclude| exclude.contains(&source.manifest().info.id))
                        .unwrap_or(false)
                    {
                        let source_info = source.manifest().into();
                        return (
                            SourceMangaSearchResults {
                                source_information: source_info,
                                mangas: vec![],
                            },
                            None,
                        );
                    }

                    let token = cancellation_token.child_token();

                    let fetch_task = async { source.search_mangas(token.clone(), query).await };

                    let (manga_informations, error) =
                        match timeout(Duration::from_secs(seconds), fetch_task).await {
                            Ok(Ok(source_mangas)) => (
                                source_mangas
                                    .into_iter()
                                    .map(MangaInformation::from)
                                    .collect(),
                                None,
                            ),

                            Ok(Err(e)) => {
                                warn!(
                                    "failed to search mangas from source {}: {}",
                                    source.manifest().info.id,
                                    e
                                );

                                (
                                    vec![],
                                    Some(SearchError {
                                        source_id: source.manifest().info.id.clone(),
                                        reason: e.to_string(),
                                    }),
                                )
                            }

                            Err(_) => {
                                token.cancel();

                                (
                                    vec![],
                                    Some(SearchError {
                                        source_id: source.manifest().info.id.clone(),
                                        reason: "timeout".to_string(),
                                    }),
                                )
                            }
                        };

                    // Write through to the database
                    let _ = db
                        .upsert_cached_manga_information(&manga_informations)
                        .await;

                    // Fetch unread chapters count for each manga
                    let manga_ids: Vec<_> =
                        manga_informations.iter().map(|m| m.id.clone()).collect();
                    let unread_counts_map =
                        db.fetch_unread_chapter_counts_minimal(&manga_ids).await;
                    let mangas: Vec<_> = manga_informations
                        .into_iter()
                        .map(move |manga| {
                            let unread_count = unread_counts_map.get(&manga.id).copied();
                            (manga, unread_count)
                        })
                        .collect();

                    (
                        SourceMangaSearchResults {
                            source_information: source.manifest().into(),
                            mangas,
                        },
                        error,
                    )
                }
            })
            .buffered(CONCURRENT_SEARCH_REQUESTS)
            .collect::<Vec<_>>()
            .await;

    let mut errors: Vec<SearchError> = vec![];
    let mut mangas: Vec<_> = source_results
        .into_iter()
        .flat_map(|(results, error)| {
            if let Some(error) = error {
                errors.push(error);
            }

            let SourceMangaSearchResults {
                mangas,
                source_information,
            } = results;

            mangas.into_iter().map(move |(manga, option_tuple)| {
                let (unread_count, last_read, in_library) =
                    option_tuple.unwrap_or((None, None, false));

                Manga {
                    source_information: source_information.clone(),
                    information: manga,
                    state: MangaState::default(),
                    unread_chapters_count: unread_count,
                    last_read,
                    in_library,
                }
            })
        })
        .collect();

    mangas.sort_by_cached_key(|manga| {
        manga
            .information
            .title
            .clone()
            .unwrap_or_default()
            .nfkc()
            .flat_map(char::to_lowercase)
            .collect::<String>()
    });

    Ok((mangas, errors))
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an error occurred while fetching search results from the source")]
    SourceError(#[source] anyhow::Error),
}

type ResultManga = (MangaInformation, Option<(Option<usize>, Option<i64>, bool)>);
struct SourceMangaSearchResults {
    source_information: SourceInformation,
    /// mangas: Vec<Manga>, $0 is unread chapters count, $1 is last read time
    mangas: Vec<ResultManga>,
}
