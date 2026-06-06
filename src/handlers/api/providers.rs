//! `/api/v1/providers` — shared clinician directory (reference data, no
//! subject_id). Read + idempotent-upsert write. Dedup prefers `npi` (the global
//! key); falls back to `(source_id, external_id)` when npi is absent.

use axum::extract::{State};
use axum::response::Json;
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, clamp_limit, clamp_offset, write_err};
use crate::models::{Provider, empty_to_none};

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Vec<Provider>>> {
    let rows = sqlx::query_as::<_, Provider>(
        "select * from providers order by full_name limit $1 offset $2",
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
) -> ApiResult<Json<Provider>> {
    let row = sqlx::query_as::<_, Provider>("select * from providers where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub full_name: String,
    #[serde(default)]
    pub specialty: Option<String>,
    #[serde(default)]
    pub npi: Option<String>,
    #[serde(default)]
    pub facility_id: Option<Uuid>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
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
) -> ApiResult<Json<Provider>> {
    let full_name = c.full_name.trim().to_string();
    if full_name.is_empty() {
        return Err(ApiError::bad_request("full_name required"));
    }
    let npi = c.npi.and_then(empty_to_none);
    let external_id = c.external_id.and_then(empty_to_none);

    let set = "full_name=excluded.full_name, specialty=excluded.specialty, npi=excluded.npi, \
        facility_id=excluded.facility_id, phone=excluded.phone, email=excluded.email, \
        notes=excluded.notes, source_id=excluded.source_id, external_id=excluded.external_id, \
        external_url=excluded.external_url, source_synced_at=excluded.source_synced_at, \
        source_payload=excluded.source_payload, updated_at=now()";
    let conflict = if npi.is_some() {
        format!(" on conflict (npi) where npi is not null do update set {set}")
    } else if c.source_id.is_some() && external_id.is_some() {
        format!(
            " on conflict (source_id, external_id) \
             where source_id is not null and external_id is not null do update set {set}"
        )
    } else {
        String::new()
    };
    let sql = format!(
        "insert into providers (id, full_name, specialty, npi, facility_id, phone, email, notes, \
            source_id, external_id, external_url, source_synced_at, source_payload) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13){conflict} returning *"
    );
    let row = sqlx::query_as::<_, Provider>(&sql)
        .bind(Uuid::now_v7())
        .bind(&full_name)
        .bind(c.specialty)
        .bind(npi)
        .bind(c.facility_id)
        .bind(c.phone)
        .bind(c.email)
        .bind(c.notes.unwrap_or_default())
        .bind(c.source_id)
        .bind(external_id)
        .bind(c.external_url)
        .bind(c.source_synced_at)
        .bind(c.source_payload)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?;
    Ok(Json(row))
}
