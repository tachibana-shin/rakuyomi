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
    all_chapters: Vec<ChapterInformation>,
    filter: Filter,
    langs: &[&str],
) -> Result<Vec<ChapterInformation>> {
    let mut last_read_position: Option<ChapterPosition> = None;
    let target_scanlator = match &filter {
        Filter::ScanlatorChapters { scanlator, .. } => Some(scanlator.clone()),
        _ => None,
    };

    let use_lang_filter = !langs.is_empty();

    // Chapters are stored in source order (newest first). Numbered chapters
    // are compared by their real chapter_number; unnumbered chapters have no
    // meaningful number, so they're compared by source position instead. The
    // two spaces aren't comparable to each other, so any comparison spanning
    // a numbered and an unnumbered chapter falls back to source order too.
    // This matters for every LNReader chapter (LNReader never provides a
    // real chapter_number) and for the Aidoku sources that don't either --
    // comparing a naive positional stand-in directly against real
    // chapter_number values broke "download unread" for exactly that mix
    // (see `ChapterPosition::is_at_or_before`'s doc comment for the
    // confirmed-live failure case).
    let chapter_position = |chapter: &ChapterInformation, index: usize| ChapterPosition {
        index,
        number: chapter.chapter_number.map(ordered_float::OrderedFloat),
    };
    let chapter_group = |chapter: &ChapterInformation, index: usize| {
        chapter
            .chapter_number
            .map(ordered_float::OrderedFloat)
            .map(ChapterGroup::Numbered)
            .unwrap_or(ChapterGroup::Unnumbered(index))
    };

    // Batch-fetch all chapter states for this manga in a single query
    let manga_id = all_chapters.first().map(|c| c.id.manga_id().clone());
    let chapter_states = if let Some(id) = manga_id {
        db.find_chapter_states_for_manga(&id).await?
    } else {
        HashMap::new()
    };

    // Starting from the newest chapter (in source order), find out the first one marked as read.
    for (index, chapter) in all_chapters.iter().enumerate() {
        // Filter: language
        if use_lang_filter {
            let ch_lang = chapter.lang.as_deref().unwrap_or("unknown");
            if !langs.contains(&ch_lang) {
                continue;
            }
        }

        // Skip chapters that don't match our target scanlator (if filtering by scanlator)
        if let Some(ref target_scanlator) = target_scanlator {
            let chapter_scanlator = chapter.scanlator.as_deref().unwrap_or("Unknown");
            if chapter_scanlator != target_scanlator {
                continue;
            }
        }

        let read = chapter_states
            .get(chapter.id.value())
            .is_some_and(|state| state.read);

        if read {
            last_read_position = Some(chapter_position(chapter, index));

            break;
        }
    }

    // In reverse source order (oldest-to-newest), find out which unread chapters to download.
    // Keep the `(index, chapter)` pair alive here: both the read-boundary check above and
    // `Filter::NextUnreadChapters` below need the index -- for `chapter_group`, so chapters
    // without a real `chapter_number` are deduplicated by position instead of all collapsing
    // onto the same group and exhausting the batch quota in one entry.
    let unread_chapters = all_chapters
        .into_iter()
        .enumerate()
        .rev()
        .filter(move |(_, chapter)| {
            if use_lang_filter {
                let ch_lang = chapter.lang.as_deref().unwrap_or("unknown");
                if !langs.contains(&ch_lang) {
                    return false;
                }
            }
            true
        })
        .skip_while(move |(index, chapter)| {
            last_read_position.is_some_and(|boundary| {
                chapter_position(chapter, *index).is_at_or_before(&boundary)
            })
        });

    let filtered_chapters: Vec<_> = match filter {
        Filter::AllUnreadChapters => unread_chapters.map(|(_, chapter)| chapter).collect(),
        Filter::NextUnreadChapters(amount) => {
            let mut seen_chapter_numbers = HashSet::new();

            unread_chapters
                .take_while(|(index, chapter)| {
                    seen_chapter_numbers.insert(chapter_group(chapter, *index));

                    seen_chapter_numbers.len() <= amount
                })
                .map(|(_, chapter)| chapter)
                .collect()
        }
        Filter::ScanlatorChapters { scanlator, amount } => {
            // Filter by scanlator first
            let scanlator_chapters: Vec<_> = unread_chapters
                .filter(|(_, chapter)| {
                    chapter
                        .scanlator
                        .as_ref()
                        .map(|s| s == &scanlator)
                        .unwrap_or(scanlator == "Unknown")
                })
                .map(|(_, chapter)| chapter)
                .collect();

            // Then limit by amount if specified
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

#[derive(Hash, Eq, PartialEq)]
enum ChapterGroup {
    Numbered(ordered_float::OrderedFloat<f32>),
    Unnumbered(usize),
}

#[derive(Clone, Copy)]
struct ChapterPosition {
    index: usize,
    number: Option<ordered_float::OrderedFloat<f32>>,
}

impl ChapterPosition {
    /// Returns true if `self` is at the same position as, or older (further
    /// from the newest chapter) than, `boundary` in source order. Numbered
    /// chapters compare by their real chapter_number; when either side has
    /// no number, source position is used instead, since chapter numbers
    /// and source-order positions aren't comparable to each other.
    fn is_at_or_before(&self, boundary: &ChapterPosition) -> bool {
        match (self.number, boundary.number) {
            (Some(a), Some(b)) => a <= b,
            _ => self.index >= boundary.index,
        }
    }
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
    async fn all_un_numbered_chapters_stop_at_read_boundary() {
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

        assert_eq!(
            filtered
                .iter()
                .map(|c| c.id.value().as_str())
                .collect::<Vec<_>>(),
            vec!["chapter-1", "chapter-0"]
        );
    }

    #[tokio::test]
    async fn mixed_numbered_and_unnumbered_chapters_keep_numbered_duplicates_grouped() {
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

        assert_eq!(
            filtered
                .iter()
                .map(|c| c.id.value().as_str())
                .collect::<Vec<_>>(),
            vec!["chapter-4", "chapter-3", "chapter-2"]
        );
    }

    #[tokio::test]
    async fn unnumbered_chapter_newer_than_read_boundary_is_not_skipped() {
        // Non-contiguous, large chapter numbers so a source-position-derived
        // stand-in for the unnumbered chapter would fall well below the read
        // chapter's real number, wrongly looking "older" than it.
        let (_tmp_dir, db, manga_id) = test_db().await;
        let chapters = vec![
            chapter(&manga_id, 0, Some(100.0)),
            chapter(&manga_id, 1, None),
            chapter(&manga_id, 2, Some(50.0)),
            chapter(&manga_id, 3, None),
            chapter(&manga_id, 4, Some(10.0)),
        ];
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

        assert_eq!(
            filtered
                .iter()
                .map(|c| c.id.value().as_str())
                .collect::<Vec<_>>(),
            vec!["chapter-1", "chapter-0"]
        );
    }

    #[tokio::test]
    async fn next_unread_chapters_respects_amount_for_unnumbered_chapters() {
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

        assert_eq!(
            filtered
                .iter()
                .map(|c| c.id.value().as_str())
                .collect::<Vec<_>>(),
            vec!["chapter-5", "chapter-4"]
        );
    }

    #[tokio::test]
    async fn next_unread_chapters_respects_amount_without_chapter_numbers() {
        let (_tmp_dir, db, manga_id) = test_db().await;

        // 6 chapters, newest first (index 0 = newest, matching the source-order
        // assumption in `apply_chapter_filter`), none carrying a `chapter_number`
        // -- the LNReader/JS-source shape.
        let chapters: Vec<_> = (0..6).map(|i| chapter(&manga_id, i, None)).collect();
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();

        // Mark the two oldest chapters (indices 4 and 5 of the newest-first
        // order) as read.
        for chapter in &chapters[4..] {
            db.upsert_chapter_state(
                &chapter.id,
                ChapterState {
                    read: true,
                    last_read: None,
                },
            )
            .await
            .unwrap();
        }

        let all_chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();

        let filtered = apply_chapter_filter(&db, all_chapters, Filter::NextUnreadChapters(2), &[])
            .await
            .unwrap();

        // Regression: the quota used to compare `chapter_number.unwrap_or_default()`
        // (= 0.0) for every unnumbered chapter, so all four unread chapters
        // collided on the same key and the whole batch was returned instead of
        // exactly the two next unread ones.
        assert_eq!(filtered.len(), 2);
        assert_eq!(
            filtered
                .iter()
                .map(|c| c.id.value().as_str())
                .collect::<Vec<_>>(),
            vec!["chapter-3", "chapter-2"]
        );
    }
}
