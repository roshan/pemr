use axum::extract::{Path, Query, State};
use uuid::Uuid;
use maud::Markup;
use serde::Deserialize;
use sqlx::PgPool;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{Incident, Record, parse_subject_filter};
use crate::viewer::ViewerContext;
use crate::views::dashboard::{self, DashboardData};
use crate::views::layout::Nav;

#[derive(Debug, Deserialize, Default)]
pub struct DashQuery {
    pub subject: Option<String>,
}

pub async fn index(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<DashQuery>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let explicit =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    let current_subject = match q.subject.as_deref() {
        None => viewer.default_subject_id,
        _ => explicit,
    };

    let timeline_limit = dashboard::dashboard_timeline_limit() as i64;
    let timeline_incidents = timeline(&state.pool, current_subject, timeline_limit).await?;
    let timeline_total = count_incidents(&state.pool, current_subject).await?;
    let recent_incidents = recent_incidents(&state.pool, current_subject).await?;
    let recent_records = recent_records(&state.pool, current_subject).await?;

    let nav = Nav {
        title: "Dashboard",
        current_path: "/",
        subjects: &subjects,
        current_subject,
        viewer: &viewer,
    };
    let data = DashboardData {
        subjects: &subjects,
        timeline_incidents: &timeline_incidents,
        timeline_total,
        recent_incidents: &recent_incidents,
        recent_records: &recent_records,
    };
    Ok(dashboard::render(&nav, &data))
}

pub async fn healthz() -> &'static str {
    "ok"
}

#[derive(Debug, Deserialize, Default)]
pub struct TimelineQuery {
    pub subject: Option<String>,
}

pub async fn timeline_page(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<TimelineQuery>,
) -> AppResult<Markup> {
    let subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    timeline_render(&state, viewer, subject).await
}

pub async fn timeline_for_subject(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(subject_id): Path<Uuid>,
) -> AppResult<Markup> {
    timeline_render(&state, viewer, Some(subject_id)).await
}

async fn timeline_render(
    state: &AppState,
    viewer: ViewerContext,
    subject: Option<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let incidents = sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                created_at, updated_at
           from incidents
          where ($1::uuid is null or subject_id = $1)
          order by occurred_at desc nulls last, created_at desc",
    )
    .bind(subject)
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: "Timeline",
        current_path: "/timeline",
        subjects: &subjects,
        current_subject: subject,
        viewer: &viewer,
    };
    Ok(dashboard::full_timeline(&nav, &incidents, &subjects))
}

async fn timeline(
    pool: &PgPool,
    subject: Option<uuid::Uuid>,
    limit: i64,
) -> Result<Vec<Incident>, sqlx::Error> {
    sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                created_at, updated_at
           from incidents
          where ($1::uuid is null or subject_id = $1)
          order by occurred_at desc nulls last, created_at desc
          limit $2",
    )
    .bind(subject)
    .bind(limit)
    .fetch_all(pool)
    .await
}

async fn count_incidents(
    pool: &PgPool,
    subject: Option<uuid::Uuid>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "select count(*) from incidents where ($1::uuid is null or subject_id = $1)",
    )
    .bind(subject)
    .fetch_one(pool)
    .await
}

async fn recent_incidents(
    pool: &PgPool,
    subject: Option<uuid::Uuid>,
) -> Result<Vec<Incident>, sqlx::Error> {
    sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                created_at, updated_at
           from incidents
          where ($1::uuid is null or subject_id = $1)
          order by created_at desc
          limit 10",
    )
    .bind(subject)
    .fetch_all(pool)
    .await
}

async fn recent_records(
    pool: &PgPool,
    subject: Option<uuid::Uuid>,
) -> Result<Vec<Record>, sqlx::Error> {
    sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records
          where ($1::uuid is null or subject_id = $1)
          order by created_at desc
          limit 10",
    )
    .bind(subject)
    .fetch_all(pool)
    .await
}

