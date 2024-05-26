use super::AppJson;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub enum AppError {
    EyreError(eyre::Report),
    R2D2Error(r2d2::Error),
    RusqliteError(rusqlite::Error),
    Error(String),
}

impl From<r2d2::Error> for AppError {
    fn from(error: r2d2::Error) -> Self {
        Self::R2D2Error(error)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::RusqliteError(error)
    }
}

impl From<String> for AppError {
    fn from(error: String) -> Self {
        Self::Error(error)
    }
}

impl From<eyre::Report> for AppError {
    fn from(error: eyre::Report) -> Self {
        Self::EyreError(error)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // How we want errors responses to be serialized
        #[derive(Serialize)]
        struct ErrorResponse {
            message: String,
        }

        let (status, message) = match self {
            AppError::RusqliteError(err) => {
                tracing::error!(%err, "error from rusqlite");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong".to_owned(),
                )
            }
            AppError::R2D2Error(err) => {
                tracing::error!(%err, "error from r2d2");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong".to_owned(),
                )
            }
            AppError::Error(message) => {
                tracing::error!("app error: {}", message);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong".to_owned(),
                )
            }
            AppError::EyreError(err) => {
                tracing::error!(%err, "error from eyre");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong".to_owned(),
                )
            }
        };

        (status, AppJson(ErrorResponse { message })).into_response()
    }
}
