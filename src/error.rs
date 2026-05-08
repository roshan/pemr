use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use maud::html;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Multipart(#[from] axum::extract::multipart::MultipartError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            AppError::BadRequest(s) => (StatusCode::BAD_REQUEST, s.clone()),
            AppError::Sqlx(sqlx::Error::RowNotFound) => {
                (StatusCode::NOT_FOUND, "not found".to_string())
            }
            other => {
                tracing::error!(error = ?other, "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };
        let body = html! {
            html {
                head { title { "error" } }
                body {
                    h1 { (status.as_u16()) " " (status.canonical_reason().unwrap_or("")) }
                    p { (msg) }
                    p { a href="/" { "home" } }
                }
            }
        };
        (status, body).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
