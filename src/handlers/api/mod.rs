//! /api/v1/* — read-only JSON API gated by Bearer tokens (see [`crate::api_auth`]).
//!
//! Endpoints mirror the read shape of the UI. Every list endpoint accepts
//! `?subject=<uuid>` to filter by subject (matching the UI's subject
//! switcher); records additionally accept `?kind=<kind>`.
//!
//! Errors are JSON: `{"error": "..."}` with the appropriate HTTP status.
//! The default `AppError` renders HTML, so API handlers use `ApiError`
//! instead.

use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::error::AppError;

pub mod allergies;
pub mod appointments;
pub mod care_reminders;
pub mod conditions;
pub mod immunizations;
pub mod incidents;
pub mod me;
pub mod medications;
pub mod observations;
pub mod providers;
pub mod records;
pub mod root;
pub mod search;
pub mod sources;
pub mod subject_identifiers;
pub mod subject_providers;
pub mod subject_relationships;
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
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::CONFLICT, message: msg.into() }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

/// Validate a value against a fixed vocabulary (`const &[&str]` in `models`),
/// returning a structured 400 on mismatch. Used by the write endpoints.
pub fn validate_in(field: &str, value: &str, allowed: &[&str]) -> ApiResult<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "invalid {field}: {value:?} (expected one of {allowed:?})"
        )))
    }
}

/// `?limit=` default 100, capped at 500; `?offset=` default 0, floored at 0.
/// Matches the read endpoints' behavior (see `records::list`).
pub fn clamp_limit(l: Option<i64>) -> i64 {
    l.unwrap_or(100).clamp(1, 500)
}
pub fn clamp_offset(o: Option<i64>) -> i64 {
    o.unwrap_or(0).max(0)
}

/// Build the `ON CONFLICT (source_id, external_id) … DO UPDATE SET …` suffix
/// for the standard provenance partial-unique index. Returns an empty string
/// when the agent didn't supply both keys, so the statement is a plain insert
/// (the "always create when un-keyed" half of idempotent upsert). `set_cols`
/// is a static, caller-built `col = excluded.col, …` list — never user input.
pub fn provenance_conflict(has_keys: bool, set_cols: &str) -> String {
    if has_keys {
        format!(
            " on conflict (source_id, external_id) \
             where source_id is not null and external_id is not null \
             do update set {set_cols}, updated_at = now()"
        )
    } else {
        String::new()
    }
}

/// Map an insert/upsert `sqlx::Error` to a structured client error instead of
/// the read-path default (which collapses everything non-RowNotFound into 500).
/// FK violations → 400, unique violations → 409, other constraint/format
/// violations → 400; anything else is a genuine 500.
pub fn write_err(e: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(db) = &e {
        match db.code().as_deref() {
            // foreign_key_violation: a referenced subject/provider/source/etc. is missing.
            Some("23503") => {
                return ApiError::bad_request(format!("references a row that does not exist: {}", db.message()));
            }
            // unique_violation that slipped past an ON CONFLICT clause.
            Some("23505") => {
                return ApiError::conflict(format!("conflicts with an existing row: {}", db.message()));
            }
            // not_null / check / invalid_text_representation (bad uuid/number/date).
            Some("23502") | Some("23514") | Some("22P02") | Some("22007") | Some("22008") => {
                return ApiError::bad_request(db.message().to_string());
            }
            _ => {}
        }
    }
    tracing::error!(error = ?e, "api write failed (sqlx)");
    ApiError::internal("internal error")
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

/// Like `axum::Json<T>` for request bodies, but a malformed/invalid body is
/// rejected as an [`ApiError`] (our `{"error": ...}` JSON shape) instead of
/// axum's default plain-text rejection. Used by every write endpoint so the
/// API never breaks its JSON-error contract.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(ApiError::bad_request(rejection.body_text())),
        }
    }
}
