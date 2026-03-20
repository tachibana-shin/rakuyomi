use anyhow::Result;
use crate::{
    model::TrackingService,
    settings::Settings,
    tracking::validate_credentials,
};

pub async fn validate_tracking_settings(
    settings: &mut Settings,
    service: TrackingService,
) -> Result<()> {
    validate_credentials(settings, service).await
}
