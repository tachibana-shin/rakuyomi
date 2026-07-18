use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, post_json, title_candidates, Tracker};

const ANILIST_API_URL: &str = "https://graphql.anilist.co";

/// AniList FuzzyDate: `{ year: Int, month: Int, day: Int }`.
/// Fields are optional — partial dates are common.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AniListDate {
    pub year: Option<i64>,
    pub month: Option<i64>,
    pub day: Option<i64>,
}

impl AniListDate {
    fn from_timestamp(ts: i64) -> Option<Self> {
        let date = Utc.timestamp_opt(ts, 0).single()?.date_naive();
        Some(AniListDate {
            year: Some(date.year() as i64),
            month: Some(date.month() as i64),
            day: Some(date.day() as i64),
        })
    }

    fn to_timestamp(self) -> Option<i64> {
        let year = self.year? as i32;
        let month = self.month? as u32;
        let day = self.day? as u32;
        let date = NaiveDate::from_ymd_opt(year, month, day)?;
        let datetime = date.and_hms_opt(0, 0, 0)?;
        Some(Utc.from_utc_datetime(&datetime).timestamp())
    }
}

pub struct AnilistTracker;

impl Tracker for AnilistTracker {
    fn service(&self) -> TrackingService {
        TrackingService::Anilist
    }

    async fn search(&self, _settings: &Settings, query: &str) -> Result<Vec<TrackingCandidate>> {
        #[derive(Serialize)]
        struct GraphQlRequest<'a> {
            query: &'a str,
            variables: serde_json::Value,
        }

        #[derive(Deserialize)]
        struct GraphQlResponse {
            data: Option<SearchData>,
        }

        #[derive(Deserialize)]
        struct SearchData {
            #[serde(rename = "Page")]
            page: SearchPage,
        }

        #[derive(Deserialize)]
        struct SearchPage {
            media: Vec<MediaNode>,
        }

        #[derive(Deserialize)]
        struct MediaNode {
            id: i64,
            #[serde(rename = "siteUrl")]
            site_url: Option<Url>,
            chapters: Option<i64>,
            volumes: Option<i64>,
            title: MediaTitle,
        }

        #[derive(Deserialize)]
        struct MediaTitle {
            romaji: Option<String>,
            english: Option<String>,
            native: Option<String>,
            #[serde(rename = "userPreferred")]
            user_preferred: Option<String>,
        }

        let request = GraphQlRequest {
            query: r#"
                query ($search: String!) {
                  Page(page: 1, perPage: 5) {
                    media(search: $search, type: MANGA, sort: SEARCH_MATCH) {
                      id
                      idMal
                      siteUrl
                      chapters
                      volumes
                      title {
                        romaji
                        english
                        native
                        userPreferred
                      }
                    }
                  }
                }
            "#,
            variables: serde_json::json!({ "search": query }),
        };

        let client = build_client();
        let response: GraphQlResponse = post_json(&client, ANILIST_API_URL, &request, None).await?;

        Ok(response
            .data
            .map(|data| {
                data.page
                    .media
                    .into_iter()
                    .map(|media| TrackingCandidate {
                        service: TrackingService::Anilist,
                        remote_media_id: media.id,
                        title: title_candidates(
                            media.title.user_preferred,
                            media.title.romaji,
                            media.title.english,
                            media.title.native,
                        ),
                        url: media.site_url,
                        total_chapters: media.chapters,
                        total_volumes: media.volumes,
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn fetch_progress(
        &self,
        settings: &Settings,
        media_id: i64,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Serialize)]
        struct GraphQlRequest<'a> {
            query: &'a str,
            variables: serde_json::Value,
        }

        #[derive(Deserialize)]
        struct GraphQlResponse {
            data: Option<MediaData>,
        }

        #[derive(Deserialize)]
        struct MediaData {
            #[serde(rename = "Media")]
            media: Option<MediaWithEntry>,
        }

        #[derive(Deserialize)]
        struct MediaWithEntry {
            #[serde(rename = "mediaListEntry")]
            media_list_entry: Option<AniListEntry>,
        }

        #[derive(Deserialize)]
        struct AniListEntry {
            status: Option<String>,
            progress: Option<i64>,
            #[serde(rename = "progressVolumes")]
            progress_volumes: Option<i64>,
            #[serde(rename = "updatedAt")]
            updated_at: Option<i64>,
            #[serde(rename = "startedAt")]
            started_at: Option<AniListDate>,
            #[serde(rename = "completedAt")]
            completed_at: Option<AniListDate>,
        }

        let token = require_token(settings)?;
        let client = build_client();
        let request = GraphQlRequest {
            query: r#"
                query ($mediaId: Int!) {
                  Media(id: $mediaId, type: MANGA) {
                    mediaListEntry {
                      status
                      progress
                      progressVolumes
                      updatedAt
                      startedAt { year month day }
                      completedAt { year month day }
                    }
                  }
                }
            "#,
            variables: serde_json::json!({ "mediaId": media_id }),
        };
        let response: GraphQlResponse =
            post_json(&client, ANILIST_API_URL, &request, Some(token)).await?;

        let maybe_entry = response
            .data
            .and_then(|data| data.media)
            .and_then(|media| media.media_list_entry);

        Ok(maybe_entry
            .map(|entry| TrackingProgressSnapshot {
                status: entry.status.as_deref().and_then(parse_status),
                chapter_progress: entry.progress,
                volume_progress: entry.progress_volumes,
                updated_at: entry.updated_at,
                started_at: entry.started_at.and_then(|d| d.to_timestamp()),
                completed_at: entry.completed_at.and_then(|d| d.to_timestamp()),
            })
            .unwrap_or_default())
    }

    async fn push_progress(
        &self,
        settings: &Settings,
        media_id: i64,
        snapshot: &TrackingProgressSnapshot,
    ) -> Result<TrackingProgressSnapshot> {
        #[derive(Serialize)]
        struct GraphQlRequest<'a> {
            query: &'a str,
            variables: serde_json::Value,
        }

        #[derive(Deserialize)]
        struct GraphQlResponse {
            data: Option<MutationData>,
        }

        #[derive(Deserialize)]
        struct MutationData {
            #[serde(rename = "SaveMediaListEntry")]
            save_media_list_entry: AniListEntry,
        }

        #[derive(Deserialize)]
        struct AniListEntry {
            status: Option<String>,
            progress: Option<i64>,
            #[serde(rename = "progressVolumes")]
            progress_volumes: Option<i64>,
            #[serde(rename = "updatedAt")]
            updated_at: Option<i64>,
            #[serde(rename = "startedAt")]
            started_at: Option<AniListDate>,
            #[serde(rename = "completedAt")]
            completed_at: Option<AniListDate>,
        }

        let token = require_token(settings)?;
        let client = build_client();
        let request = GraphQlRequest {
            query: r#"
                mutation (
                  $mediaId: Int!,
                  $status: MediaListStatus,
                  $progress: Int,
                  $progressVolumes: Int,
                  $startedAt: FuzzyDateInput,
                  $completedAt: FuzzyDateInput
                ) {
                  SaveMediaListEntry(
                    mediaId: $mediaId,
                    status: $status,
                    progress: $progress,
                    progressVolumes: $progressVolumes,
                    startedAt: $startedAt,
                    completedAt: $completedAt
                  ) {
                    status
                    progress
                    progressVolumes
                    updatedAt
                    startedAt { year month day }
                    completedAt { year month day }
                  }
                }
            "#,
            variables: serde_json::json!({
                "mediaId": media_id,
                "status": snapshot.status.as_ref().map(|status| format_status(*status)),
                "progress": snapshot.chapter_progress.unwrap_or(0),
                "progressVolumes": snapshot.volume_progress.unwrap_or(0),
                "startedAt": snapshot.started_at.and_then(AniListDate::from_timestamp),
                "completedAt": snapshot.completed_at.and_then(AniListDate::from_timestamp),
            }),
        };

        let response: GraphQlResponse =
            post_json(&client, ANILIST_API_URL, &request, Some(token)).await?;
        let entry = response
            .data
            .context("AniList did not return updated entry data")?
            .save_media_list_entry;

        Ok(TrackingProgressSnapshot {
            status: entry.status.as_deref().and_then(parse_status),
            chapter_progress: entry.progress,
            volume_progress: entry.progress_volumes,
            updated_at: entry.updated_at,
            started_at: entry.started_at.and_then(|d| d.to_timestamp()),
            completed_at: entry.completed_at.and_then(|d| d.to_timestamp()),
        })
    }

    async fn get_user(&self, settings: &Settings) -> Result<Option<String>> {
        #[derive(Serialize)]
        struct GraphQlRequest<'a> {
            query: &'a str,
        }

        #[derive(Deserialize)]
        struct GraphQlResponse {
            data: Option<ViewerData>,
        }

        #[derive(Deserialize)]
        struct ViewerData {
            #[serde(rename = "Viewer")]
            viewer: Option<Viewer>,
        }

        #[derive(Deserialize)]
        struct Viewer {
            name: Option<String>,
        }

        let token = match require_token(settings) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };
        let client = build_client();
        let request = GraphQlRequest {
            query: r#"
                query {
                  Viewer {
                    name
                  }
                }
            "#,
        };
        let response: GraphQlResponse =
            post_json(&client, ANILIST_API_URL, &request, Some(token)).await?;

        Ok(response
            .data
            .and_then(|data| data.viewer)
            .and_then(|v| v.name))
    }
}

fn require_token(settings: &Settings) -> Result<&str> {
    settings
        .anilist
        .access_token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .context("AniList access token is not configured")
}

fn parse_status(status: &str) -> Option<TrackingStatus> {
    match status {
        "CURRENT" => Some(TrackingStatus::Current),
        "COMPLETED" => Some(TrackingStatus::Completed),
        "PAUSED" => Some(TrackingStatus::Paused),
        "DROPPED" => Some(TrackingStatus::Dropped),
        "PLANNING" => Some(TrackingStatus::Planning),
        "REPEATING" => Some(TrackingStatus::Repeating),
        _ => None,
    }
}

fn format_status(status: TrackingStatus) -> &'static str {
    match status {
        TrackingStatus::Current => "CURRENT",
        TrackingStatus::Completed => "COMPLETED",
        TrackingStatus::Paused => "PAUSED",
        TrackingStatus::Dropped => "DROPPED",
        TrackingStatus::Planning => "PLANNING",
        TrackingStatus::Repeating => "REPEATING",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_all_variants() {
        assert_eq!(parse_status("CURRENT"), Some(TrackingStatus::Current));
        assert_eq!(parse_status("COMPLETED"), Some(TrackingStatus::Completed));
        assert_eq!(parse_status("PAUSED"), Some(TrackingStatus::Paused));
        assert_eq!(parse_status("DROPPED"), Some(TrackingStatus::Dropped));
        assert_eq!(parse_status("PLANNING"), Some(TrackingStatus::Planning));
        assert_eq!(parse_status("REPEATING"), Some(TrackingStatus::Repeating));
        assert_eq!(parse_status("UNKNOWN"), None);
    }

    #[test]
    fn format_status_roundtrip() {
        let statuses = [
            TrackingStatus::Current,
            TrackingStatus::Completed,
            TrackingStatus::Paused,
            TrackingStatus::Dropped,
            TrackingStatus::Planning,
            TrackingStatus::Repeating,
        ];
        for status in statuses {
            let formatted = format_status(status);
            assert_eq!(parse_status(formatted), Some(status));
        }
    }
}
