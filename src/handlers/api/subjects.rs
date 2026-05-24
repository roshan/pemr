use axum::extract::{Path, State};
use axum::response::Json;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiResult};
use crate::models::Subject;

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
) -> ApiResult<Json<Vec<Subject>>> {
    let rows = sqlx::query_as::<_, Subject>(
        "select * from subjects order by family_name, given_name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn detail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Subject>> {
    let row = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}
