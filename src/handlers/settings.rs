use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::api_auth;
use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{ApiKey, empty_to_none};
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::views::settings;

pub async fn api_keys(
    State(state): State<AppState>,
    viewer: ViewerContext,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let keys = sqlx::query_as::<_, ApiKey>(
        "select * from api_keys order by revoked_at nulls first, created_at desc",
    )
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: "API keys",
        current_path: "/settings/api-keys",
        subjects: &subjects,
        current_subject: viewer.default_subject_id,
        viewer: &viewer,
    };
    Ok(settings::api_keys_page(&nav, &keys, &subjects, None))
}

#[derive(Debug, Deserialize)]
pub struct CreateForm {
    pub name: String,
    #[serde(default)]
    pub owner_subject_id: String,
}

pub async fn create_api_key(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Form(form): Form<CreateForm>,
) -> AppResult<Markup> {
    let name = form.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }
    let owner_subject_id = match empty_to_none(form.owner_subject_id) {
        None => None,
        Some(s) => Some(
            Uuid::parse_str(&s).map_err(|e| AppError::BadRequest(e.to_string()))?,
        ),
    };

    let token = api_auth::generate_token();
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into api_keys (id, name, token_hash, token_prefix, owner_subject_id)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(&name)
    .bind(&token.hash_hex)
    .bind(&token.prefix)
    .bind(owner_subject_id)
    .execute(&state.pool)
    .await?;

    let subjects = load_subjects(&state.pool).await?;
    let keys = sqlx::query_as::<_, ApiKey>(
        "select * from api_keys order by revoked_at nulls first, created_at desc",
    )
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: "API keys",
        current_path: "/settings/api-keys",
        subjects: &subjects,
        current_subject: viewer.default_subject_id,
        viewer: &viewer,
    };
    Ok(settings::api_keys_page(
        &nav,
        &keys,
        &subjects,
        Some(&token.raw),
    ))
}

pub async fn revoke_api_key(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    sqlx::query("update api_keys set revoked_at = now() where id = $1 and revoked_at is null")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Redirect::to("/settings/api-keys").into_response())
}
