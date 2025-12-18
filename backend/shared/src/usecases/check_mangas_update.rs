use std::sync::atomic::Ordering;
use std::{collections::HashSet, sync::atomic::AtomicBool};

use anyhow::{bail, Result};
use once_cell::sync::Lazy;
use tokio_util::sync::CancellationToken;

use crate::settings::Settings;
use crate::{
    arima_light::{fit_arima_from_chapters, ArimaSpec},
    chapter_storage::ChapterStorage,
    database::Database,
    model::MangaId,
    source::model::PublishingStatus,
    source_collection::SourceCollection,
    source_manager::SourceManager,
    usecases::{refresh_manga_chapters, refresh_manga_details},
};

pub async fn check_mangas_update(
    token: &CancellationToken,
    db: &Database,
    chapter_storage: &ChapterStorage,
    source_manager: &SourceManager,
) {
    let mangas_library = db.get_manga_library_and_status().await;

    for (manga, status) in mangas_library {
        if let Err(error) =
            check_manga_update(token, db, chapter_storage, source_manager, &manga, &status).await
        {
            eprintln!("Warn[{}]: {}", manga.value(), error);
            continue;
        }
    }
}

async fn check_manga_update(
    token: &CancellationToken,
    db: &Database,
    chapter_storage: &ChapterStorage,
    source_manager: &SourceManager,

    manga: &MangaId,
    status: &PublishingStatus,
) -> Result<()> {
    let spec = ArimaSpec {
        p: 1,
        d: 1,
        q: 1,
        rolling_window: Some(30),
        min_points: 6,
    };

    if *status == PublishingStatus::Completed {
        db.delete_last_check_update_manga(&manga).await;
        return Ok(());
    }

    let Some(source) = source_manager.get_by_id(&manga.source_id()) else {
        bail!(
            "Missing source {} – skip manga {}",
            manga.source_id().value(),
            manga.value(),
        )
    };

    let status = match refresh_manga_details(token, db, chapter_storage, source, &manga, 60).await {
        Ok(status) => {
            if status == PublishingStatus::Completed {
                db.delete_last_check_update_manga(&manga).await;
            }

            status
        }
        Err(err) => {
            bail!(
                "refresh_manga_details failed for manga {}<{}> ({:?})",
                manga.value(),
                manga.source_id().value(),
                err
            )
        }
    };

    let old_chapters = db.find_cached_chapter_informations(&manga).await;
    let new_chapters = match refresh_manga_chapters(token, db, source, &manga, 60).await {
        Ok(chaps) => chaps,
        Err(err) => {
            bail!(
                "refresh_manga_chapters failed for manga {}: {:?}",
                manga.value(),
                err
            )
        }
    };

    let added_chapters = if old_chapters.len() == 0 {
        [].into()
    } else {
        compute_new_chapters(&old_chapters, &new_chapters)
    };

    if status != PublishingStatus::Completed {
        let maybe_model = fit_arima_from_chapters(&new_chapters, spec);

        let next_ts_update = match maybe_model {
            Ok(model) => {
                let last_check_no_update = if added_chapters.len() == 0 {
                    Some(chrono::Utc::now().timestamp())
                } else {
                    db.get_last_check_update_manga(&manga).await.map(|t| t.0)
                };
                let next_ts = model.forecast_1_from_chapters(&new_chapters, last_check_no_update);

                next_ts
            }
            Err(err) => {
                eprintln!("{}", err);

                None
            }
        }
        .unwrap_or_else(|| chrono::Utc::now().timestamp() + 24 * 60 * 60);

        db.set_last_check_update_manga(&manga, chrono::Utc::now().timestamp(), next_ts_update)
            .await;
    }

    let _ = db.insert_notification(&manga, &added_chapters).await;

    Ok(())
}

fn compute_new_chapters(
    old_chapters: &[crate::model::ChapterInformation],
    new_chapters: &[crate::model::ChapterInformation],
) -> Vec<crate::model::ChapterInformation> {
    let old_ids: HashSet<_> = old_chapters.iter().map(|c| c.id.value()).collect();

    new_chapters
        .iter()
        .filter(|c| !old_ids.contains(&c.id.value()))
        .cloned()
        .collect()
}

// ===== cron =====
static CRON_RUNNING: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

pub async fn run_manga_cron(
    db: &Database,
    chapter_storage: &ChapterStorage,
    source_manager: &SourceManager,
    settings: &Settings,
) {
    if CRON_RUNNING.load(Ordering::SeqCst) {
        println!("Cron already running, skipping this tick");
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        return;
    }

    CRON_RUNNING.store(true, Ordering::SeqCst);

    println!("Cron started");
    let token = &CancellationToken::new();
    loop {
        let now = chrono::Utc::now().timestamp();
        let source_skip_cron = settings.source_skip_cron.clone().unwrap_or("".to_owned());
        let skip_sources: Vec<_> = source_skip_cron.split(",").into_iter().collect();

        let mut next_manga = db.get_next_ts_arima_min(&skip_sources).await;
        if next_manga.is_none() {
            println!("Next manga not found. Re-check all mangas");

            check_mangas_update(token, &db, &chapter_storage, &source_manager).await;
            next_manga = db.get_next_ts_arima_min(&skip_sources).await;

            if next_manga.is_none() {
                break;
            }
        }

        let wait_secs = next_manga.unwrap().1 - now;

        println!("Cron waiting {wait_secs}s");

        if wait_secs >= 0 {
            tokio::time::sleep(std::time::Duration::from_secs(
                wait_secs.try_into().unwrap(),
            ))
            .await;
        }

        let due_mangas = db.get_due_mangas().await;

        for (manga_id, status) in due_mangas {
            if skip_sources.contains(&manga_id.source_id().value().as_str()) {
                continue;
            }

            if let Err(err) = check_manga_update(
                token,
                &db,
                &chapter_storage,
                &source_manager,
                &manga_id,
                &status,
            )
            .await
            {
                eprintln!(
                    "[ERROR] check_mangas_update failed for {}: {:?}",
                    manga_id.value(),
                    err
                );
            }
        }
    }

    CRON_RUNNING.store(false, Ordering::SeqCst);
}
