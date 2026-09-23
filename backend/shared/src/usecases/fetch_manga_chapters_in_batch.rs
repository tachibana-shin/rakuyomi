use anyhow::{Context, Result};
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

        let chapters_to_download = match collect_chapters_to_download(db, &id, filter, langs).await {
            Ok(v) => v,
            Err(e) => {
                yield ProgressReport::Errored(e);
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
    // Sort oldest-to-newest by chapter number so the sorted position is a
    // stable, monotonic sequence. Unnumbered chapters (None) sort before
    // every numbered chapter — including Some(0.0) — so they can never sit
    // after a read chapter in the sorted order. slice::sort_by is stable,
    // so equal-ranked unnumbered chapters keep their DB order.
    all_chapters.sort_by(|a, b| match (&a.chapter_number, &b.chapter_number) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(a), Some(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
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
        .iter()
        .cloned()
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
        Filter::SpecificChapters(text) => {
            let ranges = parse_specific_chapter_ranges(&text)?;

            all_chapters
                .into_iter()
                .filter(|chapter| {
                    if use_lang_filter {
                        let ch_lang = chapter.lang.as_deref().unwrap_or("unknown");
                        if !langs.contains(&ch_lang) {
                            return false;
                        }
                    }

                    chapter
                        .chapter_number
                        .map(|number| {
                            ranges
                                .iter()
                                .any(|(start, stop)| number >= *start && number <= *stop)
                        })
                        .unwrap_or(false)
                })
                .collect()
        }
    };

    Ok(filtered_chapters)
}

/// Fetches the cached chapters for `id` and applies `filter`, turning the
/// "selection is empty" case into a `Error::Other` instead of a successful
/// zero-chapter download (which used to surface as a fake "Download
/// complete!" in the UI). Kept separate from the stream so the failure mode
/// can be tested without constructing a [`Source`].
async fn collect_chapters_to_download(
    db: &Database,
    id: &MangaId,
    filter: Filter,
    langs: &[&str],
) -> Result<Vec<ChapterInformation>, Error> {
    let all_chapters = db
        .find_cached_chapter_informations(id)
        .await
        .map_err(Error::Other)?;
    // Capture the message before `filter` is moved into `apply_chapter_filter`.
    let no_chapters_message = no_chapters_error_message(&filter);
    let chapters = apply_chapter_filter(db, all_chapters, filter, langs)
        .await
        .map_err(Error::Other)?;

    if chapters.is_empty() {
        return Err(Error::NoChapters(no_chapters_message));
    }

    Ok(chapters)
}

/// Parses a user-supplied chapter spec such as `"1-4, 10, 12"` into inclusive
/// `(start, stop)` float ranges. A bare number like `10` becomes `(10, 10)`.
///
/// Chapter numbers are stored as `f32`, so the bounds are parsed as `f32` too —
/// casting the stored `f32` to `f64` would make exact matches like `10.1` fail
/// for values that are not exactly representable in binary.
///
/// Malformed input is rejected instead of silently selecting nothing: empty
/// components (`","`, `"1,,2"`), a completely empty selector, and non-finite
/// endpoints (`NaN`, `inf`) are all errors.
fn parse_specific_chapter_ranges(text: &str) -> anyhow::Result<Vec<(f32, f32)>> {
    let mut ranges = Vec::new();

    for part in text.split(',').map(str::trim) {
        if part.is_empty() {
            anyhow::bail!("chapter selector contains an empty part");
        }

        if let Some((start_raw, stop_raw)) = part.split_once('-') {
            let start: f32 = start_raw.trim().parse().context("invalid range start")?;
            let stop: f32 = stop_raw.trim().parse().context("invalid range stop")?;

            if !start.is_finite() || !stop.is_finite() {
                anyhow::bail!("invalid range {part}: endpoints must be finite");
            }
            if start > stop {
                anyhow::bail!("invalid range {part}: start > stop");
            }

            ranges.push((start, stop));
        } else {
            let value: f32 = part.trim().parse().context("invalid chapter number")?;
            if !value.is_finite() {
                anyhow::bail!("invalid chapter number {part}: must be finite");
            }
            ranges.push((value, value));
        }
    }

    if ranges.is_empty() {
        anyhow::bail!("chapter selector is empty");
    }

    Ok(ranges)
}

/// User-facing message used when a filter selects zero chapters. Kept as a
/// pure function so the failure mode (previously a fake "download complete!")
/// can be unit tested without constructing a [`Source`].
fn no_chapters_error_message(filter: &Filter) -> String {
    match filter {
        Filter::NextUnreadChapters(_) | Filter::AllUnreadChapters => {
            "No unread chapters to download. If the manga has no unread chapters, \
            use the specific-chapters download option to re-download already-read \
            ones (e.g. \"1-4, 10, 12\")."
                .to_owned()
        }
        Filter::ScanlatorChapters { scanlator, .. } => {
            format!(
                "No chapters to download from scanlator \"{scanlator}\"; all of its \
                chapters are already read or excluded by the language filter."
            )
        }
        Filter::SpecificChapters(selector) => {
            format!(
                "No chapters match the requested chapter selection \"{selector}\"; \
                check that the chapter numbers exist and are not excluded by the \
                language filter."
            )
        }
    }
}

pub enum Filter {
    NextUnreadChapters(usize),
    AllUnreadChapters,
    ScanlatorChapters {
        scanlator: String,
        amount: Option<usize>,
    },
    /// Download the chapters matching a user-supplied list of chapter ranges,
    /// e.g. "1-4, 10, 12". Only chapters whose `chapter_number` falls inside
    /// any of the parsed ranges are selected.
    SpecificChapters(String),
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
    /// The filter selected zero chapters, carrying the user-facing guidance
    /// message. Kept as a real variant (instead of wrapping it in
    /// `Error::Other`) so the message survives `to_string()` — the job poller
    /// turns the error into a `JobState::Errored` via `e.to_string()`, and
    /// `Error::Other` would surface as the useless literal "unknown error".
    #[error("{0}")]
    NoChapters(String),
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
    async fn unnumbered_chapter_not_selected_after_read_zero() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        // DB order: ch-0(Some(0.0)), ch-1(None).  Without the fix the stable
        // sort places them in DB order [Some(0.0), None], so marking ch-0
        // read sets boundary=0 and ch-1(None) leaks through as "unread".
        // With the fix, None sorts before Some(0.0) → sorted order becomes
        // [ch-1(None), ch-0(Some(0.0))]; marking ch-0 read sets boundary=1,
        // skipping both chapters.
        let chapters = vec![
            chapter(&manga_id, 0, Some(0.0)),
            chapter(&manga_id, 1, None),
        ];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();
        db.upsert_chapter_state(
            &chapters[0].id,
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

        // Nothing should be returned — the unnumbered chapter sorts before
        // the read Some(0.0) and is excluded by the boundary.
        assert!(filtered.is_empty());
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

    #[tokio::test]
    async fn specific_chapters_selects_ranges_and_singles() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        let chapters = vec![
            chapter(&manga_id, 0, Some(1.0)),
            chapter(&manga_id, 1, Some(2.0)),
            chapter(&manga_id, 2, Some(3.0)),
            chapter(&manga_id, 3, Some(4.0)),
            chapter(&manga_id, 4, Some(10.0)),
            chapter(&manga_id, 5, Some(12.0)),
            chapter(&manga_id, 6, Some(20.0)),
        ];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        let filtered = apply_chapter_filter(
            &db,
            chapters,
            Filter::SpecificChapters("1-4, 10, 12".to_owned()),
            &[],
        )
        .await
        .unwrap();

        assert_eq!(
            ids(&filtered),
            vec![
                "chapter-0",
                "chapter-1",
                "chapter-2",
                "chapter-3",
                "chapter-4",
                "chapter-5"
            ]
        );
    }

    #[tokio::test]
    async fn specific_chapters_ignores_read_state() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        let chapters = vec![
            chapter(&manga_id, 0, Some(1.0)),
            chapter(&manga_id, 1, Some(2.0)),
            chapter(&manga_id, 2, Some(3.0)),
        ];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();
        // The newest chapter is marked read, which would empty out the unread
        // list; specific download must still select the requested chapters.
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
        let filtered = apply_chapter_filter(
            &db,
            chapters,
            Filter::SpecificChapters("2-3".to_owned()),
            &[],
        )
        .await
        .unwrap();

        assert_eq!(ids(&filtered), vec!["chapter-1", "chapter-2"]);
    }

    #[tokio::test]
    async fn specific_chapters_invalid_range_returns_error() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        let chapters = vec![chapter(&manga_id, 0, Some(1.0))];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        let result = apply_chapter_filter(
            &db,
            chapters,
            Filter::SpecificChapters("5-2".to_owned()),
            &[],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn specific_chapters_matches_decimal_chapter_numbers() {
        let (_tmp_dir, db, manga_id) = test_db().await;
        // Decimal values that are not exactly representable in binary f32.
        // The selector must match the stored f32 value without widening.
        let chapters = vec![
            chapter(&manga_id, 0, Some(1.1)),
            chapter(&manga_id, 1, Some(2.0)),
            chapter(&manga_id, 2, Some(10.1)),
        ];
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();

        let chapters = db
            .find_cached_chapter_informations(&manga_id)
            .await
            .unwrap();
        let filtered = apply_chapter_filter(
            &db,
            chapters,
            Filter::SpecificChapters("1.1, 1.9-2.1, 10.1".to_owned()),
            &[],
        )
        .await
        .unwrap();

        assert_eq!(ids(&filtered), vec!["chapter-0", "chapter-1", "chapter-2"]);
    }

    #[tokio::test]
    async fn specific_chapters_rejects_malformed_selectors() {
        for selector in [",", "1,,2", "1, ,2", "NaN", "inf", "-inf", ""] {
            let result = parse_specific_chapter_ranges(selector);
            assert!(
                result.is_err(),
                "selector {selector:?} should be rejected, got {result:?}"
            );
        }
    }

    fn ids(chapters: &[ChapterInformation]) -> Vec<&str> {
        chapters.iter().map(|c| c.id.value().as_str()).collect()
    }

    #[test]
    fn no_chapters_error_message_is_explanatory_per_filter() {
        let unread = no_chapters_error_message(&Filter::NextUnreadChapters(5));
        assert!(unread.contains("No unread chapters to download"));
        assert!(unread.contains("1-4, 10, 12"));

        let all = no_chapters_error_message(&Filter::AllUnreadChapters);
        assert!(all.contains("No unread chapters to download"));

        let scanlator = no_chapters_error_message(&Filter::ScanlatorChapters {
            scanlator: "SomeTL".to_owned(),
            amount: None,
        });
        assert!(scanlator.contains("SomeTL"));

        let specific = no_chapters_error_message(&Filter::SpecificChapters("1-4, 10, 12".to_owned()));
        assert!(specific.contains("1-4, 10, 12"));
        assert!(!specific.contains("complete"));
    }

    #[tokio::test]
    async fn empty_selection_errors_instead_of_downloading_nothing() {
        // Regression guard for #347: an empty chapter selection must surface as
        // an error (shown as an error dialog by the UI), never as a successful
        // zero-chapter download.
        let (_tmp_dir, db, manga_id) = test_db().await;
        // All chapters read -> AllUnreadChapters selects nothing.
        let chapters: Vec<_> = (0..3)
            .map(|i| chapter(&manga_id, i, Some(i as f32)))
            .collect();
        db.upsert_cached_chapter_informations(&manga_id, &chapters)
            .await
            .unwrap();
        for c in &chapters {
            db.upsert_chapter_state(
                &c.id,
                ChapterState {
                    read: true,
                    last_read: None,
                },
            )
            .await
            .unwrap();
        }

        for (filter, expected_in_message) in [
            (
                Filter::NextUnreadChapters(10),
                "No unread chapters to download",
            ),
            (Filter::AllUnreadChapters, "No unread chapters to download"),
            (
                Filter::ScanlatorChapters {
                    scanlator: "SomeTL".to_owned(),
                    amount: None,
                },
                "SomeTL",
            ),
            (
                Filter::SpecificChapters("99-100".to_owned()),
                "99-100",
            ),
        ] {
            let result = collect_chapters_to_download(&db, &manga_id, filter, &[]).await;
            match result {
                Err(e) => {
                    // Job-visible string: the poller maps Errored(e) to
                    // JobState::Errored via e.to_string(), so the guidance must
                    // survive Display — not be swallowed by the "unknown error"
                    // literal of the Error::Other variant.
                    let msg = e.to_string();
                    assert!(
                        msg.contains(expected_in_message),
                        "expected {expected_in_message:?} in message, got {msg:?}"
                    );
                    assert!(
                        !msg.contains("complete"),
                        "message must not claim success: {msg:?}"
                    );
                }
                Ok(_) => panic!("expected an error for empty selection"),
            }
        }
    }
}
