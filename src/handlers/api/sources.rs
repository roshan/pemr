use axum::extract::{Path, State};
use axum::response::Json;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiResult};
use crate::models::Source;

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
