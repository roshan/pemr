//! `/api/v1/records` — read (incl. file bytes) + multipart upsert write.
//!
//! A record is **bytes + provenance**, so this is the one write endpoint that
//! takes `multipart/form-data` rather than a JSON body (the response is still
//! JSON, like every other endpoint). It mirrors the UI's `POST /records`
//! handler — same content-addressed storage, same thumbnailing, same
//! `link_incident` join-row — so the two upload paths can't drift.
//!
//! **DICOM is deliberately not parsed here.** `POST /records/import` owns that
//! (pixel decode → PNG preview → tag extraction → one record per image), and it
//! is a fundamentally different shape: one upload fans out to many records. Post
//! a `.dcm` here and you get exactly one record holding the original bytes, no
//! preview and no `dicom_metadata` — which is the same thing the UI's plain
//! upload form does.

use axum::body::Body;
use axum::extract::{State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use bytes::Bytes;
use serde::Deserialize;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::files::{self, StoredFile};
use crate::handlers::AppState;
use crate::handlers::api::{
    ApiError, ApiMultipart, ApiPath, ApiQuery, ApiResult, provenance_conflict, validate_in,
    write_err,
};
use crate::images;
use crate::models::{DATE_PRECISIONS, RECORD_KINDS, Record, parse_date, parse_subject_filter};

/// Per-file ceiling, matching the UI upload handler's `MAX_UPLOAD_BYTES`. The
/// app-wide body limit (1 GiB, set in `main.rs`) is the outer bound; this is
/// the friendlier per-part error.
const MAX_UPLOAD_BYTES: usize = 256 * 1024 * 1024;

/// Every column of `records`, in `Record`'s field order — the same list the
/// `select`s above use, so the read and write shapes stay identical. (`*` would
/// also work: the generated `search_tsv` is simply never decoded, since sqlx's
/// `FromRow` reads only the columns the struct names.)
const COLS: &str = "id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                    file_path, content_type, byte_size, sha256,
                    preview_path, preview_content_type,
                    thumbnail_path, thumbnail_content_type, study_instance_uid,
                    dicom_metadata, instance_number,
                    source_id, external_id, external_url, source_synced_at,
                    created_at, updated_at";

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

// ---------------------------------------------------------------------------
// POST /api/v1/records — multipart upsert
// ---------------------------------------------------------------------------

/// The multipart form, collected before any validation so a bad field reports
/// its own error rather than whichever one happened to stream first.
#[derive(Default)]
struct Form {
    subject_id: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    notes: Option<String>,
    occurred_at: Option<String>,
    occurred_precision: Option<String>,
    source_id: Option<String>,
    external_id: Option<String>,
    external_url: Option<String>,
    link_incident: Option<String>,
    file_bytes: Option<Bytes>,
    file_name: Option<String>,
    file_content_type: Option<String>,
}

/// Parse a uuid field, naming the field in the 400 so the caller knows which
/// one was malformed (axum's own message wouldn't).
fn uuid_field(field: &str, raw: Option<&String>) -> ApiResult<Option<Uuid>> {
    match raw.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => Uuid::parse_str(s)
            .map(Some)
            .map_err(|e| ApiError::bad_request(format!("invalid {field}: {e}"))),
    }
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiMultipart(mut multipart): ApiMultipart,
) -> ApiResult<Json<Record>> {
    let mut f = Form::default();
    while let Some(field) = multipart.next_field().await? {
        match field.name().unwrap_or("").to_string().as_str() {
            "subject_id" => f.subject_id = Some(field.text().await?),
            "kind" => f.kind = Some(field.text().await?),
            "title" => f.title = Some(field.text().await?),
            "notes" => f.notes = Some(field.text().await?),
            "occurred_at" => f.occurred_at = Some(field.text().await?),
            "occurred_precision" => f.occurred_precision = Some(field.text().await?),
            "source_id" => f.source_id = Some(field.text().await?),
            "external_id" => f.external_id = Some(field.text().await?),
            "external_url" => f.external_url = Some(field.text().await?),
            "link_incident" => f.link_incident = Some(field.text().await?),
            "file" => {
                f.file_name = field.file_name().map(|s| s.to_string());
                f.file_content_type = field.content_type().map(|s| s.to_string());
                let bytes = field.bytes().await?;
                if bytes.len() > MAX_UPLOAD_BYTES {
                    return Err(ApiError::bad_request(format!(
                        "upload too large: {} bytes (max {MAX_UPLOAD_BYTES})",
                        bytes.len()
                    )));
                }
                // An empty part means "no file" (a note record), not a 0-byte file.
                if !bytes.is_empty() {
                    f.file_bytes = Some(bytes);
                }
            }
            // Unknown fields are ignored, but their bodies must still be drained
            // or the multipart stream stalls on the next `next_field()`.
            _ => {
                let _ = field.bytes().await?;
            }
        }
    }

    let subject_id = uuid_field("subject_id", f.subject_id.as_ref())?
        .ok_or_else(|| ApiError::bad_request("subject_id required"))?;
    let kind = f
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("kind required"))?
        .to_string();
    validate_in("kind", &kind, RECORD_KINDS)?;
    let title = f
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("title required"))?
        .to_string();
    let occurred_at = parse_date(f.occurred_at.as_deref().unwrap_or(""))
        .map_err(ApiError::bad_request)?;
    let occurred_precision = f
        .occurred_precision
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("day")
        .to_string();
    validate_in("occurred_precision", &occurred_precision, DATE_PRECISIONS)?;
    let source_id = uuid_field("source_id", f.source_id.as_ref())?;
    let link_incident = uuid_field("link_incident", f.link_incident.as_ref())?;
    let external_id = f.external_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let external_url = f.external_url.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let notes = f.notes.unwrap_or_default();

    // Content-addressed storage: identical bytes reuse the same file on disk
    // (`store_bytes` no-ops when the path exists), so re-uploading is cheap.
    let stored: Option<StoredFile> = match &f.file_bytes {
        None => None,
        Some(bytes) => {
            let ext = f
                .file_name
                .as_deref()
                .and_then(files::extension_from_filename)
                .or_else(|| {
                    f.file_content_type
                        .as_deref()
                        .and_then(files::extension_from_content_type)
                });
            Some(files::store_bytes(&state.files_dir, bytes, ext).await?)
        }
    };
    let stored_thumb: Option<StoredFile> = match (&f.file_bytes, f.file_content_type.as_deref()) {
        (Some(bytes), Some(ct)) if images::can_thumbnail(ct) => {
            match images::thumbnail_webp(bytes, 400) {
                Ok(webp) => Some(files::store_bytes(&state.files_dir, &webp, Some("webp")).await?),
                // A record with no thumbnail still beats a failed upload.
                Err(e) => {
                    tracing::warn!(error = %e, "thumbnail generation failed; continuing without one");
                    None
                }
            }
        }
        _ => None,
    };

    // `coalesce` on every file column so a metadata-only re-POST (same
    // provenance key, no `file` part) edits the row without orphaning its file.
    let has_keys = source_id.is_some() && external_id.is_some();
    let set = "subject_id=excluded.subject_id, kind=excluded.kind, title=excluded.title, \
        notes=excluded.notes, occurred_at=excluded.occurred_at, \
        occurred_precision=excluded.occurred_precision, \
        file_path=coalesce(excluded.file_path, records.file_path), \
        content_type=coalesce(excluded.content_type, records.content_type), \
        byte_size=coalesce(excluded.byte_size, records.byte_size), \
        sha256=coalesce(excluded.sha256, records.sha256), \
        thumbnail_path=coalesce(excluded.thumbnail_path, records.thumbnail_path), \
        thumbnail_content_type=coalesce(excluded.thumbnail_content_type, records.thumbnail_content_type), \
        external_url=excluded.external_url";
    let sql = format!(
        "insert into records
            (id, subject_id, kind, title, notes, occurred_at, occurred_precision,
             file_path, content_type, byte_size, sha256,
             thumbnail_path, thumbnail_content_type,
             source_id, external_id, external_url)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16){}
         returning {COLS}",
        provenance_conflict(has_keys, set),
    );
    // The row insert and the incident link are ONE transaction: a bad
    // `link_incident` must not leave a committed record behind, or a caller who
    // (correctly) retries after the 400 creates a duplicate. The stored file is
    // deliberately outside it — bytes are content-addressed, so an unreferenced
    // blob is inert and the next upload of the same bytes reuses it.
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query_as::<_, Record>(&sql)
        .bind(Uuid::now_v7())
        .bind(subject_id)
        .bind(&kind)
        .bind(&title)
        .bind(&notes)
        .bind(occurred_at)
        .bind(&occurred_precision)
        .bind(stored.as_ref().map(|s| s.relative_path.as_str()))
        .bind(f.file_content_type.as_deref())
        .bind(stored.as_ref().map(|s| s.byte_size))
        .bind(stored.as_ref().map(|s| s.sha256_hex.as_str()))
        .bind(stored_thumb.as_ref().map(|s| s.relative_path.as_str()))
        .bind(stored_thumb.as_ref().map(|_| "image/webp"))
        .bind(source_id)
        .bind(external_id)
        .bind(external_url)
        .fetch_one(&mut *tx)
        .await
        .map_err(write_err)?;

    // Link into an event, if asked. `on conflict do nothing` keeps a re-POST
    // idempotent; an unknown incident_id is an FK violation → 400, not a 500,
    // and rolls the record insert back with it.
    if let Some(incident_id) = link_incident {
        sqlx::query(
            "insert into incident_records (incident_id, record_id) values ($1,$2)
             on conflict do nothing",
        )
        .bind(incident_id)
        .bind(row.id)
        .execute(&mut *tx)
        .await
        .map_err(write_err)?;
    }
    tx.commit().await?;

    Ok(Json(row))
}
