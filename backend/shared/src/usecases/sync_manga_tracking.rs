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
            let local_snapshot =
                derive_remote_snapshot(local_snapshot.clone(), binding.total_chapters)?;
            let remote = tracking::push_progress(settings, &binding, &local_snapshot).await?;
            db.set_tracking_sync_state(
                manga_id,
                binding.service,
                remote.chapter_progress,
                remote.updated_at,
            )
            .await?;

            results.push(TrackingSyncResult {
                service: binding.service,
                direction,
                local_progress: local_snapshot.chapter_progress,
                remote_progress: remote.chapter_progress,
                message: format!("Pushed progress to {}", binding.service.display_name()),
            });
        }

        return Ok(results);
    }

    let chapters = db.find_cached_chapters(manga_id, chapter_storage).await?;
    for binding in bindings {
        let remote = tracking::fetch_remote_progress(settings, &binding).await?;
        apply_remote_progress(db, manga_id, &chapters, &remote).await?;
        db.set_tracking_sync_state(
            manga_id,
            binding.service,
            remote.chapter_progress,
            remote.updated_at,
        )
        .await?;

        results.push(TrackingSyncResult {
            service: binding.service,
            direction,
            local_progress: remote.chapter_progress,
            remote_progress: remote.chapter_progress,
            message: format!("Pulled progress from {}", binding.service.display_name()),
        });
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
        let remote = tracking::push_progress(settings, &binding, &local_snapshot).await?;
        db.set_tracking_sync_state(
            manga_id,
            binding.service,
            remote.chapter_progress,
            remote.updated_at,
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
        .filter(|binding| service.map(|value| value == binding.service).unwrap_or(true))
        .collect()
}

fn derive_remote_snapshot(
    snapshot: TrackingProgressSnapshot,
    total_chapters: Option<i64>,
) -> Result<TrackingProgressSnapshot> {
    let snapshot = tracking::sanitize_progress(&snapshot)?;
    let status = match (snapshot.chapter_progress.unwrap_or_default(), total_chapters) {
        (progress, Some(total)) if total > 0 && progress >= total => Some(TrackingStatus::Completed),
        (progress, _) if progress > 0 => Some(TrackingStatus::Current),
        _ => snapshot.status,
    };

    Ok(TrackingProgressSnapshot { status, ..snapshot })
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
