//! `/api/v1/import/*` (PEMR-26) — bulk bundle import. Bearer-auth like the rest
//! of the API. Requires `?subject=<uuid>`; attributes rows to a source named by
//! `?source=<name>` (created if new). Idempotent (upsert on resource id).

use axum::extract::State;
use axum::response::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiQuery, ApiResult, write_err};
use crate::importer::{self, Counts};

#[derive(Debug, Deserialize)]
pub struct ImportQuery {
    pub subject: String,
    #[serde(default)]
    pub source: Option<String>,
}

async fn ensure_source(pool: &sqlx::PgPool, name: &str) -> ApiResult<Uuid> {
    if let Some(id) =
        sqlx::query_scalar::<_, Uuid>("select id from sources where lower(name) = lower($1)")
            .bind(name)
            .fetch_optional(pool)
            .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    sqlx::query("insert into sources (id, name, kind, notes) values ($1,$2,'other',$3)")
        .bind(id)
        .bind(name)
        .bind("Auto-created at bundle import.")
        .execute(pool)
        .await
        .map_err(write_err)?;
    Ok(id)
}

fn subject_uuid(q: &ImportQuery) -> ApiResult<Uuid> {
    Uuid::parse_str(q.subject.trim()).map_err(|_| ApiError::bad_request("subject must be a uuid"))
}

/// `POST /api/v1/import/fhir?subject=<uuid>&source=<name>` — FHIR R4 Bundle JSON.
pub async fn fhir(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<ImportQuery>,
    ApiJson(bundle): ApiJson<Value>,
) -> ApiResult<Json<Counts>> {
    let subject = subject_uuid(&q)?;
    let source_name = q.source.clone().unwrap_or_else(|| "FHIR import".into());
    let source_id = ensure_source(&state.pool, &source_name).await?;
    let counts = importer::import_fhir(&state.pool, subject, source_id, &bundle)
        .await
        .map_err(write_err)?;
    Ok(Json(counts))
}

/// `POST /api/v1/import/ccda?subject=<uuid>&source=<name>` — C-CDA XML body.
/// Best-effort against the C-CDA R2.1 templates; validate against a real export.
pub async fn ccda(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<ImportQuery>,
    body: String,
) -> ApiResult<Json<Counts>> {
    let subject = subject_uuid(&q)?;
    let source_name = q.source.clone().unwrap_or_else(|| "C-CDA import".into());
    let source_id = ensure_source(&state.pool, &source_name).await?;
    let counts = importer::import_ccda(&state.pool, subject, source_id, &body)
        .await
        .map_err(write_err)?;
    Ok(Json(counts))
}
