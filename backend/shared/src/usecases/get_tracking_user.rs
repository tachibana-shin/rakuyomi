use crate::{model::TrackingService, settings::Settings, tracking::get_tracking_user};
use anyhow::Result;

pub async fn get_tracking_user_info(
    settings: &mut Settings,
    service: TrackingService,
) -> Result<Option<String>> {
    let username = get_tracking_user(settings, service).await?;

    let svc = match service {
        TrackingService::Anilist => Some(&mut settings.anilist),
        TrackingService::MyAnimeList => Some(&mut settings.myanimelist),
        TrackingService::Shikimori => Some(&mut settings.shikimori),
        TrackingService::Bangumi => Some(&mut settings.bangumi),
        TrackingService::Mangabaka => Some(&mut settings.mangabaka),
        TrackingService::Kavita => Some(&mut settings.kavita),
        TrackingService::Komga => Some(&mut settings.komga),
        TrackingService::Suwayomi => Some(&mut settings.suwayomi),
    };
    if let Some(s) = svc {
        s.username = username.clone();
    }

    Ok(username)
}
