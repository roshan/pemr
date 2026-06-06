use axum::body::Body;
use axum::extract::{State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::files;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiPath, ApiQuery, ApiResult};
use crate::models::{Record, parse_subject_filter};

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub subject: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Vec<Record>>> {
    let subject = parse_subject_filter(q.subject.as_deref())
        .map_err(ApiError::bad_request)?;
    let kind = q
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records
          where ($1::uuid is null or subject_id = $1)
            and ($2::text is null or kind = $2)
          order by occurred_at desc nulls last, created_at desc
          limit $3 offset $4",
    )
    .bind(subject)
    .bind(kind.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn detail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Record>> {
    let row = sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

pub async fn file(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Response> {
    let (file_path, content_type, byte_size, title): (
        Option<String>,
        Option<String>,
        Option<i64>,
        String,
    ) = sqlx::query_as(
        "select file_path, content_type, byte_size, title from records where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;
    serve_file(&state, file_path, content_type, byte_size, &title).await
}

pub async fn preview(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Response> {
    let (path, ct, title): (Option<String>, Option<String>, String) = sqlx::query_as(
        "select preview_path, preview_content_type, title from records where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;
    serve_file(&state, path, ct, None, &title).await
}

pub async fn thumbnail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Response> {
    let (path, ct, title): (Option<String>, Option<String>, String) = sqlx::query_as(
        "select thumbnail_path, thumbnail_content_type, title from records where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;
    serve_file(&state, path, ct, None, &title).await
}

async fn serve_file(
    state: &AppState,
    file_path: Option<String>,
    content_type: Option<String>,
    byte_size: Option<i64>,
    title: &str,
) -> ApiResult<Response> {
    let rel = file_path.ok_or_else(ApiError::not_found)?;
    let abs = files::resolve(&state.files_dir, &rel)
        .ok_or_else(|| ApiError::bad_request("invalid path"))?;
    let f = File::open(&abs).await?;
    let stream = ReaderStream::new(f);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    let ct = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    if let Ok(v) = HeaderValue::from_str(&ct) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Some(n) = byte_size {
        if let Ok(v) = HeaderValue::from_str(&n.to_string()) {
            headers.insert(header::CONTENT_LENGTH, v);
        }
    }
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{}\"", sanitize(title))) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok((StatusCode::OK, headers, body).into_response())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
