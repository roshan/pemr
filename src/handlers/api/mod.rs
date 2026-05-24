//! /api/v1/* — read-only JSON API gated by Bearer tokens (see [`crate::api_auth`]).
//!
//! Endpoints mirror the read shape of the UI. Every list endpoint accepts
//! `?subject=<uuid>` to filter by subject (matching the UI's subject
//! switcher); records additionally accept `?kind=<kind>`.
//!
//! Errors are JSON: `{"error": "..."}` with the appropriate HTTP status.
//! The default `AppError` renders HTML, so API handlers use `ApiError`
//! instead.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::error::AppError;

pub mod incidents;
pub mod me;
pub mod records;
pub mod root;
pub mod search;
pub mod sources;
pub mod subjects;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }
    pub fn not_found() -> Self {
        Self { status: StatusCode::NOT_FOUND, message: "not found".into() }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self::not_found(),
            other => {
                tracing::error!(error = ?other, "api request failed (sqlx)");
                Self::internal("internal error")
            }
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        tracing::error!(error = ?e, "api request failed (io)");
        Self::internal("internal error")
    }
}

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::NotFound => Self::not_found(),
            AppError::BadRequest(s) => Self::bad_request(s),
            AppError::Sqlx(sqlx::Error::RowNotFound) => Self::not_found(),
            other => {
                tracing::error!(error = ?other, "api request failed");
                Self::internal("internal error")
            }
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
