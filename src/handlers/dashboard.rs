use axum::extract::{Path, Query, State};
use uuid::Uuid;
use maud::Markup;
use serde::Deserialize;
use sqlx::PgPool;

use time::Date;

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
    pub range: Option<String>,
}

pub async fn timeline_page(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<TimelineQuery>,
) -> AppResult<Markup> {
    let subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    timeline_render(&state, viewer, subject, q.range.as_deref()).await
}

pub async fn timeline_for_subject(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(subject_id): Path<Uuid>,
) -> AppResult<Markup> {
    timeline_render(&state, viewer, Some(subject_id), None).await
}

async fn timeline_render(
    state: &AppState,
    viewer: ViewerContext,
    subject: Option<Uuid>,
    range: Option<&str>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let data = load_timeline(&state.pool, subject, range.unwrap_or("all")).await?;
    let nav = Nav {
        title: "Timeline",
        current_path: "/timeline",
        subjects: &subjects,
        current_subject: subject,
        viewer: &viewer,
    };
    Ok(dashboard::visual_timeline(&nav, &data, &subjects))
}

/// Build the timeline model for a subject (or all subjects). Reused by the
/// `/timeline` page and the subject chart.
pub async fn load_timeline(
    pool: &PgPool,
    subject: Option<Uuid>,
    range: &str,
) -> Result<dashboard::TimelineData, sqlx::Error> {
    // Every dated clinical artifact, unioned into one event stream.
    let rows = sqlx::query_as::<_, (Date, String, String, String, Uuid)>(
        "select occurred_at as d, 'incident' as kind, title as t, id::text as i, subject_id as s
           from incidents where occurred_at is not null and ($1::uuid is null or subject_id = $1)
         union all
         select occurred_at, 'record', title, id::text, subject_id
           from records where occurred_at is not null and ($1::uuid is null or subject_id = $1)
         union all
         select onset_date, 'condition', name, id::text, subject_id
           from conditions where onset_date is not null and ($1::uuid is null or subject_id = $1)
         union all
         select occurred_at, 'immunization', vaccine, id::text, subject_id
           from immunizations where occurred_at is not null and ($1::uuid is null or subject_id = $1)
         union all
         select effective_on, 'observation', display, id::text, subject_id
           from observations where ($1::uuid is null or subject_id = $1)
         union all
         select starts_at::date, 'appointment', title, id::text, subject_id
           from appointments where ($1::uuid is null or subject_id = $1)
         order by d asc",
    )
    .bind(subject)
    .fetch_all(pool)
    .await?;

    let events: Vec<dashboard::TimelineEvent> = rows
        .into_iter()
        .map(|(date, kind, title, id, sid)| {
            let href = match kind.as_str() {
                "incident" => Some(format!("/incidents/{id}")),
                "record" => Some(format!("/records/{id}")),
                "appointment" => Some(format!("/appointments/{id}/edit")),
                _ => Some(format!("/subjects/{sid}")),
            };
            dashboard::TimelineEvent { date, kind, title, href, subject_id: sid }
        })
        .collect();

    Ok(build_timeline_data(events, range, subject))
}

/// Window, group, and lay out events. Dots are spaced **evenly** (by index, not
/// calendar position) so a lone outlier — a 2013 record amid 2025 data — can't
/// stretch the axis and push everything off-screen; the real date rides along in
/// each marker's label and popover.
fn build_timeline_data(
    mut events: Vec<dashboard::TimelineEvent>,
    range: &str,
    subject: Option<Uuid>,
) -> dashboard::TimelineData {
    let range = match range {
        "1y" | "3y" | "5y" | "all" => range,
        _ => "all",
    }
    .to_string();
    events.sort_by_key(|e| e.date);
    if events.is_empty() {
        return dashboard::TimelineData { range, width_px: 1000, buckets: vec![], subject };
    }

    // Window anchored on the data (not "today"): the latest event is the right
    // edge; the duration trims older events off the left.
    let min_d = events.first().unwrap().date;
    let end = events.last().unwrap().date;
    let years: Option<i64> = match range.as_str() {
        "1y" => Some(1),
        "3y" => Some(3),
        "5y" => Some(5),
        _ => None,
    };
    let start = years
        .and_then(|y| end.checked_sub(time::Duration::days(365 * y)))
        .filter(|s| *s > min_d)
        .unwrap_or(min_d);

    let mut buckets: Vec<dashboard::TimelineBucket> = Vec::new();
    for e in events.into_iter().filter(|e| e.date >= start) {
        match buckets.last_mut() {
            Some(b) if b.date == e.date => b.events.push(e),
            _ => buckets.push(dashboard::TimelineBucket {
                pct: 0.0,
                date: e.date,
                kind: String::new(),
                events: vec![e],
            }),
        }
    }
    let n = buckets.len();
    for (i, b) in buckets.iter_mut().enumerate() {
        b.pct = if n <= 1 { 50.0 } else { 3.0 + i as f64 / (n - 1) as f64 * 94.0 };
        b.kind = primary_kind(&b.events);
    }
    let width_px = (n as f64 * 64.0).clamp(1000.0, 6000.0) as i64;

    dashboard::TimelineData { range, width_px, buckets, subject }
}

/// The dot colour follows the highest-priority kind present in the bucket.
fn primary_kind(events: &[dashboard::TimelineEvent]) -> String {
    const ORDER: [&str; 6] =
        ["incident", "appointment", "record", "condition", "immunization", "observation"];
    for k in ORDER {
        if events.iter().any(|e| e.kind == k) {
            return k.to_string();
        }
    }
    events.first().map(|e| e.kind.clone()).unwrap_or_default()
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

