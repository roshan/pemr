use axum::extract::{State};
use axum::response::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiPath, ApiQuery, ApiResult};
use crate::models::{Incident, Record, parse_subject_filter};

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub subject: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Vec<Incident>>> {
    let subject = parse_subject_filter(q.subject.as_deref())
        .map_err(ApiError::bad_request)?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                ended_at, ended_precision, created_at, updated_at
           from incidents
          where ($1::uuid is null or subject_id = $1)
          order by occurred_at desc nulls last, created_at desc
          limit $2 offset $3",
    )
    .bind(subject)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize)]
pub struct IncidentDetail {
    #[serde(flatten)]
    pub incident: Incident,
    pub records: Vec<Record>,
}

pub async fn detail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<IncidentDetail>> {
    let incident = sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                ended_at, ended_precision, created_at, updated_at
           from incidents where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;

    let records = sqlx::query_as::<_, Record>(
        "select r.id, r.subject_id, r.kind, r.title, r.notes, r.occurred_at, r.occurred_precision,
                r.file_path, r.content_type, r.byte_size, r.sha256,
                r.preview_path, r.preview_content_type,
                r.thumbnail_path, r.thumbnail_content_type, r.study_instance_uid,
                r.dicom_metadata, r.instance_number,
                r.source_id, r.external_id, r.external_url, r.source_synced_at,
                r.created_at, r.updated_at
           from records r
           join incident_records ir on ir.record_id = r.id
          where ir.incident_id = $1
          order by r.occurred_at desc nulls last, r.created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(IncidentDetail { incident, records }))
}
