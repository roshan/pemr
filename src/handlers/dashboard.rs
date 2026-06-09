use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use uuid::Uuid;
use maud::Markup;
use serde::Deserialize;
use sqlx::PgPool;

use time::{Date, Duration, Month};

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
    /// Explicit window (ISO `YYYY-MM-DD`), set by wheel-zoom and the date boxes.
    pub from: Option<String>,
    pub to: Option<String>,
}

pub async fn timeline_page(
    State(state): State<AppState>,
    viewer: ViewerContext,
    headers: HeaderMap,
    Query(q): Query<TimelineQuery>,
) -> AppResult<Markup> {
    let subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    timeline_render(&state, viewer, subject, &q, is_hx(&headers)).await
}

pub async fn timeline_for_subject(
    State(state): State<AppState>,
    viewer: ViewerContext,
    headers: HeaderMap,
    Path(subject_id): Path<Uuid>,
    Query(q): Query<TimelineQuery>,
) -> AppResult<Markup> {
    timeline_render(&state, viewer, Some(subject_id), &q, is_hx(&headers)).await
}

fn is_hx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

async fn timeline_render(
    state: &AppState,
    viewer: ViewerContext,
    subject: Option<Uuid>,
    q: &TimelineQuery,
    hx: bool,
) -> AppResult<Markup> {
    let data = load_timeline(
        &state.pool,
        subject,
        q.range.as_deref().unwrap_or("1y"),
        q.from.as_deref(),
        q.to.as_deref(),
    )
    .await?;
    // htmx zoom/window requests swap just the inner band; full loads get the
    // page. The inner band doesn't need the subjects list, so skip that query on
    // the hot zoom path.
    if hx {
        return Ok(dashboard::timeline_inner(&data));
    }
    let subjects = load_subjects(&state.pool).await?;
    let nav = Nav {
        title: "Timeline",
        current_path: "/timeline",
        subjects: &subjects,
        current_subject: subject,
        viewer: &viewer,
    };
    Ok(dashboard::visual_timeline(&nav, &data))
}

/// Every dated clinical artifact for a subject (or all subjects), unioned into
/// one event stream, sorted oldest-first. Shared by the windowed timeline and
/// the per-point detail panel.
async fn load_events(
    pool: &PgPool,
    subject: Option<Uuid>,
) -> Result<Vec<dashboard::TimelineEvent>, sqlx::Error> {
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

    Ok(rows
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
        .collect())
}

/// Build the timeline model for a subject (or all subjects). Reused by the
/// `/timeline` page and the subject chart. An explicit `from`/`to` window (ISO)
/// overrides the `range` preset.
pub async fn load_timeline(
    pool: &PgPool,
    subject: Option<Uuid>,
    range: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<dashboard::TimelineData, sqlx::Error> {
    let events = load_events(pool, subject).await?;
    Ok(build_timeline_data(events, range, from, to, subject))
}

#[derive(Debug, Deserialize)]
pub struct TimelineDayQuery {
    pub subject: Option<String>,
    pub from: String,
    pub to: String,
}

/// The persistent detail panel for a clicked timeline point: every event whose
/// date falls in the bucket's `[from,to]` span. Always an htmx partial.
pub async fn timeline_day(
    State(state): State<AppState>,
    Query(q): Query<TimelineDayQuery>,
) -> AppResult<Markup> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    let from = parse_iso(&q.from).ok_or_else(|| AppError::BadRequest("invalid 'from' date".into()))?;
    let to = parse_iso(&q.to).ok_or_else(|| AppError::BadRequest("invalid 'to' date".into()))?;
    let subjects = load_subjects(&state.pool).await?;
    let mut events = load_events(&state.pool, subject).await?;
    events.retain(|e| e.date >= from && e.date <= to);
    events.sort_by_key(|e| e.date);
    let heading = if from == to {
        fmt_day(from)
    } else {
        format!("{} – {}", fmt_day(from), fmt_day(to))
    };
    Ok(dashboard::timeline_day_detail(&events, subject, &subjects, &heading))
}

fn fmt_day(d: Date) -> String {
    format!("{} {}, {}", month3(d.month()), d.day(), d.year())
}

fn parse_iso(s: &str) -> Option<Date> {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(s, fmt).ok()
}

/// Window + group events **time-proportionally** so gaps reflect real time and
/// clusters read as dense. An explicit `from`/`to` window (from wheel-zoom or
/// the date boxes) wins; otherwise the `range` preset anchors on the latest
/// event. Bucket granularity follows the span so dots don't pile up. Positions
/// are percentages — the band fits to width, no dead scroll space.
fn build_timeline_data(
    mut events: Vec<dashboard::TimelineEvent>,
    range: &str,
    from: Option<&str>,
    to: Option<&str>,
    subject: Option<Uuid>,
) -> dashboard::TimelineData {
    events.sort_by_key(|e| e.date);
    if events.is_empty() {
        return dashboard::TimelineData {
            range: range.to_string(),
            start: String::new(),
            end: String::new(),
            min: String::new(),
            max: String::new(),
            ticks: vec![],
            buckets: vec![],
            subject,
        };
    }
    let min_d = events.first().unwrap().date;
    let max_d = events.last().unwrap().date;

    // Window: explicit from/to wins; else the preset anchored on the latest event.
    let (start, end, range_label) = match (from.and_then(parse_iso), to.and_then(parse_iso)) {
        (Some(a), Some(b)) if b > a => (a, b, String::new()),
        _ => {
            let r = match range {
                "3m" | "1y" | "5y" | "all" => range,
                _ => "1y",
            };
            let span = match r {
                "3m" => 91,
                "1y" => 365,
                "5y" => 1826,
                _ => (max_d - min_d).whole_days().max(1),
            };
            let start = max_d
                .checked_sub(Duration::days(span))
                .filter(|s| *s > min_d)
                .unwrap_or(min_d);
            (start, max_d, r.to_string())
        }
    };

    let span = (end - start).whole_days().max(1) as f64;
    // Coarser buckets as the window widens.
    let bucket = |d: Date| -> Date {
        if span <= 150.0 {
            d
        } else if span <= 1100.0 {
            d.checked_sub(Duration::days(d.weekday().number_days_from_monday() as i64))
                .unwrap_or(d)
        } else {
            Date::from_calendar_date(d.year(), d.month(), 1).unwrap_or(d)
        }
    };

    let mut buckets: Vec<dashboard::TimelineBucket> = Vec::new();
    for e in events.into_iter().filter(|e| e.date >= start && e.date <= end) {
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

    dashboard::TimelineData {
        range: range_label,
        start: start.to_string(),
        end: end.to_string(),
        min: min_d.to_string(),
        max: max_d.to_string(),
        ticks: timeline_ticks(start, end),
        buckets,
        subject,
    }
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
        idx += step;
        // Day 1 of any real month is always a valid date; skip rather than panic
        // if `time`'s year bounds are ever exceeded.
        let Ok(d) = Date::from_calendar_date(y, m, 1) else { continue };
        if d >= start {
            let label = if step >= 12 {
                y.to_string()
            } else {
                format!("{} '{:02}", month3(m), y.rem_euclid(100))
            };
            out.push((pct(d), label));
        }
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
                ended_at, ended_precision, created_at, updated_at
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

