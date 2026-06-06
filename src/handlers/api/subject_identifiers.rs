//! `/api/v1/subject-identifiers` — cross-system identity (MRNs, member IDs).
//! Read + idempotent-upsert keyed on (source_id, id_type, value).

use axum::extract::{State};
use axum::response::Json;
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, clamp_limit, clamp_offset, validate_in, write_err};
use crate::models::{SUBJECT_IDENTIFIER_TYPES, SubjectIdentifier, parse_subject_filter};

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
) -> ApiResult<Json<Vec<SubjectIdentifier>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, SubjectIdentifier>(
        "select * from subject_identifiers
          where ($1::uuid is null or subject_id = $1)
          order by created_at desc limit $2 offset $3",
    )
    .bind(subject)
    .bind(clamp_limit(q.limit))
    .bind(clamp_offset(q.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn detail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<SubjectIdentifier>> {
    let row = sqlx::query_as::<_, SubjectIdentifier>(
        "select * from subject_identifiers where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub source_id: Uuid,
    pub value: String,
    #[serde(default)]
    pub id_type: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    #[serde(with = "time::serde::rfc3339::option")]
    pub source_synced_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub source_payload: Option<Value>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<SubjectIdentifier>> {
    let value = c.value.trim().to_string();
    if value.is_empty() {
        return Err(ApiError::bad_request("value required"));
    }
    let id_type = c.id_type.unwrap_or_else(|| "mrn".into());
    validate_in("id_type", &id_type, SUBJECT_IDENTIFIER_TYPES)?;
    let row = sqlx::query_as::<_, SubjectIdentifier>(
        "insert into subject_identifiers
            (id, subject_id, source_id, id_type, value, notes, source_synced_at, source_payload)
         values ($1,$2,$3,$4,$5,$6,$7,$8)
         on conflict (source_id, id_type, value) do update set
            subject_id = excluded.subject_id, notes = excluded.notes,
            source_synced_at = excluded.source_synced_at,
            source_payload = excluded.source_payload, updated_at = now()
         returning *",
    )
    .bind(Uuid::now_v7())
    .bind(c.subject_id)
    .bind(c.source_id)
    .bind(&id_type)
    .bind(&value)
    .bind(c.notes.unwrap_or_default())
    .bind(c.source_synced_at)
    .bind(c.source_payload)
    .fetch_one(&state.pool)
    .await
    .map_err(write_err)?;
    Ok(Json(row))
}
