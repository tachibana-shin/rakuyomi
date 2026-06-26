use std::path::PathBuf;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use log::error;
use serde::Serialize;

use shared::usecases::{
    fetch_manga_chapter::Error as FetchMangaChaptersError,
    search_mangas::Error as SearchMangasError,
};

pub(crate) fn setcap_hint() -> String {
    let bin_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("./server"));
    let display = bin_path.display();
    format!(
        "\n\nHint: Run the following command to grant mount capability:\n  sudo setcap cap_sys_admin+ep {display}\n\nThen restart the server."
    )
}

pub enum AppError {
    SourceNotFound,
    NotFound,
    DownloadAllChaptersProgressNotFound,
    NetworkFailure(anyhow::Error),
    Other(anyhow::Error),
    MountTmpFs(anyhow::Error),
}

#[derive(Serialize, Clone)]
pub struct ErrorResponse {
    pub message: String,
}

impl AppError {
    pub fn from_search_mangas_error(value: SearchMangasError) -> Self {
        match value {
            SearchMangasError::SourceError(e) => Self::NetworkFailure(e),
        }
    }

    pub fn from_fetch_manga_chapters_error(value: FetchMangaChaptersError) -> Self {
        match value {
            FetchMangaChaptersError::DownloadError(e) => Self::NetworkFailure(e),
            FetchMangaChaptersError::Other(e) => Self::Other(e),
        }
    }
}

impl From<&AppError> for StatusCode {
    fn from(value: &AppError) -> Self {
        match &value {
            AppError::SourceNotFound
            | AppError::NotFound
            | AppError::DownloadAllChaptersProgressNotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<&AppError> for ErrorResponse {
    fn from(value: &AppError) -> Self {
        let message = match value {
            AppError::SourceNotFound => "Source was not found".to_string(),
            AppError::NotFound => "Requested item was not found".to_string(),
            AppError::DownloadAllChaptersProgressNotFound => {
                "No download is in progress.".to_string()
            }
            AppError::NetworkFailure(_) => {
                "There was a network error. Check your connection and try again.".to_string()
            }
            AppError::MountTmpFs(ref e) => format!("Failed to mount tmpfs: {}{}", e, setcap_hint()),
            AppError::Other(ref e) => {
                eprintln!("Unexpected error: {:?}", e);

                format!("Something went wrong: {}", e)
            }
        };

        Self { message }
    }
}

impl From<AppError> for ErrorResponse {
    fn from(value: AppError) -> Self {
        Self::from(&value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status_code = StatusCode::from(&self);
        let error_response = ErrorResponse::from(&self);

        let inner_exception = match self {
            Self::NetworkFailure(ref e) => Some(e),
            Self::Other(ref e) => Some(e),
            _ => None,
        };

        if let Some(e) = inner_exception {
            error!("Error caused by: {:?}", e);
        }

        (status_code, Json(error_response)).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self::Other(err.into())
    }
}
