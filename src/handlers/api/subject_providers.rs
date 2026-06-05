//! `/api/v1/subject-providers` — care-team membership (subject ↔ provider).
//! Read (list) + idempotent-upsert keyed on the (subject_id, provider_id) PK.
//! No detail-by-id route: the row has a composite PK, not a surrogate id.

use axum::extract::{Query, State};
use axum::response::Json;
use serde::Deserialize;
use time::Date;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{
    ApiError, ApiJson, ApiResult, clamp_limit, clamp_offset, validate_in, write_err,
};
use crate::models::{SUBJECT_PROVIDER_ROLES, SubjectProvider, parse_subject_filter};

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
) -> ApiResult<Json<Vec<SubjectProvider>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, SubjectProvider>(
        "select * from subject_providers
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

#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub provider_id: Uuid,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub since: Option<Date>,
    #[serde(default)]
    pub notes: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<SubjectProvider>> {
    let role = c.role.unwrap_or_else(|| "care".into());
    validate_in("role", &role, SUBJECT_PROVIDER_ROLES)?;
    let active = c.active.unwrap_or(true);
    let row = sqlx::query_as::<_, SubjectProvider>(
        "insert into subject_providers (subject_id, provider_id, role, active, since, notes)
         values ($1,$2,$3,$4,$5,$6)
         on conflict (subject_id, provider_id) do update set
            role = excluded.role, active = excluded.active,
            since = excluded.since, notes = excluded.notes
         returning *",
    )
    .bind(c.subject_id)
    .bind(c.provider_id)
    .bind(&role)
    .bind(active)
    .bind(c.since)
    .bind(c.notes.unwrap_or_default())
    .fetch_one(&state.pool)
    .await
    .map_err(write_err)?;
    Ok(Json(row))
}
