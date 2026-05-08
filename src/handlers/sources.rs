use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{Incident, Record, SOURCE_KINDS, Source, empty_to_none};
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::views::source;

pub async fn list(
    State(state): State<AppState>,
    viewer: ViewerContext,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let sources = sqlx::query_as::<_, Source>("select * from sources order by name")
        .fetch_all(&state.pool)
        .await?;
    let nav = Nav {
        title: "Sources",
        current_path: "/sources",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(source::list_page(&nav, &sources))
}

#[derive(Debug, Deserialize)]
pub struct CreateForm {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub notes: String,
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<CreateForm>,
) -> AppResult<Response> {
    let name = form.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }
    let kind = form.kind.trim().to_string();
    if !SOURCE_KINDS.iter().any(|k| *k == kind) {
        return Err(AppError::BadRequest(format!("unknown kind: {kind}")));
    }
    let id = Uuid::now_v7();
    sqlx::query("insert into sources (id, name, kind, base_url, notes) values ($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(&name)
        .bind(&kind)
        .bind(empty_to_none(form.base_url))
        .bind(form.notes)
        .execute(&state.pool)
        .await?;
    Ok(Redirect::to("/sources").into_response())
}

pub async fn detail(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Source>("select * from sources where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    // Incidents touching this source = incidents that have at least one
    // linked record from this source. (Incidents themselves carry no source.)
    let incidents = sqlx::query_as::<_, Incident>(
        "select i.id, i.subject_id, i.title, i.narrative,
                i.occurred_at, i.occurred_precision,
                i.created_at, i.updated_at
           from incidents i
          where exists (
            select 1 from incident_records ir
              join records r on r.id = ir.record_id
             where ir.incident_id = i.id and r.source_id = $1
          )
          order by i.occurred_at desc nulls last, i.created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let records = sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records where source_id = $1
           order by occurred_at desc nulls last, created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: &s.name,
        current_path: "/sources",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(source::detail_page(&nav, &s, &incidents, &records, &subjects))
}
