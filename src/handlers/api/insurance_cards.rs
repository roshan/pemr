//! `/api/v1/insurance-plans/{id}/cards` — the scanned card images for a policy.
//!
//! This is the endpoint an agent (MoltBoss) calls when it needs to *show*
//! someone the card: `GET /api/v1/insurance-plans/{id}/card` streams the
//! current front as image bytes, ready to hand to a viewer, with
//! `?side=back` for the reverse. It is deliberately a bytes endpoint rather
//! than a JSON blob with a path — the caller wants an image, not a lookup.
//!
//! "Current" is resolved in the database (`superseded_at is null`, one row per
//! (plan, side) by unique index), so a caller never has to sort versions or
//! guess which upload is live. Uploads happen through the UI
//! (`POST /insurance/{id}/cards`); this module is read-only.

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::files;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiPath, ApiQuery, ApiResult, validate_in};
use crate::models::{INSURANCE_CARD_SIDES, InsuranceCard};

#[derive(Debug, Deserialize, Default)]
pub struct CardQuery {
    /// `front` (default) or `back`.
    pub side: Option<String>,
    /// Include superseded cards in the listing. Off by default: the common
    /// question is "what's in my wallet now", not "what has it ever been".
    #[serde(default)]
    pub include_superseded: bool,
}

/// All card images for a plan. Current first, then superseded newest-first.
pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(plan_id): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<CardQuery>,
) -> ApiResult<Json<Vec<InsuranceCard>>> {
    let sql = if q.include_superseded {
        "select * from insurance_cards where plan_id = $1
          order by (superseded_at is null) desc, side, created_at desc"
    } else {
        "select * from insurance_cards where plan_id = $1 and superseded_at is null
          order by side"
    };
    let rows = sqlx::query_as::<_, InsuranceCard>(sql)
        .bind(plan_id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows))
}

/// Stream the **current** card image for a plan. `?side=front|back`.
///
/// 404 when the plan has no current card for that side — an agent can treat
/// that as "no card on file" without parsing a body.
pub async fn current(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(plan_id): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<CardQuery>,
) -> ApiResult<Response> {
    let side = q.side.unwrap_or_else(|| "front".to_string());
    let side = side.trim().to_lowercase();
    validate_in("side", &side, INSURANCE_CARD_SIDES)?;

    let row: Option<(String, Option<String>, Option<i64>, String)> = sqlx::query_as(
        "select c.file_path, c.content_type, c.byte_size, p.payer_name
           from insurance_cards c join insurance_plans p on p.id = c.plan_id
          where c.plan_id = $1 and c.side = $2 and c.superseded_at is null",
    )
    .bind(plan_id)
    .bind(&side)
    .fetch_optional(&state.pool)
    .await?;

    let (file_path, content_type, byte_size, payer) = row.ok_or_else(ApiError::not_found)?;
    let filename = format!("{payer}-{side}");
    serve(&state, &file_path, content_type, byte_size, &filename).await
}

async fn serve(
    state: &AppState,
    rel: &str,
    content_type: Option<String>,
    byte_size: Option<i64>,
    filename: &str,
) -> ApiResult<Response> {
    let abs = files::resolve(&state.files_dir, rel).ok_or_else(ApiError::not_found)?;
    let f = File::open(&abs).await.map_err(|_| ApiError::not_found())?;
    let body = Body::from_stream(ReaderStream::new(f));

    let mut headers = HeaderMap::new();
    let ct = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    if let Ok(v) = HeaderValue::from_str(&ct) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Some(n) = byte_size {
        if let Ok(v) = HeaderValue::from_str(&n.to_string()) {
            headers.insert(header::CONTENT_LENGTH, v);
        }
    }
    let safe: String = filename
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{safe}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok((StatusCode::OK, headers, body).into_response())
}
