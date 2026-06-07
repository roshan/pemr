use axum::extract::{Path, Query, State};
use uuid::Uuid;
use maud::Markup;
use serde::Deserialize;
use sqlx::PgPool;

use time::{Date, Month};

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
    let data = load_timeline(&state.pool, subject, range.unwrap_or("1y")).await?;
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

/// Window + group events **time-proportionally** so gaps reflect real time and
/// clusters read as dense. The zoom level sets both the window (anchored on the
/// latest event) and the bucket granularity, so dots stay legible without
/// overlap as you zoom out. Positions are percentages — the view fits the band
/// to its width, so there's no dead scroll space.
fn build_timeline_data(
    mut events: Vec<dashboard::TimelineEvent>,
    range: &str,
    subject: Option<Uuid>,
) -> dashboard::TimelineData {
    let range = match range {
        "3m" | "1y" | "5y" | "all" => range,
        _ => "1y",
    }
    .to_string();
    events.sort_by_key(|e| e.date);
    if events.is_empty() {
        return dashboard::TimelineData { range, ticks: vec![], buckets: vec![], subject };
    }

    let min_d = events.first().unwrap().date;
    let end = events.last().unwrap().date;
    let window_days: Option<i64> = match range.as_str() {
        "3m" => Some(91),
        "1y" => Some(365),
        "5y" => Some(1826),
        _ => None, // all
    };
    let start = window_days
        .and_then(|d| end.checked_sub(time::Duration::days(d)))
        .filter(|s| *s > min_d)
        .unwrap_or(min_d);

    // Coarser buckets as the window widens, so dots don't pile up.
    let bucket = |d: Date| -> Date {
        match range.as_str() {
            "3m" | "1y" => d, // day
            "5y" => d
                .checked_sub(time::Duration::days(d.weekday().number_days_from_monday() as i64))
                .unwrap_or(d), // week (Monday)
            _ => Date::from_calendar_date(d.year(), d.month(), 1).unwrap_or(d), // month
        }
    };

    let span = (end - start).whole_days().max(1) as f64;
    let mut buckets: Vec<dashboard::TimelineBucket> = Vec::new();
    for e in events.into_iter().filter(|e| e.date >= start) {
        let anchor = bucket(e.date);
        match buckets.last_mut() {
            Some(b) if b.date == anchor => b.events.push(e),
            _ => buckets.push(dashboard::TimelineBucket {
                pct: 0.0,
                date: anchor,
                kind: String::new(),
                events: vec![e],
            }),
        }
    }
    for b in &mut buckets {
        b.pct = 4.0 + (b.date - start).whole_days() as f64 / span * 92.0;
        b.kind = primary_kind(&b.events);
    }

    dashboard::TimelineData { range, ticks: timeline_ticks(start, end), buckets, subject }
}

fn month3(m: Month) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][m as usize - 1]
}

/// Month/year axis labels, spaced so they don't collide: monthly for short
/// windows, quarterly mid, yearly when zoomed out.
fn timeline_ticks(start: Date, end: Date) -> Vec<(f64, String)> {
    let span = (end - start).whole_days().max(1) as f64;
    let pct = |d: Date| 4.0 + (d - start).whole_days() as f64 / span * 92.0;
    let step: i32 = if span <= 400.0 {
        1
    } else if span <= 1200.0 {
        3
    } else {
        12
    };
    let mut out = Vec::new();
    let mut idx = start.year() * 12 + (start.month() as i32 - 1); // months since year 0
    let end_idx = end.year() * 12 + (end.month() as i32 - 1);
    while idx <= end_idx {
        let (y, m) = (idx.div_euclid(12), Month::try_from(idx.rem_euclid(12) as u8 + 1).unwrap());
        let d = Date::from_calendar_date(y, m, 1).unwrap();
        if d >= start {
            let label = if step >= 12 {
                y.to_string()
            } else {
                format!("{} '{:02}", month3(m), y.rem_euclid(100))
            };
            out.push((pct(d), label));
        }
        idx += step;
    }
    out
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

