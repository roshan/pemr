//! Care reminders CRUD (PEMR-19). Form POST → redirect.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{CARE_REMINDER_KINDS, CareReminder, Subject, parse_date};
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::views::reminders;

pub async fn page(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let rems = sqlx::query_as::<_, CareReminder>(
        "select * from care_reminders where subject_id = $1
          order by (status <> 'due'), due_on asc nulls last, created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(reminders::page(&nav, &s, &rems, crate::peds::today()))
}

#[derive(Debug, Deserialize)]
pub struct ReminderForm {
    pub title: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub due_on: String,
}

pub async fn add(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(f): Form<ReminderForm>,
) -> AppResult<Response> {
    let title = f.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::BadRequest("title required".into()));
    }
    let kind = {
        let k = f.kind.trim();
        let k = if k.is_empty() { "other" } else { k };
        if !CARE_REMINDER_KINDS.contains(&k) {
            return Err(AppError::BadRequest(format!("unknown kind: {k}")));
        }
        k.to_string()
    };
    let due_on = parse_date(&f.due_on).map_err(AppError::BadRequest)?;
    sqlx::query(
        "insert into care_reminders (id, subject_id, title, kind, due_on, status)
         values ($1,$2,$3,$4,$5,'due')",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(&title)
    .bind(&kind)
    .bind(due_on)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/subjects/{id}/reminders")).into_response())
}

pub async fn mark(
    State(state): State<AppState>,
    Path((id, rid, status)): Path<(Uuid, Uuid, String)>,
) -> AppResult<Response> {
    if status != "done" && status != "dismissed" {
        return Err(AppError::BadRequest("status must be done or dismissed".into()));
    }
    sqlx::query(
        "update care_reminders set status = $3, updated_at = now()
          where id = $1 and subject_id = $2",
    )
    .bind(rid)
    .bind(id)
    .bind(&status)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/subjects/{id}/reminders")).into_response())
}
