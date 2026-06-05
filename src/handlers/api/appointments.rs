//! `/api/v1/appointments` — read + idempotent-upsert write (see `api::mod`).

use axum::extract::{Path, Query, State};
use axum::response::Json;
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{
    ApiError, ApiJson, ApiResult, clamp_limit, clamp_offset, provenance_conflict, validate_in,
    write_err,
};
use crate::models::{APPOINTMENT_STATUSES, Appointment, parse_subject_filter};

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
) -> ApiResult<Json<Vec<Appointment>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, Appointment>(
        "select * from appointments
          where ($1::uuid is null or subject_id = $1)
          order by starts_at desc limit $2 offset $3",
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
) -> ApiResult<Json<Appointment>> {
    let row = sqlx::query_as::<_, Appointment>("select * from appointments where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub title: String,
    #[serde(with = "time::serde::rfc3339")]
    pub starts_at: OffsetDateTime,
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    #[serde(default)]
    pub source_id: Option<Uuid>,
    #[serde(default)]
    pub incident_id: Option<Uuid>,
    #[serde(default)]
    #[serde(with = "time::serde::rfc3339::option")]
    pub ends_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub all_day: Option<bool>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub external_url: Option<String>,
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
) -> ApiResult<Json<Appointment>> {
    let title = c.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::bad_request("title required"));
    }
    let status = c.status.unwrap_or_else(|| "scheduled".into());
    validate_in("status", &status, APPOINTMENT_STATUSES)?;
    let all_day = c.all_day.unwrap_or(false);
    let notes = c.notes.unwrap_or_default();
    let has_keys = c.source_id.is_some() && c.external_id.is_some();
    let set = "subject_id=excluded.subject_id, provider_id=excluded.provider_id, \
        source_id=excluded.source_id, incident_id=excluded.incident_id, \
        starts_at=excluded.starts_at, ends_at=excluded.ends_at, all_day=excluded.all_day, \
        status=excluded.status, title=excluded.title, location=excluded.location, \
        notes=excluded.notes, external_id=excluded.external_id, external_url=excluded.external_url, \
        source_synced_at=excluded.source_synced_at, source_payload=excluded.source_payload";
    let sql = format!(
        "insert into appointments (id, subject_id, provider_id, source_id, incident_id, starts_at, \
            ends_at, all_day, status, title, location, notes, external_id, external_url, \
            source_synced_at, source_payload) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16){} returning *",
        provenance_conflict(has_keys, set),
    );
    let row = sqlx::query_as::<_, Appointment>(&sql)
        .bind(Uuid::now_v7())
        .bind(c.subject_id)
        .bind(c.provider_id)
        .bind(c.source_id)
        .bind(c.incident_id)
        .bind(c.starts_at)
        .bind(c.ends_at)
        .bind(all_day)
        .bind(&status)
        .bind(&title)
        .bind(c.location)
        .bind(&notes)
        .bind(c.external_id)
        .bind(c.external_url)
        .bind(c.source_synced_at)
        .bind(c.source_payload)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?;
    Ok(Json(row))
}
