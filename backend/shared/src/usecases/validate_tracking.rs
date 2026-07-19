use crate::{model::TrackingService, settings::Settings, tracking::validate_credentials};
use anyhow::Result;

pub async fn validate_tracking_settings(
    settings: &mut Settings,
    service: TrackingService,
) -> Result<()> {
    validate_credentials(settings, service).await
}
