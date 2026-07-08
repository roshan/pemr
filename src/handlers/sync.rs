use axum::Form;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppResult;
use crate::handlers::import as import_handler;
use crate::handlers::{AppState, load_subjects};
use crate::models::{Subject, empty_to_none};
use crate::sync;
use crate::viewer::ViewerContext;
use crate::views::import as import_view;
use crate::views::layout::Nav;
use crate::views::sync as views;

fn import_nav<'a>(subjects: &'a [Subject], viewer: &'a ViewerContext) -> Nav<'a> {
    Nav {
        title: "Import",
        current_path: "/settings/import",
        subjects,
        current_subject: viewer.default_subject_id,
        viewer,
    }
}

/// Old `/settings/sync` — folded into the unified Import page. Kept as a redirect
/// so existing links/bookmarks still land somewhere sensible.
pub async fn page() -> Redirect {
    Redirect::to("/settings/import")
}

#[derive(Debug, Deserialize)]
pub struct ProviderQuery {
    #[serde(default)]
    pub provider: String,
}

/// Returns the import form for the picked sync source, swapped into `#sync-form`
/// by the picker. Falls back to the default provider for an unknown key.
pub async fn provider_form(
    State(state): State<AppState>,
    Query(query): Query<ProviderQuery>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let key = if sync::provider(&query.provider).is_some() {
        query.provider.as_str()
    } else {
        sync::DEFAULT_PROVIDER
    };
    Ok(views::provider_form(key, &subjects, None))
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

    // Subject is required — we never auto-detect it from the CDPH page.
    let subject_id = match empty_to_none(form.subject_id).and_then(|s| s.parse::<Uuid>().ok()) {
        Some(id) => id,
        None => {
            let subjects = load_subjects(&state.pool).await?;
            let jobs = sync::all_jobs(&state.pool).await?;
            let nav = import_nav(&subjects, &viewer);
            return Ok(import_view::page(
                &nav,
                &subjects,
                &jobs,
                "dvr",
                import_handler::DEFAULT_SOURCE,
                None,
                Some(("error", "Pick a subject before importing.")),
            ));
        }
    };

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
        sync::vaccine::import_from_urls(&state.pool, inputs, subject_id).await;

    let (status, message) = match &result {
        Ok(msg) => ("ok", msg.clone()),
        Err(msg) => ("error", msg.clone()),
    };
    sync::record_import(&state.pool, "vaccine_records", status, &message).await;

    let subjects = load_subjects(&state.pool).await?;
    let jobs = sync::all_jobs(&state.pool).await?;
    let nav = import_nav(&subjects, &viewer);
    Ok(import_view::page(
        &nav,
        &subjects,
        &jobs,
        "dvr",
        import_handler::DEFAULT_SOURCE,
        None,
        Some((status, &message)),
    ))
}

pub async fn run_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<Response> {
    let _ = state.sync_tx.try_send(name);
    Ok(Redirect::to("/settings/sync").into_response())
}
