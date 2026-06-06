//! Providers directory CRUD (PEMR-17). Plain form POST → redirect, matching
//! the sources/subjects handlers.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{Provider, Source, empty_to_none};
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::views::provider;

async fn load_sources(pool: &sqlx::PgPool) -> Result<Vec<Source>, sqlx::Error> {
    sqlx::query_as::<_, Source>("select * from sources order by name")
        .fetch_all(pool)
        .await
}

fn parse_opt_uuid(s: String, field: &str) -> AppResult<Option<Uuid>> {
    match empty_to_none(s) {
        None => Ok(None),
        Some(v) => Uuid::parse_str(&v)
            .map(Some)
            .map_err(|_| AppError::BadRequest(format!("invalid {field}"))),
    }
}

pub async fn list(State(state): State<AppState>, viewer: ViewerContext) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let providers = sqlx::query_as::<_, Provider>("select * from providers order by full_name")
        .fetch_all(&state.pool)
        .await?;
    let sources = load_sources(&state.pool).await?;
    let nav = Nav {
        title: "Providers",
        current_path: "/providers",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(provider::list_page(&nav, &providers, &sources))
}

#[derive(Debug, Deserialize)]
pub struct CreateForm {
    pub full_name: String,
    #[serde(default)]
    pub specialty: String,
    #[serde(default)]
    pub npi: String,
    #[serde(default)]
    pub facility_id: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub notes: String,
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<CreateForm>,
) -> AppResult<Response> {
    let full_name = form.full_name.trim().to_string();
    if full_name.is_empty() {
        return Err(AppError::BadRequest("full_name required".into()));
    }
    let facility_id = parse_opt_uuid(form.facility_id, "facility")?;
    sqlx::query(
        "insert into providers (id, full_name, specialty, npi, facility_id, phone, email, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::now_v7())
    .bind(&full_name)
    .bind(empty_to_none(form.specialty))
    .bind(empty_to_none(form.npi))
    .bind(facility_id)
    .bind(empty_to_none(form.phone))
    .bind(empty_to_none(form.email))
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to("/providers").into_response())
}

pub async fn edit_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let p = sqlx::query_as::<_, Provider>("select * from providers where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let sources = load_sources(&state.pool).await?;
    let nav = Nav {
        title: "Edit provider",
        current_path: "/providers",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(provider::edit_form(&nav, &p, &sources, None))
}

pub async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<CreateForm>,
) -> AppResult<Response> {
    let full_name = form.full_name.trim().to_string();
    if full_name.is_empty() {
        return Err(AppError::BadRequest("full_name required".into()));
    }
    let facility_id = parse_opt_uuid(form.facility_id, "facility")?;
    sqlx::query(
        "update providers set full_name=$2, specialty=$3, npi=$4, facility_id=$5,
            phone=$6, email=$7, notes=$8, updated_at=now() where id=$1",
    )
    .bind(id)
    .bind(&full_name)
    .bind(empty_to_none(form.specialty))
    .bind(empty_to_none(form.npi))
    .bind(facility_id)
    .bind(empty_to_none(form.phone))
    .bind(empty_to_none(form.email))
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to("/providers").into_response())
}
