use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::Result;
use rust_decimal::{prelude::FromPrimitive, Decimal};
use sqlx::{sqlite::SqliteConnectOptions, Error, Pool, QueryBuilder, Sqlite};
use url::Url;

use crate::{
    model::{
        Chapter, ChapterId, ChapterInformation, ChapterState, Manga, MangaId, MangaInformation,
        MangaState, SourceId, SourceInformation,
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
                    WITH last_read AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            MAX(ci.chapter_number) AS last_read_chapter
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.read = 1
                        GROUP BY ci.source_id, ci.manga_id
                    ),
                    last_time_interacted AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            COALESCE(MAX(cs.last_read), 0) AS last_read_time
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.last_read IS NOT NULL
                        GROUP BY ci.source_id, ci.manga_id
                    )
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COUNT(ci.chapter_number) AS unread_chapters_count,
                        lti.last_read_time AS "last_read?: i64"
                    FROM manga_library ml
                    JOIN manga_informations mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    LEFT JOIN last_read lr
                        ON lr.source_id = ml.source_id AND lr.manga_id = ml.manga_id
                    LEFT JOIN last_time_interacted lti
                        ON lti.source_id = ml.source_id AND lti.manga_id = ml.manga_id
                    LEFT JOIN chapter_informations ci
                        ON ci.source_id = ml.source_id
                        AND ci.manga_id = ml.manga_id
                        AND (ms.preferred_scanlator IS NULL OR ci.scanlator = ms.preferred_scanlator OR ci.scanlator IS NULL)
                        AND ci.chapter_number > COALESCE(lr.last_read_chapter, -1)
                    GROUP BY ml.source_id, ml.manga_id, lti.last_read_time
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
                    WITH last_read AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            MAX(ci.chapter_number) AS last_read_chapter
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.read = 1
                        GROUP BY ci.source_id, ci.manga_id
                    ),
                    last_time_interacted AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            COALESCE(MAX(cs.last_read), 0) AS last_read_time
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.last_read IS NOT NULL
                        GROUP BY ci.source_id, ci.manga_id
                    )
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COUNT(ci.chapter_number) AS unread_chapters_count,
                        lti.last_read_time AS "last_read?: i64"
                    FROM manga_library ml
                    JOIN manga_informations mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    LEFT JOIN last_read lr
                        ON lr.source_id = ml.source_id AND lr.manga_id = ml.manga_id
                    LEFT JOIN last_time_interacted lti
                        ON lti.source_id = ml.source_id AND lti.manga_id = ml.manga_id
                    LEFT JOIN chapter_informations ci
                        ON ci.source_id = ml.source_id
                        AND ci.manga_id = ml.manga_id
                        AND (ms.preferred_scanlator IS NULL OR ci.scanlator = ms.preferred_scanlator OR ci.scanlator IS NULL)
                        AND ci.chapter_number > COALESCE(lr.last_read_chapter, -1)
                    GROUP BY ml.source_id, ml.manga_id, lti.last_read_time
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
                    WITH last_read AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            MAX(ci.chapter_number) AS last_read_chapter
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.read = 1
                        GROUP BY ci.source_id, ci.manga_id
                    ),
                    last_time_interacted AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            COALESCE(MAX(cs.last_read), 0) AS last_read_time
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.last_read IS NOT NULL
                        GROUP BY ci.source_id, ci.manga_id
                    )
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COUNT(ci.chapter_number) AS unread_chapters_count,
                        lti.last_read_time AS "last_read?: i64"
                    FROM manga_library ml
                    JOIN manga_informations mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    LEFT JOIN last_read lr
                        ON lr.source_id = ml.source_id AND lr.manga_id = ml.manga_id
                    LEFT JOIN last_time_interacted lti
                        ON lti.source_id = ml.source_id AND lti.manga_id = ml.manga_id
                    LEFT JOIN chapter_informations ci
                        ON ci.source_id = ml.source_id
                        AND ci.manga_id = ml.manga_id
                        AND (ms.preferred_scanlator IS NULL OR ci.scanlator = ms.preferred_scanlator OR ci.scanlator IS NULL)
                        AND ci.chapter_number > COALESCE(lr.last_read_chapter, -1)
                    GROUP BY ml.source_id, ml.manga_id, lti.last_read_time
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
                    WITH last_read AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            MAX(ci.chapter_number) AS last_read_chapter
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.read = 1
                        GROUP BY ci.source_id, ci.manga_id
                    ),
                    last_time_interacted AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            COALESCE(MAX(cs.last_read), 0) AS last_read_time
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.last_read IS NOT NULL
                        GROUP BY ci.source_id, ci.manga_id
                    )
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COUNT(ci.chapter_number) AS unread_chapters_count,
                        lti.last_read_time AS "last_read?: i64"
                    FROM manga_library ml
                    JOIN manga_informations mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    LEFT JOIN last_read lr
                        ON lr.source_id = ml.source_id AND lr.manga_id = ml.manga_id
                    LEFT JOIN last_time_interacted lti
                        ON lti.source_id = ml.source_id AND lti.manga_id = ml.manga_id
                    LEFT JOIN chapter_informations ci
                        ON ci.source_id = ml.source_id
                        AND ci.manga_id = ml.manga_id
                        AND (ms.preferred_scanlator IS NULL OR ci.scanlator = ms.preferred_scanlator OR ci.scanlator IS NULL)
                        AND ci.chapter_number > COALESCE(lr.last_read_chapter, -1)
                    GROUP BY ml.source_id, ml.manga_id, lti.last_read_time
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
                    WITH last_read AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            MAX(ci.chapter_number) AS last_read_chapter
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.read = 1
                        GROUP BY ci.source_id, ci.manga_id
                    ),
                    last_time_interacted AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            COALESCE(MAX(cs.last_read), 0) AS last_read_time
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.last_read IS NOT NULL
                        GROUP BY ci.source_id, ci.manga_id
                    )
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COUNT(ci.chapter_number) AS unread_chapters_count,
                        lti.last_read_time AS "last_read?: i64"
                    FROM manga_library ml
                    JOIN manga_informations mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    LEFT JOIN last_read lr
                        ON lr.source_id = ml.source_id AND lr.manga_id = ml.manga_id
                    LEFT JOIN last_time_interacted lti
                        ON lti.source_id = ml.source_id AND lti.manga_id = ml.manga_id
                    LEFT JOIN chapter_informations ci
                        ON ci.source_id = ml.source_id
                        AND ci.manga_id = ml.manga_id
                        AND (ms.preferred_scanlator IS NULL OR ci.scanlator = ms.preferred_scanlator OR ci.scanlator IS NULL)
                        AND ci.chapter_number > COALESCE(lr.last_read_chapter, -1)
                    GROUP BY ml.source_id, ml.manga_id, lti.last_read_time
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
                    WITH last_read AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            MAX(ci.chapter_number) AS last_read_chapter
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.read = 1
                        GROUP BY ci.source_id, ci.manga_id
                    ),
                    last_time_interacted AS (
                        SELECT
                            ci.source_id,
                            ci.manga_id,
                            COALESCE(MAX(cs.last_read), 0) AS last_read_time
                        FROM chapter_informations ci
                        JOIN chapter_state cs
                            ON ci.source_id = cs.source_id
                            AND ci.manga_id = cs.manga_id
                            AND ci.chapter_id = cs.chapter_id
                        LEFT JOIN manga_state ms
                            ON ms.source_id = ci.source_id AND ms.manga_id = ci.manga_id
                        WHERE (ms.preferred_scanlator IS NULL
                        OR ci.scanlator = ms.preferred_scanlator
                        OR ci.scanlator IS NULL)
                        AND cs.last_read IS NOT NULL
                        GROUP BY ci.source_id, ci.manga_id
                    )
                    SELECT
                        ml.source_id,
                        ml.manga_id,
                        mi.title,
                        mi.author,
                        mi.artist,
                        mi.cover_url,
                        COUNT(ci.chapter_number) AS unread_chapters_count,
                        lti.last_read_time AS "last_read?: i64"
                    FROM manga_library ml
                    JOIN manga_informations mi
                        ON mi.source_id = ml.source_id AND mi.manga_id = ml.manga_id
                    LEFT JOIN manga_state ms
                        ON ms.source_id = ml.source_id AND ms.manga_id = ml.manga_id
                    LEFT JOIN last_read lr
                        ON lr.source_id = ml.source_id AND lr.manga_id = ml.manga_id
                    LEFT JOIN last_time_interacted lti
                        ON lti.source_id = ml.source_id AND lti.manga_id = ml.manga_id
                    LEFT JOIN chapter_informations ci
                        ON ci.source_id = ml.source_id
                        AND ci.manga_id = ml.manga_id
                        AND (ms.preferred_scanlator IS NULL OR ci.scanlator = ms.preferred_scanlator OR ci.scanlator IS NULL)
                        AND ci.chapter_number > COALESCE(lr.last_read_chapter, -1)
                    GROUP BY ml.source_id, ml.manga_id, lti.last_read_time
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
                    last_read: row.last_read,
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
                WITH filtered AS (
                    SELECT ci.chapter_number, cs.read
                    FROM chapter_informations ci
                    LEFT JOIN chapter_state cs
                        ON ci.source_id = cs.source_id
                        AND ci.manga_id = cs.manga_id
                        AND ci.chapter_id = cs.chapter_id
                    WHERE ci.source_id = ?1
                    AND ci.manga_id = ?2
                    AND (?3 IS NULL OR ci.scanlator = ?3 OR ci.scanlator IS NULL)
                ),
                max_read AS (
                    SELECT COALESCE(MAX(chapter_number), -1) AS last_read
                    FROM filtered
                    WHERE read = 1
                )
                SELECT
                    COUNT(*) AS count,
                    CASE WHEN EXISTS (SELECT 1 FROM filtered) THEN 1 ELSE 0 END AS "has_chapters: bool"
                FROM filtered, max_read
                WHERE filtered.chapter_number > max_read.last_read
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

    pub async fn fetch_unread_chapter_counts_minimal(
        &self,
        manga_ids: &[MangaId],
    ) -> HashMap<MangaId, (Option<usize>, Option<i64>)> {
        let mut map = HashMap::new();

        if manga_ids.is_empty() {
            return map;
        }

        // Build dynamic SQL placeholders
        let pairs: Vec<String> = manga_ids.iter().map(|_| "(?, ?)".into()).collect();
        let in_clause = pairs.join(", ");

        let query = format!(
            r#"
            WITH filtered AS (
                SELECT
                    ci.source_id,
                    ci.manga_id,
                    ci.chapter_number,
                    cs.read,
                    cs.last_time
                FROM chapter_informations ci
                LEFT JOIN chapter_state cs
                    ON ci.source_id = cs.source_id
                    AND ci.manga_id = cs.manga_id
                    AND ci.chapter_id = cs.chapter_id
                WHERE (ci.source_id, ci.manga_id) IN ({})
            ),
            max_read AS (
                SELECT
                    source_id,
                    manga_id,
                    COALESCE(MAX(CASE WHEN read = 1 THEN chapter_number END), -1) AS last_read,
                    COALESCE(MAX(last_time), 0) AS last_read_time
                FROM filtered
                GROUP BY source_id, manga_id
            )
            SELECT
                f.source_id,
                f.manga_id,
                COUNT(*) AS count,
                mr.last_read_time as "last_time?: i64"
            FROM filtered f
            JOIN max_read mr
                ON f.source_id = mr.source_id AND f.manga_id = mr.manga_id
            WHERE f.chapter_number > mr.last_read
            GROUP BY f.source_id, f.manga_id, mr.last_read_time
            "#,
            in_clause
        );

        // Bind params
        let mut query_builder = sqlx::query_as::<_, UnreadChaptersRowFull>(&query);
        for id in manga_ids {
            query_builder = query_builder.bind(id.source_id().value()).bind(id.value());
        }

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        for row in rows {
            let id = MangaId::new(SourceId::new(row.source_id), row.manga_id);
            map.insert(id, (row.count.map(|v| v as usize), row.last_time));
        }

        for id in manga_ids {
            map.entry(id.clone()).or_insert((None, None));
        }

        map
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

    pub async fn find_cached_chapter_ids(
        &self,
        manga_id: &MangaId,
    ) -> anyhow::Result<HashSet<ChapterId>> {
        let source_id = manga_id.source_id().value();
        let manga_id_value = manga_id.value();

        let rows = sqlx::query!(
            r#"
                SELECT chapter_id
                FROM chapter_informations
                WHERE source_id = ?1 AND manga_id = ?2
            "#,
            source_id,
            manga_id_value
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ChapterId::new(manga_id.clone(), row.chapter_id))
            .collect())
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

    pub async fn find_cached_chapters(
        &self,
        manga_id: &MangaId,
        chapter_storage: &crate::chapter_storage::ChapterStorage,
    ) -> Vec<Chapter> {
        let source_id = manga_id.source_id().value();
        let manga_id_val = manga_id.value();

        let rows = sqlx::query!(
            r#"
            SELECT 
                ci.source_id,
                ci.manga_id,
                ci.chapter_id,
                ci.title,
                ci.scanlator,
                ci.chapter_number,
                ci.volume_number,
                cs.read AS "read?: bool",
                cs.last_read AS "last_read?: i64"
            FROM chapter_informations ci
            LEFT JOIN chapter_state cs
                ON ci.source_id = cs.source_id
                AND ci.manga_id = cs.manga_id
                AND ci.chapter_id = cs.chapter_id
            WHERE ci.source_id = ?1 AND ci.manga_id = ?2
            GROUP BY ci.source_id, ci.manga_id, ci.chapter_id
            ORDER BY ci.manga_order ASC;
            "#,
            source_id,
            manga_id_val,
        )
        .fetch_all(&self.pool)
        .await
        .unwrap();

        let out = rows
            .into_iter()
            .map(|row| {
                let id = ChapterId::new(
                    MangaId::new(SourceId::new(row.source_id), row.manga_id),
                    row.chapter_id,
                );

                let information = ChapterInformation {
                    id: id.clone(),
                    title: row.title,
                    scanlator: row.scanlator,

                    chapter_number: row.chapter_number.and_then(|f| Decimal::from_f64(f)),
                    volume_number: row.volume_number.and_then(|f| Decimal::from_f64(f)),
                    // manga_order: row.manga_order as usize,
                };

                let state = ChapterState {
                    read: row.read.unwrap_or(false),
                    last_read: row.last_read,
                };

                let downloaded = chapter_storage.get_stored_chapter(&id).is_some();

                Chapter {
                    information,
                    state,
                    downloaded,
                }
            })
            .collect();
        out
    }

    pub async fn upsert_cached_manga_information(
        &self,
        manga_informations: &[MangaInformation],
    ) -> Result<(), Error> {
        if manga_informations.is_empty() {
            return Ok(());
        }

        const MAX_BATCH_SIZE: usize = 20; // Kindle safe size
        let mut start = 0;

        while start < manga_informations.len() {
            let end = (start + MAX_BATCH_SIZE).min(manga_informations.len());
            let chunk = &manga_informations[start..end];
            start = end;

            // Build VALUES (?, ?, ?, ?, ?, ?), ...
            let mut values_sql = String::new();
            for (i, _) in chunk.iter().enumerate() {
                if i > 0 {
                    values_sql.push_str(", ");
                }
                values_sql.push_str("(?, ?, ?, ?, ?, ?)");
            }

            let sql = format!(
                r#"
                INSERT INTO manga_informations (
                    source_id, manga_id, title, author, artist, cover_url
                )
                VALUES {values_sql}
                ON CONFLICT(source_id, manga_id) DO UPDATE SET
                    title = excluded.title,
                    author = excluded.author,
                    artist = excluded.artist,
                    cover_url = excluded.cover_url
                "#
            );

            let mut query = sqlx::query(&sql);
            for info in chunk {
                query = query.bind(info.id.source_id().value());
                query = query.bind(info.id.value());
                query = query.bind(&info.title);
                query = query.bind(&info.author);
                query = query.bind(&info.artist);
                query = query.bind(info.cover_url.as_ref().map(|url| url.to_string()));
            }

            // No transaction, flush immediately
            if let Err(e) = query.execute(&self.pool).await {
                eprintln!("WARN: upsert_cached_manga_information failed: {e}");
            }
        }

        Ok(())
    }

    pub async fn upsert_cached_chapter_informations(
        &self,
        manga_id: &MangaId,
        chapter_informations: &[ChapterInformation],
    ) -> anyhow::Result<()> {
        use rust_decimal::prelude::ToPrimitive;

        let cached_chapter_ids: HashSet<_> = self.find_cached_chapter_ids(manga_id).await?;

        let chapter_ids: HashSet<_> = chapter_informations
            .iter()
            .map(|info| info.id.clone())
            .collect();
        let removed_chapter_ids: Vec<_> = cached_chapter_ids
            .difference(&chapter_ids)
            .cloned()
            .collect();

        let remove_chunk_size = BIND_LIMIT.saturating_sub(2);
        for chunk in removed_chapter_ids.chunks(remove_chunk_size) {
            let mut builder = QueryBuilder::new("DELETE FROM chapter_informations WHERE ");
            builder
                .push("source_id = ")
                .push_bind(manga_id.source_id().value())
                .push(" AND manga_id = ")
                .push_bind(manga_id.value())
                .push(" AND chapter_id IN ")
                .push_tuples(chunk, |mut b, chapter_id| {
                    b.push_bind(chapter_id.value());
                });

            builder.build().execute(&self.pool).await?;
        }

        const INSERT_FIELD_COUNT: usize = 8;
        const CHUNK_SIZE: usize = BIND_LIMIT / INSERT_FIELD_COUNT;

        for (offset, chunk) in chapter_informations.chunks(CHUNK_SIZE).enumerate() {
            let mut builder = QueryBuilder::new(
            "INSERT INTO chapter_informations (source_id, manga_id, chapter_id, manga_order, title, scanlator, chapter_number, volume_number)"
            );

            builder.push_values(chunk.iter().enumerate(), |mut b, (i, info)| {
                let chapter_number = info.chapter_number.map(|d| d.to_f64());
                let volume_number = info.volume_number.map(|d| d.to_f64());

                b.push_bind(info.id.source_id().value())
                    .push_bind(info.id.manga_id().value())
                    .push_bind(info.id.value())
                    .push_bind((offset * CHUNK_SIZE + i) as i64)
                    .push_bind(&info.title)
                    .push_bind(&info.scanlator)
                    .push_bind(chapter_number)
                    .push_bind(volume_number);
            });

            builder.push(
                " ON CONFLICT DO UPDATE SET
                manga_order = excluded.manga_order,
                title = excluded.title,
                scanlator = excluded.scanlator,
                chapter_number = excluded.chapter_number,
                volume_number = excluded.volume_number",
            );

            builder.build().execute(&self.pool).await?;
        }

        Ok(())
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
                SELECT source_id, manga_id, chapter_id, read AS "read: bool", last_read AS "last_read?: i64" FROM chapter_state
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
                INSERT INTO chapter_state (source_id, manga_id, chapter_id, read, last_read)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT DO UPDATE SET
                    read = excluded.read,
                    last_read = excluded.last_read
            "#,
            source_id,
            manga_id,
            chapter_id,
            state.read,
            state.last_read,
        )
        .execute(&self.pool)
        .await
        .unwrap();
    }

    pub async fn mark_chapter_as_read(&self, id: &ChapterId) {
        let now = chrono::Utc::now().timestamp();

        let source_id = id.source_id().value();
        let manga_id = id.manga_id().value();
        let chapter_id = id.value();

        sqlx::query!(
            r#"
            INSERT INTO chapter_state (source_id, manga_id, chapter_id, read, last_read)
            VALUES (?1, ?2, ?3, TRUE, ?4)
            ON CONFLICT DO UPDATE SET
                read = TRUE,
                last_read = excluded.last_read
        "#,
            source_id,
            manga_id,
            chapter_id,
            now,
        )
        .execute(&self.pool)
        .await
        .unwrap();
    }

    pub async fn update_last_read_chapter(&self, id: &ChapterId) {
        let now = chrono::Utc::now().timestamp();

        let source_id = id.source_id().value();
        let manga_id = id.manga_id().value();
        let chapter_id = id.value();

        sqlx::query!(
            r#"
            INSERT INTO chapter_state (source_id, manga_id, chapter_id, read, last_read)
            VALUES (?1, ?2, ?3, FALSE, ?4)
            ON CONFLICT DO UPDATE SET
                last_read = excluded.last_read
        "#,
            source_id,
            manga_id,
            chapter_id,
            now,
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

    /// Timestamp of the last read chapter (nullable in DB)
    pub last_read: Option<i64>,
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
    last_read: Option<i64>,
}

impl From<ChapterStateRow> for ChapterState {
    fn from(value: ChapterStateRow) -> Self {
        Self {
            read: value.read,
            last_read: value.last_read,
        }
    }
}

#[derive(sqlx::FromRow)]
struct UnreadChaptersRow {
    count: Option<i32>,
    has_chapters: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct UnreadChaptersRowFull {
    source_id: String,
    manga_id: String,
    count: Option<i32>,
    last_time: Option<i64>,
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
