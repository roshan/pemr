use axum::Form;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;

use crate::error::AppResult;
use crate::handlers::{AppState, load_subjects};
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
    Ok(views::page(&nav, &jobs, None))
}

#[derive(Debug, Deserialize)]
pub struct VaccineImportForm {
    pub urls: String,
}

pub async fn import_vaccines(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Form(form): Form<VaccineImportForm>,
) -> AppResult<Markup> {
    let urls: Vec<String> = form
        .urls
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();

    let result = sync::vaccine::import_from_urls(&state.pool, urls).await;

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
    Ok(views::page(&nav, &jobs, Some((status, &message))))
}

/// Trigger a scheduled task on-demand. Currently no tasks are scheduled
/// (vaccine import is manual via the form), but the route stays live for
/// future periodic tasks.
pub async fn run_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<Response> {
    let _ = state.sync_tx.try_send(name);
    Ok(Redirect::to("/settings/sync").into_response())
}
