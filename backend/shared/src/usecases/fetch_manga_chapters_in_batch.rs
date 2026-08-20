use anyhow::Result;
use async_stream::stream;
use futures::Stream;
use std::collections::{HashMap, HashSet};
use tokio::select;
use tokio_util::sync::CancellationToken;

use crate::{
    chapter_downloader::ensure_chapter_is_in_storage,
    chapter_downloader::Error as ChapterDownloaderError,
    chapter_storage::ChapterStorage,
    database::Database,
    model::{ChapterInformation, MangaId},
    settings::ChapterTitleFormat,
    source::Source,
};

#[allow(clippy::too_many_arguments)]
pub fn fetch_manga_chapters_in_batch<'a>(
    cancellation_token: CancellationToken,
    source: &'a Source,
    db: &'a Database,
    chapter_storage: &'a ChapterStorage,
    id: MangaId,
    filter: Filter,
    langs: &'a [&'a str],
    concurrent_requests_pages: usize,
    optimize_image: bool,
    chapter_title_format: ChapterTitleFormat,
) -> impl Stream<Item = ProgressReport> + 'a {
    stream! {
        let manga = match db.find_cached_manga_information(&id).await {
            Ok(Some(manga)) => manga,
            Ok(None) => {
                yield ProgressReport::Errored(Error::Other(anyhow::anyhow!("Expected manga to be in the database")));
                return;
            }
            Err(e) => {
                yield ProgressReport::Errored(Error::Other(e));
                return;
            }
        };

        let all_chapters = match db.find_cached_chapter_informations(&id).await {
            Ok(v) => v,
            Err(e) => {
                yield ProgressReport::Errored(Error::Other(e));
                return;
            }
        };
        let chapters_to_download = match apply_chapter_filter(db, all_chapters, filter, langs).await {
            Ok(v) => v,
            Err(e) => {
                yield ProgressReport::Errored(Error::Other(e));
                return;
            }
        };

        let total = chapters_to_download.len();
        yield ProgressReport::Progressing { downloaded: 0, total };

        for (index, information) in chapters_to_download.into_iter().enumerate() {
            let ensure_in_storage_result = select! {
                _ = cancellation_token.cancelled() => {
                    yield ProgressReport::Cancelled;

                    return;
                },
                result = ensure_chapter_is_in_storage(
                    &cancellation_token,
                    chapter_storage,
                    source,
                    &manga,
                    &information,
                    concurrent_requests_pages,
                    optimize_image,
                    None,
                    false, // batch download never use RAM
                    None,
                    chapter_title_format,
                ) => result
            };

            match ensure_in_storage_result {
                Ok(_) => yield ProgressReport::Progressing { downloaded: index + 1, total },
                Err(e) => {
                    let error = match e {
                        ChapterDownloaderError::DownloadError(e) => Error::DownloadError(e),
                        ChapterDownloaderError::Other(e) => Error::Other(e),
                    };

                    yield ProgressReport::Errored(error);
                    return;
                },
            }
        };

        yield ProgressReport::Finished;
    }
}

async fn apply_chapter_filter(
    db: &Database,
    mut all_chapters: Vec<ChapterInformation>,
    filter: Filter,
    langs: &[&str],
) -> Result<Vec<ChapterInformation>> {
    // Sort oldest-to-newest by chapter number so the position in the array is
    // a stable, monotonic sequence regardless of numbering scheme. Unnumbered
    // chapters (None → 0) sort to the front; slice::sort_by is stable so they
    // keep their DB order relative to each other.
    all_chapters.sort_by(|a, b| {
        a.chapter_number
            .unwrap_or_default()
            .partial_cmp(&b.chapter_number.unwrap_or_default())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let target_scanlator = match &filter {
        Filter::ScanlatorChapters { scanlator, .. } => Some(scanlator.clone()),
        _ => None,
    };

    let use_lang_filter = !langs.is_empty();

    // Batch-fetch all chapter states for this manga in a single query.
    let manga_id = all_chapters.first().map(|c| c.id.manga_id().clone());
    let chapter_states = if let Some(id) = manga_id {
        db.find_chapter_states_for_manga(&id).await?
    } else {
        HashMap::new()
    };

    // Walk newest-to-oldest (reverse of the sorted order) to locate the
    // highest sorted index where a chapter is marked read. That index is the
    // read boundary — everything at or before it is skipped.
    let mut last_read_boundary: Option<usize> = None;
    for (rev_idx, chapter) in all_chapters.iter().enumerate().rev() {
        if use_lang_filter {
            let ch_lang = chapter.lang.as_deref().unwrap_or("unknown");
            if !langs.contains(&ch_lang) {
                continue;
            }
        }
        if let Some(ref target_scanlator) = target_scanlator {
            let chapter_scanlator = chapter.scanlator.as_deref().unwrap_or("Unknown");
            if chapter_scanlator != target_scanlator {
                continue;
            }
        }

        if chapter_states
            .get(chapter.id.value())
            .is_some_and(|s| s.read)
        {
            last_read_boundary = Some(rev_idx);
            break;
        }
    }

    // Collect unread chapters (oldest-to-newest), skipping everything at or
    // before the read boundary index.
    let unread_chapters: Vec<_> = all_chapters
        .into_iter()
        .enumerate()
        .filter(|(_, chapter)| {
            if use_lang_filter {
                let ch_lang = chapter.lang.as_deref().unwrap_or("unknown");
                if !langs.contains(&ch_lang) {
                    return false;
                }
            }
            true
        })
        .skip_while(move |(index, _)| last_read_boundary.is_some_and(|boundary| *index <= boundary))
        .collect();

    let filtered_chapters: Vec<_> = match filter {
        Filter::AllUnreadChapters => unread_chapters.into_iter().map(|(_, ch)| ch).collect(),
        Filter::NextUnreadChapters(amount) => {
            let mut seen_groups = HashSet::new();

            unread_chapters
                .into_iter()
                .take_while(|(index, chapter)| {
                    seen_groups.insert(chapter_group(chapter, *index));
                    seen_groups.len() <= amount
                })
                .map(|(_, chapter)| chapter)
                .collect()
        }
        Filter::ScanlatorChapters { scanlator, amount } => {
            let scanlator_chapters: Vec<_> = unread_chapters
                .into_iter()
                .filter(|(_, chapter)| {
                    chapter
                        .scanlator
                        .as_ref()
                        .map(|s| s == &scanlator)
                        .unwrap_or(scanlator == "Unknown")
                })
                .map(|(_, chapter)| chapter)
                .collect();

            if let Some(amount) = amount {
                scanlator_chapters.into_iter().take(amount).collect()
            } else {
                scanlator_chapters
            }
        }
    };

    Ok(filtered_chapters)
}

pub enum Filter {
    NextUnreadChapters(usize),
    AllUnreadChapters,
    ScanlatorChapters {
        scanlator: String,
        amount: Option<usize>,
    },
}

/// Grouping key for `NextUnreadChapters` deduplication: numbered chapters
/// with the same value share one group; each unnumbered chapter is distinct
/// (identified by its sorted-array index).
#[derive(Hash, Eq, PartialEq)]
enum ChapterGroup {
    Numbered(ordered_float::OrderedFloat<f32>),
    Unnumbered(usize),
}

fn chapter_group(chapter: &ChapterInformation, index: usize) -> ChapterGroup {
    chapter
        .chapter_number
        .map(ordered_float::OrderedFloat)
        .map(ChapterGroup::Numbered)
        .unwrap_or(ChapterGroup::Unnumbered(index))
}

pub enum ProgressReport {
    Progressing { downloaded: usize, total: usize },
    Finished,
    Cancelled,
    Errored(Error),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an error occurred while downloading all chapters")]
    DownloadError(#[source] anyhow::Error),
    #[error("unknown error")]
    Other(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChapterId, ChapterState};

    fn chapter(manga_id: &MangaId, index: usize, number: Option<f32>) -> ChapterInformation {
        ChapterInformation {
            id: ChapterId::new(manga_id.clone(), format!("chapter-{index}")),
            title: Some(format!("Chapter {index}")),
            scanlator: None,
            chapter_number: number,
            volume_number: None,
            last_updated: None,
            thumbnail: None,
            lang: None,
            url: None,
            locked: None,
        }
    }

    async fn test_db() -> (tempfile::TempDir, Database, MangaId) {
        let tmp_dir = tempfile::tempdir().unwrap();
        let db = Database::new(&tmp_dir.path().join("test.db"))
            .await
            .unwrap();
        let manga_id = MangaId::from_strings("test.source".to_owned(), "manga-1".to_owned());
        (tmp_dir, db, manga_id)
    }

    #[tokio::test]
    async fn all_unnumbered_chapters_stop_at_read_boundary() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        let chapters: Vec<_> = (0..5).map(|i| chapter(&manga_id, i, None)).collect();
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();
        db.upsert_chapter_state(
            &chapters[2].id,
            ChapterState {
                read: true,
                last_read: None,
            },
        )
        .await
        .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        let filtered = apply_chapter_filter(&db, chapters, Filter::AllUnreadChapters, &[])
            .await
            .unwrap();

        // Sorted oldest-to-newest: [ch-0, ch-1, ch-2(R), ch-3, ch-4].
        // Boundary index 2; keep indices 3, 4.
        assert_eq!(ids(&filtered), vec!["chapter-3", "chapter-4"]);
    }

    #[tokio::test]
    async fn mixed_none_and_fractional_read_boundary() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        // Created as: ch-0(None), ch-1(None), ch-2(Some(0.5)), ch-3(Some(1.0)).
        // After sort (oldest-to-newest): [ch-0(None), ch-1(None), ch-2(0.5), ch-3(1.0)].
        let chapters = vec![
            chapter(&manga_id, 0, None),
            chapter(&manga_id, 1, None),
            chapter(&manga_id, 2, Some(0.5)),
            chapter(&manga_id, 3, Some(1.0)),
        ];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();
        // Mark the fractional chapter (sorted index 2) as read.
        db.upsert_chapter_state(
            &chapters[2].id,
            ChapterState {
                read: true,
                last_read: None,
            },
        )
        .await
        .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        let filtered = apply_chapter_filter(&db, chapters, Filter::AllUnreadChapters, &[])
            .await
            .unwrap();

        // Boundary index 2; only ch-3 (1.0) at index 3 is returned.
        // Neither the read chapter nor the older unnumbered ones appear.
        assert_eq!(ids(&filtered), vec!["chapter-3"]);
    }

    #[tokio::test]
    async fn next_unread_chapters_amount_reaches_duplicate_numbered() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        // ch-0(1.0), ch-1(1.0) duplicate, ch-2(2.0), ch-3(3.0).
        // After sort: same order (1.0, 1.0, 2.0, 3.0).
        let chapters = vec![
            chapter(&manga_id, 0, Some(1.0)),
            chapter(&manga_id, 1, Some(1.0)),
            chapter(&manga_id, 2, Some(2.0)),
            chapter(&manga_id, 3, Some(3.0)),
        ];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        // amount=3 so the iterator passes through both duplicates (same group)
        // and reaches ch-2 (second unique group) and ch-3 (third).
        let filtered = apply_chapter_filter(&db, chapters, Filter::NextUnreadChapters(3), &[])
            .await
            .unwrap();

        // First unique group: Numbered(1.0) → ch-0, ch-1.
        // Second unique group: Numbered(2.0) → ch-2.
        // Third unique group: Numbered(3.0) → ch-3.
        // amount=3 keeps all four chapters.
        assert_eq!(
            ids(&filtered),
            vec!["chapter-0", "chapter-1", "chapter-2", "chapter-3"]
        );
    }

    #[tokio::test]
    async fn mixed_numbered_and_unnumbered_keep_numbered_duplicates_grouped() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        let chapters = vec![
            chapter(&manga_id, 0, Some(3.0)),
            chapter(&manga_id, 1, None),
            chapter(&manga_id, 2, Some(2.0)),
            chapter(&manga_id, 3, Some(2.0)),
            chapter(&manga_id, 4, Some(1.0)),
        ];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        let filtered = apply_chapter_filter(&db, chapters, Filter::NextUnreadChapters(2), &[])
            .await
            .unwrap();

        // Sort order: ch-1(None), ch-4(1.0), ch-2(2.0), ch-3(2.0), ch-0(3.0).
        // Group keys:  Unnumbered(1), Numbered(1.0), Numbered(2.0), Numbered(2.0), Numbered(3.0).
        // amount=2 → groups {Unnumbered(1), Numbered(1.0)} → ch-1, ch-4.
        assert_eq!(ids(&filtered), vec!["chapter-1", "chapter-4"]);
    }

    #[tokio::test]
    async fn next_unread_chapters_amount_for_unnumbered() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        let chapters: Vec<_> = (0..6).map(|i| chapter(&manga_id, i, None)).collect();
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        let filtered = apply_chapter_filter(&db, chapters, Filter::NextUnreadChapters(2), &[])
            .await
            .unwrap();

        // Each unnumbered chapter is its own group; first 2 are taken.
        assert_eq!(ids(&filtered), vec!["chapter-0", "chapter-1"]);
    }

    fn ids(chapters: &[ChapterInformation]) -> Vec<&str> {
        chapters.iter().map(|c| c.id.value().as_str()).collect()
    }
}
