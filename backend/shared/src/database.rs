use std::{collections::HashSet, path::Path};

use anyhow::Result;
use futures::{stream, StreamExt, TryStreamExt};
use sqlx::{sqlite::SqliteConnectOptions, Pool, QueryBuilder, Sqlite};
use url::Url;

use crate::{
    model::{
        ChapterId, ChapterInformation, ChapterState, Manga, MangaId, MangaInformation, MangaState,
        SourceId, SourceInformation,
    },
    source_collection::SourceCollection,
};

pub struct Database {
    pool: Pool<Sqlite>,
}

const BIND_LIMIT: usize = 32766;

// FIXME add proper error handling
impl Database {
    pub async fn new(filename: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(filename)
            .create_if_missing(true);
        let pool = Pool::connect_with(options).await?;

        sqlx::migrate!().run(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn get_manga_library(&self) -> Vec<MangaId> {
        let rows = sqlx::query_as!(
            MangaLibraryRow,
            r#"
                SELECT * FROM manga_library;
            "#
        )
        .fetch_all(&self.pool)
        .await
        .unwrap();

        rows.into_iter().map(|row| row.manga_id()).collect()
    }

    pub async fn get_manga_library_with_read_count(
        &self,
        source_collection: &impl SourceCollection,
        library_sorting_mode: &crate::settings::LibrarySortingMode,
    ) -> Result<Vec<Manga>> {
        let rows = match library_sorting_mode {
            &crate::settings::LibrarySortingMode::Ascending => {
                sqlx::query_as!(
                    MangaLibraryRowWithReadCount,
                    r#"
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COALESCE((
                            SELECT COUNT(*)
                            FROM chapter_informations ci
                            WHERE ci.source_id = ml.source_id
                            AND ci.manga_id = ml.manga_id
                            AND (
                                ms.preferred_scanlator IS NULL
                                OR ci.scanlator = ms.preferred_scanlator
                                OR ci.scanlator IS NULL
                            )
                            AND ci.chapter_number > COALESCE((
                                SELECT MAX(ci2.chapter_number)
                                FROM chapter_informations ci2
                                JOIN chapter_state cs2
                                    ON ci2.source_id = cs2.source_id
                                AND ci2.manga_id = cs2.manga_id
                                AND ci2.chapter_id = cs2.chapter_id
                                WHERE ci2.source_id = ml.source_id
                                    AND ci2.manga_id = ml.manga_id
                                    AND (
                                        ms.preferred_scanlator IS NULL
                                        OR ci2.scanlator = ms.preferred_scanlator
                                        OR ci2.scanlator IS NULL
                                    )
                                    AND cs2.read = 1
                            ), -1)
                        ), 0) AS unread_chapters_count
                    FROM manga_library AS ml
                    JOIN manga_informations AS mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state AS ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    ORDER BY ml.rowid
                    "#
                )
                .fetch_all(&self.pool)
                .await?
            }
            &crate::settings::LibrarySortingMode::Descending => {
                sqlx::query_as!(
                    MangaLibraryRowWithReadCount,
                    r#"
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COALESCE((
                            SELECT COUNT(*)
                            FROM chapter_informations ci
                            WHERE ci.source_id = ml.source_id
                            AND ci.manga_id = ml.manga_id
                            AND (
                                ms.preferred_scanlator IS NULL
                                OR ci.scanlator = ms.preferred_scanlator
                                OR ci.scanlator IS NULL
                            )
                            AND ci.chapter_number > COALESCE((
                                SELECT MAX(ci2.chapter_number)
                                FROM chapter_informations ci2
                                JOIN chapter_state cs2
                                    ON ci2.source_id = cs2.source_id
                                AND ci2.manga_id = cs2.manga_id
                                AND ci2.chapter_id = cs2.chapter_id
                                WHERE ci2.source_id = ml.source_id
                                    AND ci2.manga_id = ml.manga_id
                                    AND (
                                        ms.preferred_scanlator IS NULL
                                        OR ci2.scanlator = ms.preferred_scanlator
                                        OR ci2.scanlator IS NULL
                                    )
                                    AND cs2.read = 1
                            ), -1)
                        ), 0) AS unread_chapters_count
                    FROM manga_library AS ml
                    JOIN manga_informations AS mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state AS ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    ORDER BY ml.rowid DESC
                    "#
                )
                .fetch_all(&self.pool)
                .await?
            }
            &crate::settings::LibrarySortingMode::TitleAsc => {
                sqlx::query_as!(
                    MangaLibraryRowWithReadCount,
                    r#"
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COALESCE((
                            SELECT COUNT(*)
                            FROM chapter_informations ci
                            WHERE ci.source_id = ml.source_id
                            AND ci.manga_id = ml.manga_id
                            AND (
                                ms.preferred_scanlator IS NULL
                                OR ci.scanlator = ms.preferred_scanlator
                                OR ci.scanlator IS NULL
                            )
                            AND ci.chapter_number > COALESCE((
                                SELECT MAX(ci2.chapter_number)
                                FROM chapter_informations ci2
                                JOIN chapter_state cs2
                                    ON ci2.source_id = cs2.source_id
                                AND ci2.manga_id = cs2.manga_id
                                AND ci2.chapter_id = cs2.chapter_id
                                WHERE ci2.source_id = ml.source_id
                                    AND ci2.manga_id = ml.manga_id
                                    AND (
                                        ms.preferred_scanlator IS NULL
                                        OR ci2.scanlator = ms.preferred_scanlator
                                        OR ci2.scanlator IS NULL
                                    )
                                    AND cs2.read = 1
                            ), -1)
                        ), 0) AS unread_chapters_count
                    FROM manga_library AS ml
                    JOIN manga_informations AS mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state AS ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    ORDER BY mi.title COLLATE NOCASE ASC
                    "#
                )
                .fetch_all(&self.pool)
                .await?
            }
            &crate::settings::LibrarySortingMode::TitleDesc => {
                sqlx::query_as!(
                    MangaLibraryRowWithReadCount,
                    r#"
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COALESCE((
                            SELECT COUNT(*)
                            FROM chapter_informations ci
                            WHERE ci.source_id = ml.source_id
                            AND ci.manga_id = ml.manga_id
                            AND (
                                ms.preferred_scanlator IS NULL
                                OR ci.scanlator = ms.preferred_scanlator
                                OR ci.scanlator IS NULL
                            )
                            AND ci.chapter_number > COALESCE((
                                SELECT MAX(ci2.chapter_number)
                                FROM chapter_informations ci2
                                JOIN chapter_state cs2
                                    ON ci2.source_id = cs2.source_id
                                AND ci2.manga_id = cs2.manga_id
                                AND ci2.chapter_id = cs2.chapter_id
                                WHERE ci2.source_id = ml.source_id
                                    AND ci2.manga_id = ml.manga_id
                                    AND (
                                        ms.preferred_scanlator IS NULL
                                        OR ci2.scanlator = ms.preferred_scanlator
                                        OR ci2.scanlator IS NULL
                                    )
                                    AND cs2.read = 1
                            ), -1)
                        ), 0) AS unread_chapters_count
                    FROM manga_library AS ml
                    JOIN manga_informations AS mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state AS ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    ORDER BY mi.title COLLATE NOCASE DESC
                    "#
                )
                .fetch_all(&self.pool)
                .await?
            }
            &crate::settings::LibrarySortingMode::UnreadAsc => {
                sqlx::query_as!(
                    MangaLibraryRowWithReadCount,
                    r#"
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COALESCE((
                            SELECT COUNT(*)
                            FROM chapter_informations ci
                            WHERE ci.source_id = ml.source_id
                            AND ci.manga_id = ml.manga_id
                            AND (
                                ms.preferred_scanlator IS NULL
                                OR ci.scanlator = ms.preferred_scanlator
                                OR ci.scanlator IS NULL
                            )
                            AND ci.chapter_number > COALESCE((
                                SELECT MAX(ci2.chapter_number)
                                FROM chapter_informations ci2
                                JOIN chapter_state cs2
                                    ON ci2.source_id = cs2.source_id
                                AND ci2.manga_id = cs2.manga_id
                                AND ci2.chapter_id = cs2.chapter_id
                                WHERE ci2.source_id = ml.source_id
                                    AND ci2.manga_id = ml.manga_id
                                    AND (
                                        ms.preferred_scanlator IS NULL
                                        OR ci2.scanlator = ms.preferred_scanlator
                                        OR ci2.scanlator IS NULL
                                    )
                                    AND cs2.read = 1
                            ), -1)
                        ), 0) AS unread_chapters_count
                    FROM manga_library AS ml
                    JOIN manga_informations AS mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state AS ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    ORDER BY unread_chapters_count ASC
                    "#
                )
                .fetch_all(&self.pool)
                .await?
            }
            &crate::settings::LibrarySortingMode::UnreadDesc => {
                sqlx::query_as!(
                    MangaLibraryRowWithReadCount,
                    r#"
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COALESCE((
                            SELECT COUNT(*)
                            FROM chapter_informations ci
                            WHERE ci.source_id = ml.source_id
                            AND ci.manga_id = ml.manga_id
                            AND (
                                ms.preferred_scanlator IS NULL
                                OR ci.scanlator = ms.preferred_scanlator
                                OR ci.scanlator IS NULL
                            )
                            AND ci.chapter_number > COALESCE((
                                SELECT MAX(ci2.chapter_number)
                                FROM chapter_informations ci2
                                JOIN chapter_state cs2
                                    ON ci2.source_id = cs2.source_id
                                AND ci2.manga_id = cs2.manga_id
                                AND ci2.chapter_id = cs2.chapter_id
                                WHERE ci2.source_id = ml.source_id
                                    AND ci2.manga_id = ml.manga_id
                                    AND (
                                        ms.preferred_scanlator IS NULL
                                        OR ci2.scanlator = ms.preferred_scanlator
                                        OR ci2.scanlator IS NULL
                                    )
                                    AND cs2.read = 1
                            ), -1)
                        ), 0) AS unread_chapters_count
                    FROM manga_library AS ml
                    JOIN manga_informations AS mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state AS ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    ORDER BY unread_chapters_count DESC
                    "#
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        let mangas = rows
            .into_iter()
            .filter_map(|row| {
                let source = source_collection.get_by_id(&SourceId::new(row.source_id.clone()))?;
                let info = MangaInformation {
                    id: MangaId::from_strings(row.source_id, row.manga_id),
                    title: row.title,
                    author: row.author,
                    artist: row.artist,
                    cover_url: row.cover_url.and_then(|url| Url::parse(&url).ok()),
                };

                Some(Manga {
                    source_information: SourceInformation::from(source.manifest()),
                    information: info,
                    state: MangaState::default(),
                    unread_chapters_count: row.unread_chapters_count.map(|v| v as usize),
                })
            })
            .collect();

        Ok(mangas)
    }

    pub async fn add_manga_to_library(&self, manga_id: MangaId) {
        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        sqlx::query!(
            r#"
                INSERT INTO manga_library (source_id, manga_id)
                VALUES (?1, ?2)
                ON CONFLICT DO NOTHING
            "#,
            source_id,
            manga_id
        )
        .execute(&self.pool)
        .await
        .unwrap();
    }

    pub async fn remove_manga_from_library(&self, manga_id: MangaId) {
        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        sqlx::query!(
            r#"
                DELETE FROM manga_library
                WHERE source_id = ?1 AND manga_id = ?2
            "#,
            source_id,
            manga_id
        )
        .execute(&self.pool)
        .await
        .unwrap();
    }

    pub async fn count_unread_chapters(&self, manga_id: &MangaId) -> Option<usize> {
        // Get preferred scanlator if it exists
        let preferred_scanlator = self
            .find_manga_state(manga_id)
            .await
            .and_then(|state| state.preferred_scanlator);

        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        let row = sqlx::query_as!(
            UnreadChaptersRow,
            r#"
                SELECT COUNT(*) as count,
                    EXISTS(SELECT 1 FROM chapter_informations 
                            WHERE source_id = ?1 AND manga_id = ?2 
                            AND (?3 IS NULL OR scanlator = ?3 OR scanlator IS NULL)) AS "has_chapters: bool"
                FROM chapter_informations ci
                LEFT JOIN chapter_state cs 
                    ON ci.source_id = cs.source_id 
                    AND ci.manga_id = cs.manga_id 
                    AND ci.chapter_id = cs.chapter_id
                WHERE ci.source_id = ?1 
                    AND ci.manga_id = ?2 
                    AND (?3 IS NULL OR ci.scanlator = ?3 OR ci.scanlator IS NULL)
                    AND ci.chapter_number > COALESCE(
                        (SELECT MAX(ci2.chapter_number) 
                        FROM chapter_informations ci2 
                        JOIN chapter_state cs2 
                            ON ci2.source_id = cs2.source_id 
                            AND ci2.manga_id = cs2.manga_id 
                            AND ci2.chapter_id = cs2.chapter_id
                        WHERE ci2.source_id = ?1 
                            AND ci2.manga_id = ?2 
                            AND (?3 IS NULL OR ci2.scanlator = ?3 OR ci2.scanlator IS NULL)
                            AND cs2.read = 1
                        ), -1
                    )
            "#,
            source_id, manga_id, preferred_scanlator
        )
        .fetch_one(&self.pool)
        .await
        .unwrap();

        if !row.has_chapters.unwrap_or(false) {
            return None;
        }

        row.count.map(|count| count.try_into().unwrap())
    }

    pub async fn find_cached_manga_information(
        &self,
        manga_id: &MangaId,
    ) -> Option<MangaInformation> {
        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        let maybe_row = sqlx::query_as!(
            MangaInformationsRow,
            r#"
                SELECT * FROM manga_informations
                    WHERE source_id = ?1 AND manga_id = ?2;
            "#,
            source_id,
            manga_id
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap();

        maybe_row.map(|row| row.into())
    }

    pub async fn find_cached_chapter_information(
        &self,
        chapter_id: &ChapterId,
    ) -> Option<ChapterInformation> {
        let source_id = chapter_id.source_id().value();
        let manga_id = chapter_id.manga_id().value();
        let chapter_id = chapter_id.value();

        let maybe_row = sqlx::query_as!(
            ChapterInformationsRow,
            r#"
                SELECT * FROM chapter_informations
                WHERE source_id = ?1 AND manga_id = ?2 AND chapter_id = ?3;
            "#,
            source_id,
            manga_id,
            chapter_id
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap();

        maybe_row.map(|row| row.into())
    }

    pub async fn find_cached_chapter_informations(
        &self,
        manga_id: &MangaId,
    ) -> Vec<ChapterInformation> {
        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        let rows = sqlx::query_as!(
            ChapterInformationsRow,
            r#"
                SELECT * FROM chapter_informations
                WHERE source_id = ?1 AND manga_id = ?2
                ORDER BY manga_order ASC;
            "#,
            source_id,
            manga_id
        )
        .fetch_all(&self.pool)
        .await
        .unwrap();

        rows.into_iter().map(|row| row.into()).collect()
    }

    pub async fn upsert_cached_manga_information(&self, manga_information: MangaInformation) {
        let source_id = manga_information.id.source_id().value();
        let manga_id = manga_information.id.value();
        let cover_url = manga_information.cover_url.map(|url| url.to_string());

        sqlx::query!(
            r#"
                INSERT INTO manga_informations (source_id, manga_id, title, author, artist, cover_url)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT DO UPDATE SET
                    title = excluded.title,
                    author = excluded.author,
                    artist = excluded.artist,
                    cover_url = excluded.cover_url
            "#,
            source_id,
            manga_id,
            manga_information.title,
            manga_information.author,
            manga_information.artist,
            cover_url
        ).execute(&self.pool).await.unwrap();
    }

    pub async fn upsert_cached_chapter_informations(
        &self,
        manga_id: &MangaId,
        chapter_informations: Vec<ChapterInformation>,
    ) {
        // We need to both update the existing information about the chapters, and delete the chapters that are no longer present
        let cached_chapter_ids: HashSet<_> = self
            .find_cached_chapter_informations(manga_id)
            .await
            .into_iter()
            .map(|information| information.id)
            .collect();

        let chapter_ids: HashSet<_> = chapter_informations
            .iter()
            .map(|information| information.id.clone())
            .collect();

        let removed_chapter_ids: Vec<_> =
            (&cached_chapter_ids - &chapter_ids).into_iter().collect();

        // We use 2 binds to place the `source_id` and `manga_id` on the query.
        let remove_chapters_query_available_binds = BIND_LIMIT - 2;

        stream::iter(removed_chapter_ids.chunks(remove_chapters_query_available_binds))
            .then(|chapter_ids| async move {
                let mut builder = QueryBuilder::new("DELETE FROM chapter_informations WHERE ");

                builder
                    .push(" source_id = ")
                    .push_bind(manga_id.source_id().value())
                    .push(" AND manga_id = ")
                    .push_bind(manga_id.value())
                    .push(" AND chapter_id IN ")
                    .push_tuples(chapter_ids, |mut b, chapter_id| {
                        b.push_bind(chapter_id.value());
                    });

                builder.build().execute(&self.pool).await?;

                anyhow::Ok(())
            })
            .try_collect::<()>()
            .await
            .unwrap();

        let insert_field_count = 8;
        stream::iter(chapter_informations.into_iter().enumerate().collect::<Vec<_>>().chunks(BIND_LIMIT / insert_field_count))
            .then(|enumerated_chapter_informations| async move {
                let mut builder = QueryBuilder::new(
                    "INSERT INTO chapter_informations (source_id, manga_id, chapter_id, manga_order, title, scanlator, chapter_number, volume_number) "
                );

                builder
                    .push_values(enumerated_chapter_informations, |mut b, (index, chapter_information)| {
                        let index = *index as i64;
                        let source_id = chapter_information.id.source_id().value();
                        let manga_id = chapter_information.id.manga_id().value();
                        let chapter_id = chapter_information.id.value();

                        let chapter_number = chapter_information
                            .chapter_number
                            .map(|dec| f64::try_from(dec).unwrap());
                        let volume_number = chapter_information
                            .volume_number
                            .map(|dec| f64::try_from(dec).unwrap());

                        b.push_bind(source_id)
                            .push_bind(manga_id)
                            .push_bind(chapter_id)
                            .push_bind(index)
                            .push_bind(chapter_information.title.clone())
                            .push_bind(chapter_information.scanlator.clone())
                            .push_bind(chapter_number)
                            .push_bind(volume_number);
                    })
                    .push(r#"
                        ON CONFLICT DO UPDATE SET
                        manga_order = excluded.manga_order,
                        title = excluded.title,
                        scanlator = excluded.scanlator,
                        chapter_number = excluded.chapter_number,
                        volume_number = excluded.volume_number
                    "#);

                builder.build().execute(&self.pool).await?;

                anyhow::Ok(())
            })
            .try_collect::<()>().await.unwrap();
    }

    pub async fn find_manga_state(&self, manga_id: &MangaId) -> Option<MangaState> {
        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        let maybe_row = sqlx::query_as!(
            MangaStateRow,
            r#"
                SELECT source_id, manga_id, preferred_scanlator 
                FROM manga_state
                WHERE source_id = ?1 AND manga_id = ?2;
            "#,
            source_id,
            manga_id,
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap();

        maybe_row.map(|row| row.into())
    }

    pub async fn upsert_manga_state(&self, manga_id: &MangaId, state: MangaState) {
        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        sqlx::query!(
            r#"
                INSERT INTO manga_state (source_id, manga_id, preferred_scanlator)
                VALUES (?1, ?2, ?3)
                ON CONFLICT DO UPDATE SET
                    preferred_scanlator = excluded.preferred_scanlator
            "#,
            source_id,
            manga_id,
            state.preferred_scanlator,
        )
        .execute(&self.pool)
        .await
        .unwrap();
    }

    pub async fn find_chapter_state(&self, chapter_id: &ChapterId) -> Option<ChapterState> {
        let source_id = chapter_id.source_id().value();
        let manga_id = chapter_id.manga_id().value();
        let chapter_id = chapter_id.value();

        // FIXME we should be able to just specify a override for the `read` field here,
        // but there's a bug in sqlx preventing us: https://github.com/launchbadge/sqlx/issues/2295
        let maybe_row = sqlx::query_as!(
            ChapterStateRow,
            r#"
                SELECT source_id, manga_id, chapter_id, read AS "read: bool" FROM chapter_state
                WHERE source_id = ?1 AND manga_id = ?2 AND chapter_id = ?3;
            "#,
            source_id,
            manga_id,
            chapter_id,
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap();

        maybe_row.map(|row| row.into())
    }

    pub async fn upsert_chapter_state(&self, chapter_id: &ChapterId, state: ChapterState) {
        let source_id = chapter_id.source_id().value();
        let manga_id = chapter_id.manga_id().value();
        let chapter_id = chapter_id.value();

        sqlx::query!(
            r#"
                INSERT INTO chapter_state (source_id, manga_id, chapter_id, read)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT DO UPDATE SET
                    read = excluded.read
            "#,
            source_id,
            manga_id,
            chapter_id,
            state.read,
        )
        .execute(&self.pool)
        .await
        .unwrap();
    }
}

/// Represents a manga entry in the user's library, joined with its information
/// and the computed number of unread chapters.
#[derive(sqlx::FromRow)]
pub struct MangaLibraryRowWithReadCount {
    /// ID of the source (e.g., MangaDex, NHentai, etc.)
    pub source_id: String,

    /// ID of the manga within the source
    pub manga_id: String,

    /// Manga title (nullable in DB)
    pub title: Option<String>,

    /// Author name (nullable in DB)
    pub author: Option<String>,

    /// Artist name (nullable in DB)
    pub artist: Option<String>,

    /// Cover image URL (nullable in DB)
    pub cover_url: Option<String>,

    /// Number of unread chapters (computed via COUNT)
    /// Compatible sqlx but never None in practice
    pub unread_chapters_count: Option<i32>,
}

#[derive(sqlx::FromRow)]
struct MangaInformationsRow {
    source_id: String,
    manga_id: String,
    title: Option<String>,
    author: Option<String>,
    artist: Option<String>,
    cover_url: Option<String>,
}

impl From<MangaInformationsRow> for MangaInformation {
    fn from(value: MangaInformationsRow) -> Self {
        Self {
            id: MangaId::from_strings(value.source_id, value.manga_id),
            title: value.title,
            author: value.author,
            artist: value.artist,
            cover_url: value
                .cover_url
                .map(|url_string| url_string.as_str().try_into().unwrap()),
        }
    }
}

#[derive(sqlx::FromRow)]
struct ChapterInformationsRow {
    source_id: String,
    manga_id: String,
    chapter_id: String,
    #[allow(dead_code)]
    manga_order: i64,
    title: Option<String>,
    scanlator: Option<String>,
    chapter_number: Option<f64>,
    volume_number: Option<f64>,
}

impl From<ChapterInformationsRow> for ChapterInformation {
    fn from(value: ChapterInformationsRow) -> Self {
        Self {
            id: ChapterId::from_strings(value.source_id, value.manga_id, value.chapter_id),
            title: value.title,
            scanlator: value.scanlator,
            chapter_number: value
                .chapter_number
                .map(|decimal_as_f64| decimal_as_f64.try_into().unwrap()),
            volume_number: value
                .volume_number
                .map(|decimal_as_f64| decimal_as_f64.try_into().unwrap()),
        }
    }
}

#[derive(sqlx::FromRow)]
struct MangaLibraryRow {
    source_id: String,
    manga_id: String,
}

impl MangaLibraryRow {
    pub fn manga_id(self) -> MangaId {
        MangaId::from_strings(self.source_id, self.manga_id)
    }
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct ChapterStateRow {
    source_id: String,
    manga_id: String,
    chapter_id: String,
    read: bool,
}

impl From<ChapterStateRow> for ChapterState {
    fn from(value: ChapterStateRow) -> Self {
        Self { read: value.read }
    }
}

#[derive(sqlx::FromRow)]
struct UnreadChaptersRow {
    count: Option<i32>,
    has_chapters: Option<bool>,
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct MangaStateRow {
    source_id: String,
    manga_id: String,
    preferred_scanlator: Option<String>,
}

impl From<MangaStateRow> for MangaState {
    fn from(value: MangaStateRow) -> Self {
        Self {
            preferred_scanlator: value.preferred_scanlator,
        }
    }
}
