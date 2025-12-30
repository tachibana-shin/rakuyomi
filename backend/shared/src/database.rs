use chrono::TimeZone;
use futures::TryStreamExt;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;
use sqlx::{sqlite::SqliteConnectOptions, Error, Pool, QueryBuilder, Sqlite};
use url::Url;

use crate::{
    model::{
        Chapter, ChapterId, ChapterInformation, ChapterState, Manga, MangaId, MangaInformation,
        MangaState, NotificationInformation, SourceId, SourceInformation,
    },
    source::model::PublishingStatus,
    source_collection::SourceCollection,
};

#[derive(Clone)]
pub struct Database {
    pub filename: PathBuf,
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

        Ok(Self {
            pool,
            filename: filename.to_path_buf(),
        })
    }

    pub async fn hot_replace(&mut self, buf: &Vec<u8>) -> Result<()> {
        self.pool.close().await;

        tokio::fs::write(&self.filename, buf).await?;

        let options = SqliteConnectOptions::new()
            .filename(&self.filename)
            .create_if_missing(true);
        let pool = Pool::<Sqlite>::connect_with(options).await?;

        sqlx::migrate!().run(&pool).await?;

        self.pool = pool;

        Ok(())
    }

    pub async fn get_manga_library(&self) -> Result<Vec<MangaId>> {
        let rows = sqlx::query_as!(
            MangaLibraryRow,
            r#"
                SELECT * FROM manga_library;
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|row| row.manga_id()).collect())
    }

    pub async fn get_manga_library_and_status(&self) -> Result<Vec<(MangaId, PublishingStatus)>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                ml.manga_id,
                ml.source_id,
                md.status
            FROM manga_library ml
            LEFT JOIN manga_details md
                ON ml.manga_id = md.id
            AND ml.source_id = md.source_id
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    MangaId::from_strings(row.source_id, row.manga_id),
                    row.status
                        .map(|s| {
                            <PublishingStatus as num_enum::FromPrimitive>::from_primitive(s as u8)
                        })
                        .unwrap_or(PublishingStatus::Unknown),
                )
            })
            .collect())
    }

    pub async fn get_manga_library_with_read_count(
        &self,
        source_collection: &impl SourceCollection,
        library_sorting_mode: &crate::settings::LibrarySortingMode,
    ) -> Result<Vec<Manga>> {
        let rows = match *library_sorting_mode {
            crate::settings::LibrarySortingMode::Ascending => {
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
            crate::settings::LibrarySortingMode::Descending => {
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
            crate::settings::LibrarySortingMode::TitleAsc => {
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
            crate::settings::LibrarySortingMode::TitleDesc => {
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
            crate::settings::LibrarySortingMode::UnreadAsc => {
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
            crate::settings::LibrarySortingMode::UnreadDesc => {
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
                    in_library: true,
                })
            })
            .collect();

        Ok(mangas)
    }

    pub async fn add_manga_to_library(&self, manga_id: MangaId) -> Result<()> {
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
        .await?;

        Ok(())
    }

    pub async fn remove_manga_from_library(&self, manga_id: MangaId) -> Result<()> {
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
        .await?;

        Ok(())
    }

    pub async fn count_unread_chapters(&self, manga_id: &MangaId) -> Result<Option<usize>> {
        // Get preferred scanlator if it exists
        let preferred_scanlator = self
            .find_manga_state(manga_id)
            .await?
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
        .await?;

        if !row.has_chapters.unwrap_or(false) {
            return Ok(None);
        }

        Ok(row.count.map(|count| count.try_into().unwrap()))
    }

    pub async fn fetch_unread_chapter_counts_minimal(
        &self,
        manga_ids: &[MangaId],
    ) -> Result<HashMap<MangaId, (Option<usize>, Option<i64>, bool)>> {
        let mut map = HashMap::new();

        if manga_ids.is_empty() {
            return Ok(map);
        }

        // Build dynamic SQL placeholders
        let pairs: Vec<String> = manga_ids.iter().map(|_| "(?, ?)".into()).collect();
        let in_clause = pairs.join(", ");

        let query = format!(
            r#"
            WITH inputs(source_id, manga_id) AS (
                VALUES
                {}
            ),
            filtered AS (
                SELECT
                    i.source_id,
                    i.manga_id,
                    ci.chapter_number,
                    cs.read,
                    cs.last_read as last_time
                FROM inputs i
                LEFT JOIN chapter_informations ci
                    ON ci.source_id = i.source_id AND ci.manga_id = i.manga_id
                LEFT JOIN chapter_state cs
                    ON ci.source_id = cs.source_id
                    AND ci.manga_id = cs.manga_id
                    AND ci.chapter_id = cs.chapter_id
            ),
            max_read AS (
                SELECT
                    source_id,
                    manga_id,
                    COALESCE(MAX(CASE WHEN read = 1 THEN chapter_number END), -1) AS last_read,
                    COALESCE(MAX(last_time), NULL) AS last_read_time
                FROM filtered
                GROUP BY source_id, manga_id
            )
            SELECT
                i.source_id,
                i.manga_id,
                COALESCE(COUNT(f.chapter_number), -1) AS count,
                mr.last_read_time AS last_time,
                CASE WHEN ml.source_id IS NOT NULL THEN TRUE ELSE FALSE END AS in_library
            FROM inputs i
            LEFT JOIN filtered f
                ON i.source_id = f.source_id AND i.manga_id = f.manga_id
                AND f.chapter_number > COALESCE((SELECT last_read FROM max_read mr2 WHERE mr2.source_id = i.source_id AND mr2.manga_id = i.manga_id), -1)
            LEFT JOIN max_read mr
                ON i.source_id = mr.source_id AND i.manga_id = mr.manga_id
            LEFT JOIN manga_library ml
                ON i.source_id = ml.source_id AND i.manga_id = ml.manga_id
            GROUP BY i.source_id, i.manga_id, mr.last_read_time, in_library
            "#,
            in_clause
        );

        // Bind params
        let mut query_builder = sqlx::query_as::<_, UnreadChaptersRowFull>(&query);
        for id in manga_ids {
            query_builder = query_builder.bind(id.source_id().value()).bind(id.value());
        }

        let rows = query_builder.fetch_all(&self.pool).await.map_err(|e| {
            eprintln!("🔥 SQL query failed: {}", e);
            e
        })?;

        for row in rows {
            let id = MangaId::new(SourceId::new(row.source_id), row.manga_id);
            map.insert(
                id,
                (
                    row.count.map(|v| v as usize),
                    row.last_time.map(|v| v as i64),
                    row.in_library,
                ),
            );
        }

        for id in manga_ids {
            map.entry(id.clone()).or_insert((None, None, false));
        }

        Ok(map)
    }

    pub async fn find_cached_manga_information(
        &self,
        manga_id: &MangaId,
    ) -> Result<Option<MangaInformation>> {
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
        .await?;

        Ok(maybe_row.map(|row| row.into()))
    }

    pub async fn find_cached_chapter_information(
        &self,
        chapter_id: &ChapterId,
    ) -> Result<Option<ChapterInformation>> {
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
        .await?;

        Ok(maybe_row.map(|row| row.into()))
    }

    pub async fn find_orphan_or_read_files(
        &self,
        chapter_storage: &crate::chapter_storage::ChapterStorage,
        invalid_mode: bool,
    ) -> Result<Vec<PathBuf>> {
        if invalid_mode {
            let mut remaining = chapter_storage.collect_all_files(1);

            let mut stream = sqlx::query_as!(
                ChapterInformationsRow,
                r#"SELECT * FROM chapter_informations"#
            )
            .fetch(&self.pool);

            while let Some(row) = stream.try_next().await? {
                let id = ChapterId::from_strings(row.source_id, row.manga_id, row.chapter_id);

                for is_novel in [false, true] {
                    let path = chapter_storage.get_path_to_store_chapter(&id, is_novel);
                    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                        remaining.remove(&path);
                    }

                    let Some(path_file_errors) = chapter_storage.errors_source_path(&path).ok()
                    else {
                        continue;
                    };
                    if tokio::fs::try_exists(&path_file_errors)
                        .await
                        .unwrap_or(false)
                    {
                        remaining.remove(&path_file_errors);
                    }
                }
            }

            Ok(remaining.into_iter().collect())
        } else {
            let mut paths = Vec::new();

            let mut stream = sqlx::query!(
                r#"
                SELECT source_id, manga_id, chapter_id
                FROM chapter_state
                WHERE read = 1
                "#
            )
            .fetch(&self.pool);

            while let Some(row) = stream.try_next().await? {
                let id = ChapterId::from_strings(row.source_id, row.manga_id, row.chapter_id);

                for is_novel in [false, true] {
                    let path = chapter_storage.get_path_to_store_chapter(&id, is_novel);
                    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                        paths.push(path.clone());
                    }

                    let Some(path_file_errors) = chapter_storage.errors_source_path(&path).ok()
                    else {
                        continue;
                    };
                    if tokio::fs::try_exists(&path_file_errors)
                        .await
                        .unwrap_or(false)
                    {
                        paths.push(path_file_errors);
                    }
                }
            }

            Ok(paths)
        }
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
    ) -> Result<Vec<ChapterInformation>> {
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
        .await?;

        Ok(rows.into_iter().map(|row| row.into()).collect())
    }

    pub async fn find_cached_chapters(
        &self,
        manga_id: &MangaId,
        chapter_storage: &crate::chapter_storage::ChapterStorage,
    ) -> Result<Vec<Chapter>> {
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
                ci.last_updated,
                ci.thumbnail,
                ci.lang,
                ci.locked AS "locked: bool",
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
        .await?;

        Ok(rows
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

                    chapter_number: row.chapter_number.map(|v| v as f32),
                    volume_number: row.volume_number.map(|v| v as f32),
                    // manga_order: row.manga_order as usize,
                    last_updated: row.last_updated,
                    thumbnail: row.thumbnail.and_then(|s| Url::parse(&s).ok()),
                    lang: row.lang,

                    url: None,
                    locked: Some(row.locked),
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
            .collect())
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
            "INSERT INTO chapter_informations (source_id, manga_id, chapter_id, manga_order, title, scanlator, chapter_number, volume_number, last_updated, thumbnail, lang, locked)"
            );

            builder.push_values(chunk.iter().enumerate(), |mut b, (i, info)| {
                let chapter_number = info.chapter_number;
                let volume_number = info.volume_number;
                let last_updated = info.last_updated;

                b.push_bind(info.id.source_id().value())
                    .push_bind(info.id.manga_id().value())
                    .push_bind(info.id.value())
                    .push_bind((offset * CHUNK_SIZE + i) as i64)
                    .push_bind(&info.title)
                    .push_bind(&info.scanlator)
                    .push_bind(chapter_number)
                    .push_bind(volume_number)
                    .push_bind(last_updated)
                    .push_bind(info.thumbnail.as_ref().map(|s| s.to_string()))
                    .push_bind(info.lang.as_ref().map(|s| s.to_string()))
                    .push_bind(if info.locked.unwrap_or_default() {
                        1
                    } else {
                        0
                    });
            });

            builder.push(
                " ON CONFLICT DO UPDATE SET
                manga_order = excluded.manga_order,
                title = excluded.title,
                scanlator = excluded.scanlator,
                chapter_number = excluded.chapter_number,
                volume_number = excluded.volume_number,
                last_updated = excluded.last_updated",
            );

            builder.build().execute(&self.pool).await?;
        }

        Ok(())
    }

    pub async fn find_cached_manga_details(
        &self,
        manga_id: &MangaId,
    ) -> Result<Option<(crate::source::model::Manga, f64)>> {
        let source_id = manga_id.source_id().value();
        let manga_id = manga_id.value();

        let row = sqlx::query_as!(
            MangaDetailsRow,
            r#"
            SELECT 
                md.*, 
                COALESCE(AVG(cs.read), 0) AS "per_read: f64",
                COALESCE(
                    MAX(cs.last_read),
                    0
                ) AS "last_read: i64"
            FROM manga_details md
            LEFT JOIN chapter_state cs
                ON cs.source_id = md.source_id
                AND cs.manga_id = md.id
            WHERE md.source_id = ?1 AND md.id = ?2
            GROUP BY md.source_id, md.id
            "#,
            source_id,
            manga_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|v| {
            let per_read = v.per_read.unwrap_or(0.0);
            let manga = v.into();
            (manga, per_read)
        }))
    }

    pub async fn upsert_cached_manga_details(
        &self,
        manga_id: &MangaId,
        manga: &crate::source::model::Manga,
    ) -> Result<()> {
        // Extract keys
        let source_id = manga_id.source_id().value();
        let id = manga_id.value();

        // Serialize tags to JSON (Option<Vec<String>> → Option<String>)
        let tags = manga
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap());

        // Convert Url → Option<String>
        let cover_url = manga.cover_url.as_ref().map(|u| u.as_str().to_string());
        let url = manga.url.as_ref().map(|u| u.as_str().to_string());

        // Convert DateTime → Option<String> (ISO8601)
        let last_updated = manga.last_updated.map(|dt| dt.to_rfc3339());
        let last_opened = manga.last_opened.map(|dt| dt.to_rfc3339());
        let date_added = manga.date_added.map(|dt| dt.to_rfc3339());

        let status = manga.status.clone() as u8;
        let nsfw = manga.nsfw.clone() as u8;
        let viewer = manga.viewer.clone() as u8;

        sqlx::query!(
            r#"
            INSERT INTO manga_details (
                source_id,
                id,
                title,
                author,
                artist,
                description,
                tags,
                cover_url,
                url,
                status,
                nsfw,
                viewer,
                last_updated,
                last_opened,
                date_added
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(source_id, id) DO UPDATE SET
                title        = excluded.title,
                author       = excluded.author,
                artist       = excluded.artist,
                description  = excluded.description,
                tags         = excluded.tags,
                cover_url    = excluded.cover_url,
                url          = excluded.url,
                status       = excluded.status,
                nsfw         = excluded.nsfw,
                viewer       = excluded.viewer,
                last_updated = excluded.last_updated,
                last_opened  = excluded.last_opened,
                date_added   = excluded.date_added
            "#,
            source_id,
            id,
            manga.title,
            manga.author,
            manga.artist,
            manga.description,
            tags,
            cover_url,
            url,
            status,
            nsfw,
            viewer,
            last_updated,
            last_opened,
            date_added,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn find_manga_state(&self, manga_id: &MangaId) -> Result<Option<MangaState>> {
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
        .await?;

        Ok(maybe_row.map(|row| row.into()))
    }

    pub async fn upsert_manga_state(&self, manga_id: &MangaId, state: MangaState) -> Result<()> {
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
        .await?;

        Ok(())
    }

    pub async fn find_chapter_state(&self, chapter_id: &ChapterId) -> Result<Option<ChapterState>> {
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
        .await?;

        Ok(maybe_row.map(|row| row.into()))
    }

    pub async fn upsert_chapter_state(
        &self,
        chapter_id: &ChapterId,
        state: ChapterState,
    ) -> Result<()> {
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
        .await?;

        Ok(())
    }

    pub async fn mark_chapter_as_read(&self, id: &ChapterId, value: Option<bool>) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let value = value.unwrap_or(true);

        let source_id = id.source_id().value();
        let manga_id = id.manga_id().value();
        let chapter_id = id.value();

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
            value,
            now,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_last_read_chapter(&self, id: &ChapterId) -> Result<()> {
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
        .await?;

        Ok(())
    }

    pub async fn get_last_check_update_manga(&self, id: &MangaId) -> Result<Option<(i64, i64)>> {
        let source_id = id.source_id().value();
        let manga_id = id.value();

        let maybe_row = sqlx::query!(
            r#"SELECT last_check, next_ts_arima FROM last_check_update WHERE source_id = ?1 AND manga_id = ?2"#,
            source_id,
            manga_id,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(maybe_row.map(|row| (row.last_check, row.next_ts_arima)))
    }

    pub async fn set_last_check_update_manga(
        &self,
        id: &MangaId,
        value: i64,
        next_ts_arima: i64,
    ) -> Result<()> {
        let source_id = id.source_id().value();
        let manga_id = id.value();

        sqlx::query!(
            r#"
            INSERT INTO last_check_update (
                source_id,
                manga_id,
                last_check,
                next_ts_arima
            ) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(source_id, manga_id) DO UPDATE SET
                last_check = excluded.last_check,
                next_ts_arima = excluded.next_ts_arima
            "#,
            source_id,
            manga_id,
            value,
            next_ts_arima
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_last_check_update_manga(&self, id: &MangaId) -> Result<()> {
        let source_id = id.source_id().value();
        let manga_id = id.value();

        sqlx::query!(
            r#"
            DELETE FROM last_check_update WHERE source_id = ?1 AND manga_id = ?2
            "#,
            source_id,
            manga_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_notification(
        &self,
        manga_id: &MangaId,
        new_chapters: &[ChapterInformation],
    ) -> Result<(), sqlx::Error> {
        if new_chapters.is_empty() {
            return Ok(());
        }

        let now = chrono::Utc::now().timestamp();

        // Build query string: "(?, ?, ?, ?, ?), (?, ?, ?, ?, ?), ..."
        let mut sql = String::from(
            "INSERT INTO notifications (source_id, manga_id, chapter_id, created_at) VALUES ",
        );

        for (i, _) in new_chapters.iter().enumerate() {
            if i > 0 {
                sql.push_str(", ");
            }
            sql.push_str("(?, ?, ?, ?)");
        }

        // Create query
        let mut query = sqlx::query(&sql);

        // Bind all values in order
        for ch in new_chapters {
            query = query
                .bind(manga_id.source_id().value())
                .bind(manga_id.value()) // manga_id
                .bind(ch.id.value()) // chapter_id
                .bind(now); // created_at
        }

        // Execute
        query.execute(&self.pool).await?;

        Ok(())
    }

    pub async fn get_next_ts_arima_min(
        &self,
        skip_sources: &Vec<&str>,
    ) -> Result<Option<(MangaId, i64)>> {
        let condition = if skip_sources.is_empty() {
            "".into()
        } else {
            format!(
                "AND source_id NOT IN ({})",
                std::iter::repeat_n("?", skip_sources.len())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };

        let sql = format!(
            r#"
            SELECT manga_id, source_id, next_ts_arima
            FROM last_check_update
            WHERE next_ts_arima IS NOT NULL
            {condition}
            ORDER BY next_ts_arima ASC
            LIMIT 1
            "#
        );

        let mut query = sqlx::query(&sql);

        for s in skip_sources {
            query = query.bind(s);
        }

        let row = query.fetch_optional(&self.pool).await?;

        use sqlx::Row;
        Ok(row.map(|row| {
            (
                MangaId::from_strings(
                    row.get::<String, _>("source_id"),
                    row.get::<String, _>("manga_id"),
                ),
                row.get::<i64, _>("next_ts_arima"),
            )
        }))
    }

    pub async fn get_due_mangas(&self) -> Result<Vec<(MangaId, PublishingStatus)>> {
        let now = chrono::Utc::now().timestamp();
        let due_mangas = sqlx::query!(
            r#"
            SELECT
                ml.manga_id, ml.source_id, md.status
            FROM
                last_check_update ml
            LEFT JOIN manga_details md
            ON ml.manga_id = md.id AND
                ml.source_id = md.source_id
            WHERE ml.next_ts_arima <= ?1
            "#,
            now
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(due_mangas
            .into_iter()
            .map(|row| {
                (
                    MangaId::from_strings(row.source_id, row.manga_id),
                    row.status
                        .map(|s| {
                            <PublishingStatus as num_enum::FromPrimitive>::from_primitive(s as u8)
                        })
                        .unwrap_or(PublishingStatus::Unknown),
                )
            })
            .collect())
    }

    pub async fn get_count_notifications(&self) -> Result<i32> {
        let value = sqlx::query!(
            r#"
            SELECT
                COUNT(*) as "count: i32"
            FROM
                notifications
            WHERE is_read = 0
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(value.count)
    }

    pub async fn get_notifications(&self) -> Result<Vec<NotificationInformation>> {
        let rows = sqlx::query_as!(
            NotificationInformationRow,
            r#"
            SELECT
                n.id,
                n.source_id,
                n.manga_id,
                n.chapter_id,
                mi.title AS manga_title,
                md.cover_url AS manga_cover,
                md.status AS manga_status,
                ci.title AS chapter_title,
                ci.chapter_number,
                n.created_at
            FROM
                notifications n
            LEFT JOIN manga_informations mi
                ON mi.manga_id = n.manga_id AND mi.source_id = n.source_id
            LEFT JOIN manga_details md
                ON md.id = n.manga_id AND md.source_id = n.source_id
            LEFT JOIN chapter_informations ci
                ON ci.manga_id = n.manga_id AND ci.source_id = n.source_id AND ci.chapter_id = n.chapter_id
            WHERE
                n.is_read = 0
            ORDER BY
                n.created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|row| row.into()).collect())
    }

    pub async fn delete_notification(&self, id: i32) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM notifications WHERE id = ?1
            "#,
            id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn clear_notifications(&self) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM notifications WHERE 1 = 1
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_chapters_read_state(
        &self,
        manga_id: &MangaId,
        ids: &[ChapterId],
        read: bool,
    ) -> anyhow::Result<Option<usize>> {
        if ids.is_empty() {
            return self.count_unread_chapters(manga_id).await;
        }

        // --- Build VALUES (?, ?, ?, ?, ?), (?, ?, ?, ?, ?), ... ---
        // Each row: (source_id, manga_id, chapter_id, read, last_read)
        let mut sql = String::from(
            r#"
            INSERT INTO chapter_state (source_id, manga_id, chapter_id, read)
            VALUES
            "#,
        );

        for i in 0..ids.len() {
            if i > 0 {
                sql.push_str(", ");
            }
            sql.push_str("(?, ?, ?, ?)");
        }

        // Add ON CONFLICT logic
        sql.push_str(
            r#"
            ON CONFLICT (source_id, manga_id, chapter_id)
            DO UPDATE SET
                read = excluded.read,
                last_read = CASE
                    WHEN excluded.read = TRUE THEN excluded.last_read
                    ELSE chapter_state.last_read
                END
            "#,
        );

        let mut query = sqlx::query(&sql);

        for id in ids {
            query = query
                .bind(id.source_id().value())
                .bind(id.manga_id().value())
                .bind(id.value())
                .bind(read);
        }

        query.execute(&self.pool).await?;

        self.count_unread_chapters(manga_id).await
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
    pub unread_chapters_count: Option<i64>,

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
struct MangaDetailsRow {
    source_id: String,
    id: String,
    title: Option<String>,
    author: Option<String>,
    artist: Option<String>,
    description: Option<String>,
    tags: Option<String>,
    cover_url: Option<String>,
    url: Option<String>,
    status: i64,
    nsfw: i64,
    viewer: i64,
    // FIXME i dont think those are needed, the sources have no way of creating them
    last_updated: Option<String>,
    last_opened: Option<String>,
    last_read: Option<i64>,
    date_added: Option<String>,

    pub per_read: Option<f64>,
}
impl From<MangaDetailsRow> for crate::source::model::Manga {
    fn from(row: MangaDetailsRow) -> Self {
        // Parse enums (SQLite returns INTEGER as i64)
        let status = crate::source::model::PublishingStatus::from(row.status as u8);
        let nsfw = crate::source::model::MangaContentRating::from(row.nsfw as u8);
        let viewer = crate::source::model::MangaViewer::from(row.viewer as u8);

        // Parse tags JSON
        let tags = row.tags.map(|json| serde_json::from_str(&json).unwrap());

        // Parse URLs
        let cover_url = row
            .cover_url
            .as_ref()
            .map(|s| s.as_str().try_into().unwrap());

        let url = row.url.as_ref().map(|s| s.as_str().try_into().unwrap());

        // Parse datetime
        let parse_dt = |v: Option<String>| {
            v.map(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&chrono_tz::UTC)
            })
        };

        Self {
            source_id: row.source_id,
            id: row.id,
            title: row.title,
            author: row.author,
            artist: row.artist,
            description: row.description,

            tags,
            cover_url,
            url,

            status,
            nsfw,
            viewer,

            last_updated: parse_dt(row.last_updated),
            last_opened: parse_dt(row.last_opened),
            last_read: if let Some(last_read) = row.last_read {
                if last_read >= 0 {
                    chrono::Utc
                        .timestamp_opt(last_read, 0)
                        .single()
                        .map(|d| d.with_timezone(&chrono_tz::UTC))
                } else {
                    None
                }
            } else {
                None
            },
            date_added: parse_dt(row.date_added),
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
    last_updated: Option<i64>,
    thumbnail: Option<String>,
    lang: Option<String>,
    locked: i64,
}

impl From<ChapterInformationsRow> for ChapterInformation {
    fn from(value: ChapterInformationsRow) -> Self {
        Self {
            id: ChapterId::from_strings(value.source_id, value.manga_id, value.chapter_id),
            title: value.title,
            scanlator: value.scanlator,
            chapter_number: value.chapter_number.map(|v| v as f32),
            volume_number: value.volume_number.map(|v| v as f32),
            last_updated: value.last_updated,
            thumbnail: value.thumbnail.and_then(|s| Url::parse(&s).ok()),
            lang: value.lang,

            url: None,
            locked: Some(value.locked != 0),
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
    count: Option<i64>,
    has_chapters: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct UnreadChaptersRowFull {
    source_id: String,
    manga_id: String,
    count: Option<i32>,
    last_time: Option<i32>,
    in_library: bool,
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

#[derive(sqlx::FromRow)]
struct NotificationInformationRow {
    id: i64,
    source_id: String,
    manga_id: String,
    chapter_id: String,
    manga_title: Option<String>,
    manga_cover: Option<String>,
    manga_status: Option<i64>,
    chapter_title: Option<String>,
    chapter_number: Option<f64>,
    created_at: i64,
}

impl From<NotificationInformationRow> for NotificationInformation {
    fn from(value: NotificationInformationRow) -> Self {
        Self {
            id: value.id,
            chapter_id: ChapterId::from_strings(value.source_id, value.manga_id, value.chapter_id),
            manga_title: value.manga_title.unwrap_or("Unknown".to_owned()),
            manga_cover: value
                .manga_cover
                .map(|u| url::Url::parse(&u).map(Some).unwrap_or(None))
                .unwrap_or(None),
            manga_status: value.manga_status,
            chapter_title: value.chapter_title.unwrap_or("Unknown".to_owned()),
            chapter_number: value.chapter_number.unwrap_or(-1.0),
            created_at: value.created_at,
        }
    }
}
