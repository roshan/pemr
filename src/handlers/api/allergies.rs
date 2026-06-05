//! `/api/v1/allergies` — read + idempotent-upsert write (see `api::mod`).

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
use crate::models::{ALLERGY_CATEGORIES, ALLERGY_SEVERITIES, ALLERGY_STATUSES, Allergy, parse_subject_filter};

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
) -> ApiResult<Json<Vec<Allergy>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, Allergy>(
        "select * from allergies
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
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Allergy>> {
    let row = sqlx::query_as::<_, Allergy>("select * from allergies where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub substance: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub code_system: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub reaction: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub onset_date: Option<Date>,
    #[serde(default)]
    pub noted_date: Option<Date>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub source_id: Option<Uuid>,
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
) -> ApiResult<Json<Allergy>> {
    let substance = c.substance.trim().to_string();
    if substance.is_empty() {
        return Err(ApiError::bad_request("substance required"));
    }
    let status = c.status.unwrap_or_else(|| "active".into());
    validate_in("status", &status, ALLERGY_STATUSES)?;
    if let Some(cat) = &c.category {
        validate_in("category", cat, ALLERGY_CATEGORIES)?;
    }
    if let Some(sev) = &c.severity {
        validate_in("severity", sev, ALLERGY_SEVERITIES)?;
    }
    let notes = c.notes.unwrap_or_default();
    let has_keys = c.source_id.is_some() && c.external_id.is_some();
    let set = "subject_id=excluded.subject_id, substance=excluded.substance, code=excluded.code, \
        code_system=excluded.code_system, category=excluded.category, reaction=excluded.reaction, \
        severity=excluded.severity, status=excluded.status, onset_date=excluded.onset_date, \
        noted_date=excluded.noted_date, notes=excluded.notes, source_id=excluded.source_id, \
        external_id=excluded.external_id, external_url=excluded.external_url, \
        source_synced_at=excluded.source_synced_at, source_payload=excluded.source_payload";
    let sql = format!(
        "insert into allergies (id, subject_id, substance, code, code_system, category, reaction, \
            severity, status, onset_date, noted_date, notes, source_id, external_id, external_url, \
            source_synced_at, source_payload) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17){} returning *",
        provenance_conflict(has_keys, set),
    );
    let row = sqlx::query_as::<_, Allergy>(&sql)
        .bind(Uuid::now_v7())
        .bind(c.subject_id)
        .bind(&substance)
        .bind(c.code)
        .bind(c.code_system)
        .bind(c.category)
        .bind(c.reaction)
        .bind(c.severity)
        .bind(&status)
        .bind(c.onset_date)
        .bind(c.noted_date)
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
