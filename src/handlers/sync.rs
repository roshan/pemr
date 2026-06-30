use axum::Form;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppResult;
use crate::handlers::{AppState, load_subjects};
use crate::models::empty_to_none;
use crate::sync;
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::views::sync as views;

pub async fn page(
    State(state): State<AppState>,
    viewer: ViewerContext,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let jobs = sync::all_jobs(&state.pool).await?;
    let nav = Nav {
        title: "Sync",
        current_path: "/settings/sync",
        subjects: &subjects,
        current_subject: viewer.default_subject_id,
        viewer: &viewer,
    };
    Ok(views::page(&nav, &jobs, &subjects, None))
}

#[derive(Debug, Deserialize)]
pub struct VaccineImportForm {
    pub urls: String,
    #[serde(default)]
    pub subject_id: String,
}

pub async fn import_vaccines(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Form(form): Form<VaccineImportForm>,
) -> AppResult<Markup> {
    let content = form.urls.trim().to_string();
    let subject_override = empty_to_none(form.subject_id)
        .and_then(|s| s.parse::<Uuid>().ok());

    let inputs: Vec<String> = if content.starts_with('<') {
        vec![content]
    } else {
        content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    };

    let result =
        sync::vaccine::import_from_urls(&state.pool, inputs, subject_override).await;

    let (status, message) = match &result {
        Ok(msg) => ("ok", msg.clone()),
        Err(msg) => ("error", msg.clone()),
    };
    sync::record_import(&state.pool, "vaccine_records", status, &message).await;

    let subjects = load_subjects(&state.pool).await?;
    let jobs = sync::all_jobs(&state.pool).await?;
    let nav = Nav {
        title: "Sync",
        current_path: "/settings/sync",
        subjects: &subjects,
        current_subject: viewer.default_subject_id,
        viewer: &viewer,
    };
    Ok(views::page(&nav, &jobs, &subjects, Some((status, &message))))
}

pub async fn run_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<Response> {
    let _ = state.sync_tx.try_send(name);
    Ok(Redirect::to("/settings/sync").into_response())
}
