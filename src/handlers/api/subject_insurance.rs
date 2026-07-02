//! `/api/v1/subject-insurance` — coverage link (subject ↔ insurance plan).
//! Read (list, `?subject=`) + idempotent-upsert keyed on the (subject_id,
//! plan_id) PK. No detail-by-id route: the row has a composite PK, not a
//! surrogate id (same as subject-providers).

use axum::extract::State;
use axum::response::Json;
use serde::Deserialize;
use time::Date;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{
    ApiError, ApiJson, ApiQuery, ApiResult, clamp_limit, clamp_offset, validate_in, write_err,
};
use crate::models::{INSURANCE_RELATIONSHIPS, SubjectInsurance, empty_to_none, parse_subject_filter};

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
) -> ApiResult<Json<Vec<SubjectInsurance>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, SubjectInsurance>(
        "select * from subject_insurance
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
    pub plan_id: Uuid,
    #[serde(default)]
    pub relationship: Option<String>,
    #[serde(default)]
    pub member_id: Option<String>,
    #[serde(default)]
    pub is_primary: Option<bool>,
    #[serde(default)]
    pub effective_date: Option<Date>,
    #[serde(default)]
    pub expiration_date: Option<Date>,
    #[serde(default)]
    pub notes: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<SubjectInsurance>> {
    let relationship = c.relationship.and_then(empty_to_none).unwrap_or_else(|| "self".into());
    validate_in("relationship", &relationship, INSURANCE_RELATIONSHIPS)?;
    let is_primary = c.is_primary.unwrap_or(true);
    let row = sqlx::query_as::<_, SubjectInsurance>(
        "insert into subject_insurance
            (subject_id, plan_id, relationship, member_id, is_primary, effective_date, expiration_date, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8)
         on conflict (subject_id, plan_id) do update set
            relationship = excluded.relationship, member_id = excluded.member_id,
            is_primary = excluded.is_primary, effective_date = excluded.effective_date,
            expiration_date = excluded.expiration_date, notes = excluded.notes
         returning *",
    )
    .bind(c.subject_id)
    .bind(c.plan_id)
    .bind(&relationship)
    .bind(c.member_id.and_then(empty_to_none))
    .bind(is_primary)
    .bind(c.effective_date)
    .bind(c.expiration_date)
    .bind(c.notes.unwrap_or_default())
    .fetch_one(&state.pool)
    .await
    .map_err(write_err)?;
    Ok(Json(row))
}
