//! `/api/v1/insurance-plans` — insurance cards / policies (reference data, no
//! subject_id: a family shares one card). Read + idempotent-upsert write; dedup
//! on the standard `(source_id, external_id)` provenance key. Covered people are
//! linked via `/api/v1/subject-insurance`.

use axum::extract::State;
use axum::response::Json;
use serde::Deserialize;
use serde_json::Value;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{
    ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, clamp_limit, clamp_offset, provenance_conflict,
    validate_in, write_err,
};
use crate::models::{INSURANCE_PLAN_KINDS, INSURANCE_PLAN_TYPES, InsurancePlan, empty_to_none};

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Vec<InsurancePlan>>> {
    let rows = sqlx::query_as::<_, InsurancePlan>(
        "select * from insurance_plans order by payer_name, plan_name limit $1 offset $2",
    )
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
) -> ApiResult<Json<InsurancePlan>> {
    let row = sqlx::query_as::<_, InsurancePlan>("select * from insurance_plans where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub payer_name: String,
    #[serde(default)]
    pub plan_name: Option<String>,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub member_id: Option<String>,
    #[serde(default)]
    pub group_number: Option<String>,
    #[serde(default)]
    pub subscriber_name: Option<String>,
    #[serde(default)]
    pub plan_kind: Option<String>,
    #[serde(default)]
    pub rx_bin: Option<String>,
    #[serde(default)]
    pub rx_pcn: Option<String>,
    #[serde(default)]
    pub rx_group: Option<String>,
    #[serde(default)]
    pub payer_phone: Option<String>,
    #[serde(default)]
    pub effective_date: Option<Date>,
    #[serde(default)]
    pub expiration_date: Option<Date>,
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
) -> ApiResult<Json<InsurancePlan>> {
    let payer_name = c.payer_name.trim().to_string();
    if payer_name.is_empty() {
        return Err(ApiError::bad_request("payer_name required"));
    }
    if let Some(t) = c.plan_type.as_deref().and_then(|s| (!s.is_empty()).then_some(s)) {
        validate_in("plan_type", t, INSURANCE_PLAN_TYPES)?;
    }
    let plan_kind = c
        .plan_kind
        .and_then(empty_to_none)
        .unwrap_or_else(|| "medical".into());
    validate_in("plan_kind", &plan_kind, INSURANCE_PLAN_KINDS)?;
    let external_id = c.external_id.and_then(empty_to_none);
    let has_keys = c.source_id.is_some() && external_id.is_some();

    let set = "payer_name=excluded.payer_name, plan_name=excluded.plan_name, \
        plan_type=excluded.plan_type, member_id=excluded.member_id, \
        group_number=excluded.group_number, subscriber_name=excluded.subscriber_name, \
        plan_kind=excluded.plan_kind, rx_bin=excluded.rx_bin, rx_pcn=excluded.rx_pcn, \
        rx_group=excluded.rx_group, payer_phone=excluded.payer_phone, \
        effective_date=excluded.effective_date, expiration_date=excluded.expiration_date, \
        notes=excluded.notes, external_url=excluded.external_url, \
        source_synced_at=excluded.source_synced_at, source_payload=excluded.source_payload";
    let conflict = provenance_conflict(has_keys, set);
    let sql = format!(
        "insert into insurance_plans (id, payer_name, plan_name, plan_type, member_id, \
            group_number, subscriber_name, plan_kind, rx_bin, rx_pcn, rx_group, payer_phone, \
            effective_date, expiration_date, notes, source_id, external_id, external_url, \
            source_synced_at, source_payload) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20){conflict} \
         returning *"
    );
    let row = sqlx::query_as::<_, InsurancePlan>(&sql)
        .bind(Uuid::now_v7())
        .bind(&payer_name)
        .bind(c.plan_name.and_then(empty_to_none))
        .bind(c.plan_type.and_then(empty_to_none))
        .bind(c.member_id.and_then(empty_to_none))
        .bind(c.group_number.and_then(empty_to_none))
        .bind(c.subscriber_name.and_then(empty_to_none))
        .bind(&plan_kind)
        .bind(c.rx_bin.and_then(empty_to_none))
        .bind(c.rx_pcn.and_then(empty_to_none))
        .bind(c.rx_group.and_then(empty_to_none))
        .bind(c.payer_phone.and_then(empty_to_none))
        .bind(c.effective_date)
        .bind(c.expiration_date)
        .bind(c.notes.unwrap_or_default())
        .bind(c.source_id)
        .bind(external_id)
        .bind(c.external_url.and_then(empty_to_none))
        .bind(c.source_synced_at)
        .bind(c.source_payload)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?;
    Ok(Json(row))
}
