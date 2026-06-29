use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

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
    Ok(views::page(&nav, &jobs))
}

pub async fn run_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<Response> {
    // Fire-and-forget: non-blocking send. The background loop picks it up;
    // the user can refresh to see the result.
    let _ = state.sync_tx.try_send(name);
    Ok(Redirect::to("/settings/sync").into_response())
}
