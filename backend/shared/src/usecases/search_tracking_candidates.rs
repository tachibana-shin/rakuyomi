use anyhow::Result;

use crate::{
    model::{TrackingCandidate, TrackingService},
    settings::Settings,
    tracking,
};

pub async fn search_tracking_candidates(
    settings: &mut Settings,
    service: TrackingService,
    query: &str,
) -> Result<Vec<TrackingCandidate>> {
    tracking::search_candidates(settings, service, query).await
}
