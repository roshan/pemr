//! Per-subject appointments CRUD (PEMR-17). Form POST → redirect.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use time::{OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{APPOINTMENT_STATUSES, Appointment, Provider, Subject, empty_to_none};
use crate::viewer::ViewerContext;
use crate::views::appointment;
use crate::views::layout::Nav;

async fn load_providers(pool: &sqlx::PgPool) -> Result<Vec<Provider>, sqlx::Error> {
    sqlx::query_as::<_, Provider>("select * from providers order by full_name")
        .fetch_all(pool)
        .await
}

fn parse_dt(s: &str) -> AppResult<OffsetDateTime> {
    let s = s.trim();
    let f_sec = time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    let f_min = time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]");
    PrimitiveDateTime::parse(s, f_sec)
        .or_else(|_| PrimitiveDateTime::parse(s, f_min))
        .map(|pdt| pdt.assume_utc())
        .map_err(|e| AppError::BadRequest(format!("bad datetime: {e}")))
}

fn parse_dt_opt(s: &str) -> AppResult<Option<OffsetDateTime>> {
    if s.trim().is_empty() {
        Ok(None)
    } else {
        parse_dt(s).map(Some)
    }
}

fn parse_opt_uuid(s: String, field: &str) -> AppResult<Option<Uuid>> {
    match empty_to_none(s) {
        None => Ok(None),
        Some(v) => Uuid::parse_str(&v)
            .map(Some)
            .map_err(|_| AppError::BadRequest(format!("invalid {field}"))),
    }
}

pub async fn list(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let upcoming = sqlx::query_as::<_, Appointment>(
        "select * from appointments where subject_id = $1 and starts_at >= now()
          order by starts_at asc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let past = sqlx::query_as::<_, Appointment>(
        "select * from appointments where subject_id = $1 and starts_at < now()
          order by starts_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let providers = load_providers(&state.pool).await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(appointment::list_page(&nav, &s, &upcoming, &past, &providers))
}

#[derive(Debug, Deserialize)]
pub struct ApptForm {
    pub title: String,
    pub starts_at: String,
    #[serde(default)]
    pub ends_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub notes: String,
}

fn validated(form: &ApptForm) -> AppResult<(String, OffsetDateTime, Option<OffsetDateTime>, String, Option<Uuid>)> {
    let title = form.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::BadRequest("title required".into()));
    }
    let starts_at = parse_dt(&form.starts_at)?;
    let ends_at = parse_dt_opt(&form.ends_at)?;
    let status = if form.status.trim().is_empty() {
        "scheduled".to_string()
    } else {
        form.status.trim().to_string()
    };
    if !APPOINTMENT_STATUSES.contains(&status.as_str()) {
        return Err(AppError::BadRequest(format!("unknown status: {status}")));
    }
    let provider_id = parse_opt_uuid(form.provider_id.clone(), "provider")?;
    Ok((title, starts_at, ends_at, status, provider_id))
}

pub async fn create(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<ApptForm>,
) -> AppResult<Response> {
    let (title, starts_at, ends_at, status, provider_id) = validated(&form)?;
    sqlx::query(
        "insert into appointments
            (id, subject_id, provider_id, starts_at, ends_at, status, title, location, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(provider_id)
    .bind(starts_at)
    .bind(ends_at)
    .bind(&status)
    .bind(&title)
    .bind(empty_to_none(form.location.clone()))
    .bind(&form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/subjects/{id}/appointments")).into_response())
}

pub async fn edit_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let a = sqlx::query_as::<_, Appointment>("select * from appointments where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(a.subject_id)
        .fetch_one(&state.pool)
        .await?;
    let providers = load_providers(&state.pool).await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(a.subject_id),
        viewer: &viewer,
    };
    Ok(appointment::edit_form(&nav, &a, &s, &providers, None))
}

pub async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<ApptForm>,
) -> AppResult<Response> {
    let (title, starts_at, ends_at, status, provider_id) = validated(&form)?;
    let subject_id: Uuid = sqlx::query_scalar(
        "update appointments set title=$2, starts_at=$3, ends_at=$4, status=$5,
            provider_id=$6, location=$7, notes=$8, updated_at=now()
          where id=$1 returning subject_id",
    )
    .bind(id)
    .bind(&title)
    .bind(starts_at)
    .bind(ends_at)
    .bind(&status)
    .bind(provider_id)
    .bind(empty_to_none(form.location.clone()))
    .bind(&form.notes)
    .fetch_one(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/subjects/{subject_id}/appointments")).into_response())
}
