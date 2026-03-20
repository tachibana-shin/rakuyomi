use anyhow::{Context, Result};
use reqwest::header::AUTHORIZATION;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    model::{TrackingCandidate, TrackingProgressSnapshot, TrackingService, TrackingStatus},
    settings::Settings,
};

use super::{build_client, post_json, title_candidates};

const ANILIST_API_URL: &str = "https://graphql.anilist.co";

pub struct AnilistTracker;

impl AnilistTracker {
    pub async fn search(&self, query: &str) -> Result<Vec<TrackingCandidate>> {
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
                        service: TrackingService::AniList,
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

    pub async fn fetch_progress(
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
            })
            .unwrap_or_default())
    }

    pub async fn push_progress(
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
        }

        let token = require_token(settings)?;
        let client = build_client();
        let request = GraphQlRequest {
            query: r#"
                mutation ($mediaId: Int!, $status: MediaListStatus, $progress: Int, $progressVolumes: Int) {
                  SaveMediaListEntry(
                    mediaId: $mediaId,
                    status: $status,
                    progress: $progress,
                    progressVolumes: $progressVolumes
                  ) {
                    status
                    progress
                    progressVolumes
                    updatedAt
                  }
                }
            "#,
            variables: serde_json::json!({
                "mediaId": media_id,
                "status": snapshot.status.as_ref().map(|status| format_status(*status)),
                "progress": snapshot.chapter_progress,
                "progressVolumes": snapshot.volume_progress,
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
        })
    }
}

fn require_token(settings: &Settings) -> Result<&str> {
    settings
        .anilist_access_token
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
