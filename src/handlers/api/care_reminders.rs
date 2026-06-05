//! `/api/v1/care-reminders` — "what's due". Read + create. No provenance key,
//! so POST always inserts (these are app-generated, not synced from a source).
//! `overdue` is derived (due_on < today), never stored.

use axum::extract::{Path, Query, State};
use axum::response::Json;
use serde::Deserialize;
use time::Date;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{
    ApiError, ApiJson, ApiResult, clamp_limit, clamp_offset, validate_in, write_err,
};
use crate::models::{CARE_REMINDER_KINDS, CARE_REMINDER_STATUSES, CareReminder, parse_subject_filter};

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
) -> ApiResult<Json<Vec<CareReminder>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, CareReminder>(
        "select * from care_reminders
          where ($1::uuid is null or subject_id = $1)
          order by due_on asc nulls last, created_at desc limit $2 offset $3",
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
) -> ApiResult<Json<CareReminder>> {
    let row = sqlx::query_as::<_, CareReminder>("select * from care_reminders where id = $1")
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
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub due_on: Option<Date>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub recommended_by: Option<Uuid>,
    #[serde(default)]
    pub satisfied_by_appointment_id: Option<Uuid>,
    #[serde(default)]
    pub notes: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<CareReminder>> {
    let title = c.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::bad_request("title required"));
    }
    let kind = c.kind.unwrap_or_else(|| "other".into());
    validate_in("kind", &kind, CARE_REMINDER_KINDS)?;
    let status = c.status.unwrap_or_else(|| "due".into());
    validate_in("status", &status, CARE_REMINDER_STATUSES)?;
    let row = sqlx::query_as::<_, CareReminder>(
        "insert into care_reminders
            (id, subject_id, title, kind, due_on, status, recommended_by,
             satisfied_by_appointment_id, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9) returning *",
    )
    .bind(Uuid::now_v7())
    .bind(c.subject_id)
    .bind(&title)
    .bind(&kind)
    .bind(c.due_on)
    .bind(&status)
    .bind(c.recommended_by)
    .bind(c.satisfied_by_appointment_id)
    .bind(c.notes.unwrap_or_default())
    .fetch_one(&state.pool)
    .await
    .map_err(write_err)?;
    Ok(Json(row))
}
