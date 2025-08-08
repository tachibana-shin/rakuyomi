use axum::{
    extract::{FromRequestParts, Path},
    http::request::Parts,
};
use serde::Deserialize;
use shared::{model::SourceId, source::Source, source_collection::SourceCollection};

use crate::{AppError, State};

pub struct SourceExtractor(pub Source);

#[derive(Deserialize)]
pub struct SourceParams {
    pub source_id: String,
}

impl FromRequestParts<State> for SourceExtractor {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state @ State { source_manager, .. }: &State,
    ) -> Result<Self, Self::Rejection> {
        let Path(SourceParams { source_id }) = Path::from_request_parts(parts, state).await?;
        let source_manager = source_manager.lock().await;
        let source = source_manager
            .get_by_id(&SourceId::new(source_id))
            .ok_or(AppError::SourceNotFound)?;

        // FIXME this relies upon the `Source` being an `Arc` internally in
        // order to keep things performant.
        // If it wasn't, this would be pretty wasteful... and overall this is
        // pretty bad
        Ok(Self(source.clone()))
    }
}
