use anyhow::Result;

use crate::{
    chapter_storage::ChapterStorage,
    database::Database,
    model::{
        ChapterId, MangaId, TrackingProgressSnapshot, TrackingService, TrackingStatus,
        TrackingSyncDirection, TrackingSyncResult,
    },
    settings::Settings,
    tracking,
};

pub async fn sync_manga_tracking(
    db: &Database,
    chapter_storage: &ChapterStorage,
    settings: &mut Settings,
    manga_id: &MangaId,
    service: Option<TrackingService>,
    direction: TrackingSyncDirection,
) -> Result<Vec<TrackingSyncResult>> {
    let bindings = filter_bindings(db.list_tracking_bindings(manga_id).await?, service);
    let mut results = Vec::with_capacity(bindings.len());

    if matches!(direction, TrackingSyncDirection::Push) {
        let local_snapshot = db.get_local_tracking_progress(manga_id).await?;

        for binding in bindings {
            let result = (async {
                let local_snapshot =
                    derive_remote_snapshot(local_snapshot.clone(), binding.total_chapters)?;
                // Carry dates from the binding so push doesn't overwrite them with null.
                let local_snapshot = TrackingProgressSnapshot {
                    started_at: binding.started_at.or(local_snapshot.started_at),
                    completed_at: binding.completed_at.or(local_snapshot.completed_at),
                    ..local_snapshot
                };
                let remote = tracking::push_progress(settings, &binding, &local_snapshot).await?;
                db.set_tracking_sync_state(
                    manga_id,
                    binding.service,
                    remote.chapter_progress,
                    remote.updated_at,
                    remote.started_at,
                    remote.completed_at,
                )
                .await?;
                Ok::<_, anyhow::Error>((local_snapshot, remote))
            })
            .await;

            match result {
                Ok((local_snapshot, remote)) => {
                    results.push(TrackingSyncResult {
                        service: binding.service,
                        direction,
                        local_progress: local_snapshot.chapter_progress,
                        remote_progress: remote.chapter_progress,
                        message: format!("Pushed progress to {}", binding.service.display_name()),
                    });
                }
                Err(e) => {
                    results.push(TrackingSyncResult {
                        service: binding.service,
                        direction,
                        local_progress: None,
                        remote_progress: None,
                        message: format!(
                            "Failed to push to {}: {e}",
                            binding.service.display_name()
                        ),
                    });
                }
            }
        }

        return Ok(results);
    }

    let chapters = db
        .find_cached_chapters(manga_id, chapter_storage, true)
        .await?;
    for binding in bindings {
        let result = (async {
            let remote = tracking::fetch_remote_progress(settings, &binding).await?;
            apply_remote_progress(db, manga_id, &chapters, &remote).await?;
            db.set_tracking_sync_state(
                manga_id,
                binding.service,
                remote.chapter_progress,
                remote.updated_at,
                remote.started_at,
                remote.completed_at,
            )
            .await?;
            Ok::<_, anyhow::Error>(remote)
        })
        .await;

        match result {
            Ok(remote) => {
                results.push(TrackingSyncResult {
                    service: binding.service,
                    direction,
                    local_progress: remote.chapter_progress,
                    remote_progress: remote.chapter_progress,
                    message: format!("Pulled progress from {}", binding.service.display_name()),
                });
            }
            Err(e) => {
                results.push(TrackingSyncResult {
                    service: binding.service,
                    direction,
                    local_progress: None,
                    remote_progress: None,
                    message: format!(
                        "Failed to pull from {}: {e}",
                        binding.service.display_name()
                    ),
                });
            }
        }
    }

    Ok(results)
}

pub async fn sync_manga_tracking_push(
    db: &Database,
    settings: &mut Settings,
    manga_id: &MangaId,
) -> Result<Vec<TrackingSyncResult>> {
    let bindings = db.list_tracking_bindings(manga_id).await?;
    if bindings.is_empty() {
        return Ok(Vec::new());
    }

    let local_snapshot = db.get_local_tracking_progress(manga_id).await?;
    let mut results = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let local_snapshot =
            derive_remote_snapshot(local_snapshot.clone(), binding.total_chapters)?;
        // Carry dates from the binding so push doesn't overwrite them with null.
        let local_snapshot = TrackingProgressSnapshot {
            started_at: binding.started_at.or(local_snapshot.started_at),
            completed_at: binding.completed_at.or(local_snapshot.completed_at),
            ..local_snapshot
        };
        let remote = tracking::push_progress(settings, &binding, &local_snapshot).await?;
        db.set_tracking_sync_state(
            manga_id,
            binding.service,
            remote.chapter_progress,
            remote.updated_at,
            remote.started_at,
            remote.completed_at,
        )
        .await?;

        results.push(TrackingSyncResult {
            service: binding.service,
            direction: TrackingSyncDirection::Push,
            local_progress: local_snapshot.chapter_progress,
            remote_progress: remote.chapter_progress,
            message: format!("Pushed progress to {}", binding.service.display_name()),
        });
    }

    Ok(results)
}

fn filter_bindings(
    bindings: Vec<crate::model::TrackingBinding>,
    service: Option<TrackingService>,
) -> Vec<crate::model::TrackingBinding> {
    bindings
        .into_iter()
        .filter(|binding| {
            service
                .map(|value| value == binding.service)
                .unwrap_or(true)
        })
        .collect()
}

fn derive_remote_snapshot(
    snapshot: TrackingProgressSnapshot,
    total_chapters: Option<i64>,
) -> Result<TrackingProgressSnapshot> {
    let snapshot = tracking::sanitize_progress(&snapshot)?;
    let status = match (
        snapshot.chapter_progress.unwrap_or_default(),
        total_chapters,
    ) {
        (progress, Some(total)) if total > 0 && progress >= total => {
            Some(TrackingStatus::Completed)
        }
        (progress, _) if progress > 0 => Some(TrackingStatus::Current),
        _ => snapshot.status.or(Some(TrackingStatus::Current)),
    };

    let mut result = TrackingProgressSnapshot { status, ..snapshot };

    // Auto-set completed_at when status is derived to Completed but not yet set.
    if matches!(result.status, Some(TrackingStatus::Completed)) && result.completed_at.is_none() {
        result.completed_at = Some(chrono::Utc::now().timestamp());
    }

    Ok(result)
}

async fn apply_remote_progress(
    db: &Database,
    manga_id: &MangaId,
    chapters: &[crate::model::Chapter],
    remote: &TrackingProgressSnapshot,
) -> Result<()> {
    let ids = chapter_ids_from_remote_progress(chapters, remote);
    if ids.is_empty() {
        return Ok(());
    }

    db.set_chapters_read_state(manga_id, &ids, true).await?;
    Ok(())
}

fn chapter_ids_from_remote_progress(
    chapters: &[crate::model::Chapter],
    remote: &TrackingProgressSnapshot,
) -> Vec<ChapterId> {
    if matches!(remote.status, Some(TrackingStatus::Completed)) {
        return chapters
            .iter()
            .map(|chapter| chapter.information.id.clone())
            .collect();
    }

    let progress = match remote.chapter_progress {
        Some(progress) if progress > 0 => progress as f32,
        _ => return Vec::new(),
    };

    let numeric_matches: Vec<_> = chapters
        .iter()
        .filter(|chapter| {
            chapter
                .information
                .chapter_number
                .map(|number| number <= progress)
                .unwrap_or(false)
        })
        .map(|chapter| chapter.information.id.clone())
        .collect();

    if !numeric_matches.is_empty() {
        return numeric_matches;
    }

    chapters
        .iter()
        .take(progress as usize)
        .map(|chapter| chapter.information.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChapterInformation, ChapterState, MangaId, SourceId};

    fn make_test_chapters(count: usize) -> Vec<crate::model::Chapter> {
        let manga_id = MangaId::new(SourceId::new("src".into()), "m-1".into());
        (1..=count)
            .map(|i| crate::model::Chapter {
                information: ChapterInformation {
                    id: ChapterId::new(manga_id.clone(), format!("ch-{i}")),
                    title: Some(format!("Chapter {i}")),
                    scanlator: None,
                    chapter_number: Some(i as f32),
                    volume_number: None,
                    last_updated: None,
                    thumbnail: None,
                    lang: None,
                    url: None,
                    locked: None,
                },
                state: ChapterState::default(),
                downloaded: false,
                on_tmpfs: false,
            })
            .collect()
    }

    #[test]
    fn filter_bindings_by_service() {
        use crate::model::{TrackingBinding, TrackingService};

        let bindings = vec![
            TrackingBinding {
                service: TrackingService::Anilist,
                remote_media_id: 1,
                remote_title: "Manga A".into(),
                remote_url: None,
                total_chapters: None,
                total_volumes: None,
                last_synced_progress: None,
                last_synced_at: None,
                started_at: None,
                completed_at: None,
            },
            TrackingBinding {
                service: TrackingService::MyAnimeList,
                remote_media_id: 2,
                remote_title: "Manga B".into(),
                remote_url: None,
                total_chapters: None,
                total_volumes: None,
                last_synced_progress: None,
                last_synced_at: None,
                started_at: None,
                completed_at: None,
            },
        ];

        let filtered = filter_bindings(bindings.clone(), Some(TrackingService::Anilist));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].service, TrackingService::Anilist);

        let all = filter_bindings(bindings, None);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn derive_remote_snapshot_completed_when_all_read() {
        let snapshot = TrackingProgressSnapshot {
            status: Some(TrackingStatus::Current),
            chapter_progress: Some(10),
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        let derived = derive_remote_snapshot(snapshot, Some(10)).unwrap();
        assert_eq!(derived.status, Some(TrackingStatus::Completed));
    }

    #[test]
    fn derive_remote_snapshot_current_when_partial() {
        let snapshot = TrackingProgressSnapshot {
            status: Some(TrackingStatus::Planning),
            chapter_progress: Some(5),
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        let derived = derive_remote_snapshot(snapshot, Some(10)).unwrap();
        assert_eq!(derived.status, Some(TrackingStatus::Current));
    }

    #[test]
    fn derive_remote_snapshot_preserves_status_when_zero_progress() {
        let snapshot = TrackingProgressSnapshot {
            status: Some(TrackingStatus::Planning),
            chapter_progress: Some(0),
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        let derived = derive_remote_snapshot(snapshot, Some(10)).unwrap();
        assert_eq!(derived.status, Some(TrackingStatus::Planning));
    }

    #[test]
    fn chapter_ids_completed_returns_all() {
        let chapters = make_test_chapters(5);
        let remote = TrackingProgressSnapshot {
            status: Some(TrackingStatus::Completed),
            chapter_progress: None,
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        let ids = chapter_ids_from_remote_progress(&chapters, &remote);
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn chapter_ids_partial_progress_by_number() {
        let chapters = make_test_chapters(10);
        let remote = TrackingProgressSnapshot {
            status: Some(TrackingStatus::Current),
            chapter_progress: Some(3),
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        let ids = chapter_ids_from_remote_progress(&chapters, &remote);
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn chapter_ids_zero_progress_returns_empty() {
        let chapters = make_test_chapters(5);
        let remote = TrackingProgressSnapshot {
            status: Some(TrackingStatus::Planning),
            chapter_progress: Some(0),
            volume_progress: None,
            updated_at: None,
            started_at: None,
            completed_at: None,
        };
        let ids = chapter_ids_from_remote_progress(&chapters, &remote);
        assert!(ids.is_empty());
    }
}
