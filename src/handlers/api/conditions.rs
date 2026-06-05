//! `/api/v1/conditions` — read + idempotent-upsert write (see `api::mod`).

use axum::extract::{Path, Query, State};
use axum::response::Json;
use serde::Deserialize;
use serde_json::Value;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{
    ApiError, ApiJson, ApiResult, clamp_limit, clamp_offset, provenance_conflict, validate_in,
    write_err,
};
use crate::models::{CONDITION_STATUSES, Condition, DATE_PRECISIONS, parse_subject_filter};

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub subject: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<Condition>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, Condition>(
        "select * from conditions
          where ($1::uuid is null or subject_id = $1)
          order by onset_date desc nulls last, created_at desc limit $2 offset $3",
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
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Condition>> {
    let row = sqlx::query_as::<_, Condition>("select * from conditions where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub code_system: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub onset_date: Option<Date>,
    #[serde(default)]
    pub onset_precision: Option<String>,
    #[serde(default)]
    pub resolved_date: Option<Date>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub source_id: Option<Uuid>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub external_url: Option<String>,
    #[serde(default)]
    pub source_synced_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub source_payload: Option<Value>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<Condition>> {
    let name = c.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::bad_request("name required"));
    }
    let status = c.status.unwrap_or_else(|| "active".into());
    validate_in("status", &status, CONDITION_STATUSES)?;
    let onset_precision = c.onset_precision.unwrap_or_else(|| "day".into());
    validate_in("onset_precision", &onset_precision, DATE_PRECISIONS)?;
    let notes = c.notes.unwrap_or_default();
    let has_keys = c.source_id.is_some() && c.external_id.is_some();
    let set = "subject_id=excluded.subject_id, name=excluded.name, code=excluded.code, \
        code_system=excluded.code_system, status=excluded.status, onset_date=excluded.onset_date, \
        onset_precision=excluded.onset_precision, resolved_date=excluded.resolved_date, \
        severity=excluded.severity, notes=excluded.notes, source_id=excluded.source_id, \
        external_id=excluded.external_id, external_url=excluded.external_url, \
        source_synced_at=excluded.source_synced_at, source_payload=excluded.source_payload";
    let sql = format!(
        "insert into conditions (id, subject_id, name, code, code_system, status, onset_date, \
            onset_precision, resolved_date, severity, notes, source_id, external_id, external_url, \
            source_synced_at, source_payload) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16){} returning *",
        provenance_conflict(has_keys, set),
    );
    let row = sqlx::query_as::<_, Condition>(&sql)
        .bind(Uuid::now_v7())
        .bind(c.subject_id)
        .bind(&name)
        .bind(c.code)
        .bind(c.code_system)
        .bind(&status)
        .bind(c.onset_date)
        .bind(&onset_precision)
        .bind(c.resolved_date)
        .bind(c.severity)
        .bind(&notes)
        .bind(c.source_id)
        .bind(c.external_id)
        .bind(c.external_url)
        .bind(c.source_synced_at)
        .bind(c.source_payload)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?;
    Ok(Json(row))
}
