use axum::extract::{Path, State};
use axum::response::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiResult, validate_in, write_err};
use crate::models::{SOURCE_KINDS, Source};

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
) -> ApiResult<Json<Vec<Source>>> {
    let rows = sqlx::query_as::<_, Source>("select * from sources order by name")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows))
}

pub async fn detail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Source>> {
    let row = sqlx::query_as::<_, Source>("select * from sources where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct Create {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<Source>> {
    let name = c.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::bad_request("name required"));
    }
    let kind = c.kind.trim().to_string();
    validate_in("kind", &kind, SOURCE_KINDS)?;
    let notes = c.notes.unwrap_or_default();
    // Sources have no unique index; dedup on case-insensitive name (matching the
    // DICOM importer's lookup) so re-uploading a clinic updates its contact info
    // rather than creating a second row.
    let existing: Option<Uuid> =
        sqlx::query_scalar("select id from sources where lower(name) = lower($1)")
            .bind(&name)
            .fetch_optional(&state.pool)
            .await?;
    let row = if let Some(id) = existing {
        sqlx::query_as::<_, Source>(
            "update sources set name=$2, kind=$3, base_url=$4, notes=$5, phone=$6, address=$7,
                updated_at=now() where id=$1 returning *",
        )
        .bind(id)
        .bind(&name)
        .bind(&kind)
        .bind(c.base_url)
        .bind(&notes)
        .bind(c.phone)
        .bind(c.address)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?
    } else {
        sqlx::query_as::<_, Source>(
            "insert into sources (id, name, kind, base_url, notes, phone, address)
             values ($1,$2,$3,$4,$5,$6,$7) returning *",
        )
        .bind(Uuid::now_v7())
        .bind(&name)
        .bind(&kind)
        .bind(c.base_url)
        .bind(&notes)
        .bind(c.phone)
        .bind(c.address)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?
    };
    Ok(Json(row))
}
