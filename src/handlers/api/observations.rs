//! `/api/v1/observations` — vitals + discrete labs; read + idempotent-upsert.
//!
//! NOTE: `value_num`/`ref_low`/`ref_high` are Postgres `numeric` but the
//! `Observation` struct types them `f64` (sqlx has no decimal feature here), so
//! every read/returning projects them with `::float8`. On insert, binding f64
//! into the numeric columns relies on the implicit assignment cast.

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
use crate::models::{
    DATE_PRECISIONS, OBSERVATION_ABNORMAL_FLAGS, OBSERVATION_CATEGORIES, Observation,
    parse_subject_filter,
};

/// Column projection with the numeric columns cast to float8 (see module note).
const COLS: &str = "id, subject_id, category, code, code_system, display, \
    value_num::float8 as value_num, value_text, unit, \
    ref_low::float8 as ref_low, ref_high::float8 as ref_high, abnormal_flag, \
    effective_on, effective_precision, effective_at, panel_id, record_id, \
    appointment_id, incident_id, notes, source_id, external_id, external_url, \
    source_synced_at, created_at, updated_at";

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub subject: Option<String>,
    pub code: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<Observation>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let code = q
        .code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let sql = format!(
        "select {COLS} from observations
          where ($1::uuid is null or subject_id = $1)
            and ($2::text is null or code = $2)
          order by effective_on desc, created_at desc limit $3 offset $4"
    );
    let rows = sqlx::query_as::<_, Observation>(&sql)
        .bind(subject)
        .bind(code)
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
) -> ApiResult<Json<Observation>> {
    let sql = format!("select {COLS} from observations where id = $1");
    let row = sqlx::query_as::<_, Observation>(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub display: String,
    pub effective_on: Date,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub code_system: Option<String>,
    #[serde(default)]
    pub value_num: Option<f64>,
    #[serde(default)]
    pub value_text: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub ref_low: Option<f64>,
    #[serde(default)]
    pub ref_high: Option<f64>,
    #[serde(default)]
    pub abnormal_flag: Option<String>,
    #[serde(default)]
    pub effective_precision: Option<String>,
    #[serde(default)]
    #[serde(with = "time::serde::rfc3339::option")]
    pub effective_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub panel_id: Option<Uuid>,
    #[serde(default)]
    pub record_id: Option<Uuid>,
    #[serde(default)]
    pub appointment_id: Option<Uuid>,
    #[serde(default)]
    pub incident_id: Option<Uuid>,
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
) -> ApiResult<Json<Observation>> {
    let display = c.display.trim().to_string();
    if display.is_empty() {
        return Err(ApiError::bad_request("display required"));
    }
    let category = c.category.unwrap_or_else(|| "vital".into());
    validate_in("category", &category, OBSERVATION_CATEGORIES)?;
    let effective_precision = c.effective_precision.unwrap_or_else(|| "day".into());
    validate_in("effective_precision", &effective_precision, DATE_PRECISIONS)?;
    if let Some(flag) = &c.abnormal_flag {
        validate_in("abnormal_flag", flag, OBSERVATION_ABNORMAL_FLAGS)?;
    }
    let notes = c.notes.unwrap_or_default();
    let has_keys = c.source_id.is_some() && c.external_id.is_some();
    let set = "subject_id=excluded.subject_id, category=excluded.category, code=excluded.code, \
        code_system=excluded.code_system, display=excluded.display, value_num=excluded.value_num, \
        value_text=excluded.value_text, unit=excluded.unit, ref_low=excluded.ref_low, \
        ref_high=excluded.ref_high, abnormal_flag=excluded.abnormal_flag, \
        effective_on=excluded.effective_on, effective_precision=excluded.effective_precision, \
        effective_at=excluded.effective_at, panel_id=excluded.panel_id, \
        record_id=excluded.record_id, appointment_id=excluded.appointment_id, \
        incident_id=excluded.incident_id, notes=excluded.notes, source_id=excluded.source_id, \
        external_id=excluded.external_id, external_url=excluded.external_url, \
        source_synced_at=excluded.source_synced_at, source_payload=excluded.source_payload";
    let sql = format!(
        "insert into observations (id, subject_id, category, code, code_system, display, value_num, \
            value_text, unit, ref_low, ref_high, abnormal_flag, effective_on, effective_precision, \
            effective_at, panel_id, record_id, appointment_id, incident_id, notes, source_id, \
            external_id, external_url, source_synced_at, source_payload) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25){} \
         returning {COLS}",
        provenance_conflict(has_keys, set),
    );
    let row = sqlx::query_as::<_, Observation>(&sql)
        .bind(Uuid::now_v7())
        .bind(c.subject_id)
        .bind(&category)
        .bind(c.code)
        .bind(c.code_system)
        .bind(&display)
        .bind(c.value_num)
        .bind(c.value_text)
        .bind(c.unit)
        .bind(c.ref_low)
        .bind(c.ref_high)
        .bind(c.abnormal_flag)
        .bind(c.effective_on)
        .bind(&effective_precision)
        .bind(c.effective_at)
        .bind(c.panel_id)
        .bind(c.record_id)
        .bind(c.appointment_id)
        .bind(c.incident_id)
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
