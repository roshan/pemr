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
    .fetch_all(&state.pool)
    .await?;

    let events: Vec<dashboard::TimelineEvent> = rows
        .into_iter()
        .map(|(date, kind, title, id, sid)| {
            let href = match kind.as_str() {
                "incident" => Some(format!("/incidents/{id}")),
                "record" => Some(format!("/records/{id}")),
                _ => Some(format!("/subjects/{sid}")),
            };
            dashboard::TimelineEvent { date, kind, title, href, subject_id: sid }
        })
        .collect();

    let data = build_timeline_data(events, range.unwrap_or("all"), subject);
    let nav = Nav {
        title: "Timeline",
        current_path: "/timeline",
        subjects: &subjects,
        current_subject: subject,
        viewer: &viewer,
    };
    Ok(dashboard::visual_timeline(&nav, &data, &subjects))
}

/// Window, group, and lay out events for the timeline view.
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
        return dashboard::TimelineData { range, width_px: 1000, ticks: vec![], buckets: vec![], subject };
    }

    // Anchor the window on the data (not "today"), so imported historical
    // records frame nicely; the duration zooms within that.
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

    let span = (end - start).whole_days().max(1) as f64;
    let pct = |d: Date| 3.0 + (d - start).whole_days() as f64 / span * 94.0;
    let width_px = (span * 3.0).clamp(1000.0, 6000.0) as i64;

    let mut buckets: Vec<dashboard::TimelineBucket> = Vec::new();
    for e in events.into_iter().filter(|e| e.date >= start) {
        match buckets.last_mut() {
            Some(b) if b.date == e.date => b.events.push(e),
            _ => buckets.push(dashboard::TimelineBucket {
                pct: pct(e.date),
                date: e.date,
                kind: String::new(),
                events: vec![e],
            }),
        }
    }
    for b in &mut buckets {
        b.kind = primary_kind(&b.events);
    }

    dashboard::TimelineData { range, width_px, ticks: timeline_ticks(start, end, span), buckets, subject }
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

fn month3(m: Month) -> &'static str {
    match m {
        Month::January => "Jan",
        Month::February => "Feb",
        Month::March => "Mar",
        Month::April => "Apr",
        Month::May => "May",
        Month::June => "Jun",
        Month::July => "Jul",
        Month::August => "Aug",
        Month::September => "Sep",
        Month::October => "Oct",
        Month::November => "Nov",
        Month::December => "Dec",
    }
}

fn first_of_next_month(y: i32, m: Month) -> (i32, Month, Date) {
    let (ny, nm) = if m == Month::December { (y + 1, Month::January) } else { (y, m.next()) };
    (ny, nm, Date::from_calendar_date(ny, nm, 1).unwrap())
}

/// Axis ticks: monthly for short windows, yearly for long ones.
fn timeline_ticks(start: Date, end: Date, span: f64) -> Vec<(f64, String)> {
    let pct = |d: Date| 3.0 + (d - start).whole_days() as f64 / span * 94.0;
    let mut out = Vec::new();
    if span <= 800.0 {
        let (mut y, mut m) = (start.year(), start.month());
        let mut d = Date::from_calendar_date(y, m, 1).unwrap_or(start);
        while d < start {
            let (ny, nm, nd) = first_of_next_month(y, m);
            y = ny;
            m = nm;
            d = nd;
        }
        while d <= end {
            out.push((pct(d), format!("{} {}", month3(m), y)));
            let (ny, nm, nd) = first_of_next_month(y, m);
            y = ny;
            m = nm;
            d = nd;
        }
    } else {
        for yr in start.year()..=end.year() {
            let jan = Date::from_calendar_date(yr, Month::January, 1).unwrap();
            let d = if jan < start { start } else { jan };
            out.push((pct(d), yr.to_string()));
        }
    }
    out
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

