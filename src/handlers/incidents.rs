use axum::extract::{Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{Incident, Record, Source, parse_date, parse_subject_filter};
use crate::viewer::ViewerContext;
use crate::views::incident;
use crate::views::layout::Nav;

const INCIDENT_COLS: &str = "id, subject_id, title, narrative, occurred_at, occurred_precision,
                              ended_at, ended_precision, created_at, updated_at";

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub subject: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<ListQuery>,
) -> AppResult<Markup> {
    let subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    list_render(&state, viewer, subject).await
}

pub async fn list_for_subject(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(subject_id): Path<Uuid>,
) -> AppResult<Markup> {
    list_render(&state, viewer, Some(subject_id)).await
}

async fn list_render(
    state: &AppState,
    viewer: ViewerContext,
    subject: Option<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let incidents = sqlx::query_as::<_, Incident>(&format!(
        "select {INCIDENT_COLS}
           from incidents
          where ($1::uuid is null or subject_id = $1)
          order by occurred_at desc nulls last, created_at desc
          limit 200"
    ))
    .bind(subject)
    .fetch_all(&state.pool)
    .await?;

    let nav = Nav {
        title: "Events",
        current_path: "/incidents",
        subjects: &subjects,
        current_subject: subject,
        viewer: &viewer,
    };
    Ok(incident::list_page(&nav, &incidents, &subjects))
}

#[derive(Debug, Deserialize, Default)]
pub struct NewQuery {
    pub subject: Option<String>,
}

pub async fn new(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<NewQuery>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let url_subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    let pre = url_subject
        .or(viewer.default_subject_id)
        .or_else(|| subjects.first().map(|s| s.id));
    let nav = Nav {
        title: "New incident",
        current_path: "/incidents",
        subjects: &subjects,
        current_subject: pre,
        viewer: &viewer,
    };
    Ok(incident::new_form(&nav, &subjects, pre, None))
}

#[derive(Debug, Deserialize)]
pub struct CreateForm {
    pub subject_id: String,
    pub title: String,
    #[serde(default)]
    pub occurred_at: String,
    /// Optional end date for a multi-day event (hospital stay, trip).
    #[serde(default)]
    pub ended_at: String,
    #[serde(default)]
    pub narrative: String,
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<CreateForm>,
) -> AppResult<Response> {
    let subject_id =
        Uuid::parse_str(form.subject_id.trim()).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let title = form.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    let occurred_at = parse_date(&form.occurred_at).map_err(AppError::BadRequest)?;
    let ended_at = parse_date(&form.ended_at).map_err(AppError::BadRequest)?;
    if let (Some(s), Some(e)) = (occurred_at, ended_at) {
        if e < s {
            return Err(AppError::BadRequest("end date is before the start date".into()));
        }
    }
    let id = Uuid::now_v7();

    sqlx::query(
        "insert into incidents (id, subject_id, title, narrative, occurred_at, ended_at)
         values ($1,$2,$3,$4,$5,$6)",
    )
    .bind(id)
    .bind(subject_id)
    .bind(&title)
    .bind(form.narrative)
    .bind(occurred_at)
    .bind(ended_at)
    .execute(&state.pool)
    .await?;

    Ok(Redirect::to(&format!("/incidents/{id}")).into_response())
}

pub async fn detail(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let incident = fetch_incident(&state.pool, id).await?;

    let linked = sqlx::query_as::<_, Record>(
        "select r.id, r.subject_id, r.kind, r.title, r.notes, r.occurred_at, r.occurred_precision,
                r.file_path, r.content_type, r.byte_size, r.sha256,
                r.preview_path, r.preview_content_type,
                r.thumbnail_path, r.thumbnail_content_type, r.study_instance_uid,
                r.dicom_metadata, r.instance_number,
                r.source_id, r.external_id, r.external_url, r.source_synced_at,
                r.created_at, r.updated_at
           from records r
           join incident_records ir on ir.record_id = r.id
          where ir.incident_id = $1
          order by r.study_instance_uid nulls last,
                   case r.kind when 'report' then 1 else 0 end,
                   r.instance_number nulls last,
                   r.occurred_at desc nulls last, r.created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    // Sources touching this incident, derived from its linked records.
    let touching_sources = sqlx::query_as::<_, Source>(
        "select distinct s.* from sources s
           join records r on r.source_id = s.id
           join incident_records ir on ir.record_id = r.id
          where ir.incident_id = $1
          order by s.name",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let candidates = sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records
          where subject_id = $1
            and not exists (select 1 from incident_records ir where ir.incident_id = $2 and ir.record_id = records.id)
          order by created_at desc
          limit 100",
    )
    .bind(incident.subject_id)
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let linked_incidents = sqlx::query_as::<_, Incident>(&format!(
        "select {INCIDENT_COLS} from incidents
          where id in (
              select linked_incident_id from incident_links where incident_id = $1
              union
              select incident_id from incident_links where linked_incident_id = $1
          )
          order by occurred_at desc nulls last, created_at desc"
    ))
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let candidate_incidents = sqlx::query_as::<_, Incident>(&format!(
        "select {INCIDENT_COLS} from incidents
          where id <> $1
            and not exists (
                select 1 from incident_links il
                 where (il.incident_id = $1 and il.linked_incident_id = incidents.id)
                    or (il.linked_incident_id = $1 and il.incident_id = incidents.id)
            )
          order by occurred_at desc nulls last, created_at desc
          limit 200"
    ))
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let nav = Nav {
        title: &incident.title,
        current_path: "/incidents",
        subjects: &subjects,
        current_subject: Some(incident.subject_id),
        viewer: &viewer,
    };
    Ok(incident::detail_page(
        &nav,
        &incident,
        &subjects,
        &touching_sources,
        &linked,
        &candidates,
        &linked_incidents,
        &candidate_incidents,
    ))
}

pub async fn edit_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let incident = fetch_incident(&state.pool, id).await?;
    let nav = Nav {
        title: "Edit incident",
        current_path: "/incidents",
        subjects: &subjects,
        current_subject: Some(incident.subject_id),
        viewer: &viewer,
    };
    Ok(incident::edit_form(&nav, &incident, &subjects, None))
}

pub async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<CreateForm>,
) -> AppResult<Response> {
    let subject_id =
        Uuid::parse_str(form.subject_id.trim()).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let title = form.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    let occurred_at = parse_date(&form.occurred_at).map_err(AppError::BadRequest)?;
    let ended_at = parse_date(&form.ended_at).map_err(AppError::BadRequest)?;
    if let (Some(s), Some(e)) = (occurred_at, ended_at) {
        if e < s {
            return Err(AppError::BadRequest("end date is before the start date".into()));
        }
    }
    sqlx::query(
        "update incidents set
            subject_id  = $2,
            title       = $3,
            narrative   = $4,
            occurred_at = $5,
            ended_at    = $6,
            updated_at  = now()
          where id = $1",
    )
    .bind(id)
    .bind(subject_id)
    .bind(&title)
    .bind(form.narrative)
    .bind(occurred_at)
    .bind(ended_at)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/incidents/{id}")).into_response())
}

#[derive(Debug, Deserialize)]
pub struct LinkForm {
    pub record_id: String,
    #[serde(default)]
    pub note: String,
}

pub async fn link_record(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<LinkForm>,
) -> AppResult<Response> {
    let record_id = Uuid::parse_str(form.record_id.trim())
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    sqlx::query(
        "insert into incident_records (incident_id, record_id, note)
         values ($1,$2,$3)
         on conflict (incident_id, record_id) do update set note = excluded.note",
    )
    .bind(id)
    .bind(record_id)
    .bind(form.note)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/incidents/{id}")).into_response())
}

pub async fn unlink_record(
    State(state): State<AppState>,
    Path((id, rid)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    sqlx::query("delete from incident_records where incident_id = $1 and record_id = $2")
        .bind(id)
        .bind(rid)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct LinkIncidentForm {
    pub linked_incident_id: String,
}

pub async fn link_incident(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<LinkIncidentForm>,
) -> AppResult<Response> {
    let other_id = Uuid::parse_str(form.linked_incident_id.trim())
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    // Store one direction; queries union both directions.
    sqlx::query(
        "insert into incident_links (incident_id, linked_incident_id)
         values ($1, $2)
         on conflict do nothing",
    )
    .bind(id)
    .bind(other_id)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/incidents/{id}")).into_response())
}

pub async fn unlink_incident(
    State(state): State<AppState>,
    Path((id, other_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    sqlx::query(
        "delete from incident_links
          where (incident_id = $1 and linked_incident_id = $2)
             or (incident_id = $2 and linked_incident_id = $1)",
    )
    .bind(id)
    .bind(other_id)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::OK)
}

async fn fetch_incident(pool: &PgPool, id: Uuid) -> Result<Incident, sqlx::Error> {
    sqlx::query_as::<_, Incident>(&format!(
        "select {INCIDENT_COLS} from incidents where id = $1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
}
