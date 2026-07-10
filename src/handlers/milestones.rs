//! Developmental-milestone tracker handlers (PEMR-40/41/42/44) + the per-subject
//! feature registry's add/remove endpoints (PEMR-45). The checklist and the
//! feature area are server-rendered HTML partials swapped in place by HTMX; the
//! progress + printable-summary pages are full pages.

use axum::extract::{Form, Path, Query, State};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{MilestoneResponse, Subject, empty_to_none, parse_date};
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::{feature_registry, milestone_age, milestones, peds, views};

async fn load_subject(pool: &sqlx::PgPool, id: Uuid) -> Result<Subject, sqlx::Error> {
    sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
}

async fn load_responses(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Vec<MilestoneResponse>, sqlx::Error> {
    sqlx::query_as::<_, MilestoneResponse>(
        "select * from milestone_responses where subject_id = $1",
    )
    .bind(id)
    .fetch_all(pool)
    .await
}

/// The subject's current tracker age (basis + checkpoint), if they have a DOB.
fn tracker_for(s: &Subject) -> Option<milestone_age::TrackerAge> {
    s.dob
        .map(|dob| milestone_age::tracker_age(dob, s.gestational_age_weeks, peds::today()))
}

/// The rendered milestone feature surface (inline module): checklist for the
/// computed checkpoint, or a "set DOB" notice.
async fn milestone_module(pool: &sqlx::PgPool, s: &Subject) -> AppResult<Markup> {
    let tracker = tracker_for(s);
    let inner = match tracker {
        Some(t) => {
            let responses = load_responses(pool, s.id).await?;
            views::milestones::checklist(s.id, t.checkpoint, Some(t.checkpoint), &responses)
        }
        None => views::milestones::needs_dob(s),
    };
    Ok(views::milestones::module(s, tracker, inner))
}

/// Render the `#subject-features` area: every enabled feature's surface + the
/// "Add feature" picker. Shared by the chart page and the add/remove handlers.
pub async fn render_feature_area(pool: &sqlx::PgPool, s: &Subject) -> AppResult<Markup> {
    let enabled = feature_registry::enabled_keys(pool, s.id).await?;
    let mut surfaces: Vec<Markup> = Vec::new();
    for key in &enabled {
        match key.as_str() {
            "milestones" => surfaces.push(milestone_module(pool, s).await?),
            // Future features (e.g. growth, PEMR-47) render their surface here.
            _ => {}
        }
    }
    let available = feature_registry::available_to_add(pool, s.id).await?;
    Ok(views::milestones::feature_area(s.id, surfaces, &available))
}

/// `POST /subjects/{id}/features/{key}` — enable a feature (idempotent) and swap
/// the feature area back in so the surface pops in with no reload.
pub async fn enable_feature(
    State(state): State<AppState>,
    Path((id, key)): Path<(Uuid, String)>,
) -> AppResult<Markup> {
    if feature_registry::by_key(&key).is_none() {
        return Err(AppError::BadRequest(format!("unknown feature: {key}")));
    }
    let s = load_subject(&state.pool, id).await?;
    feature_registry::enable(&state.pool, id, &key).await?;
    render_feature_area(&state.pool, &s).await
}

/// `POST /subjects/{id}/features/{key}/remove` — disable a feature. Hides the
/// surface; the module's underlying data is preserved (disable, never delete).
pub async fn disable_feature(
    State(state): State<AppState>,
    Path((id, key)): Path<(Uuid, String)>,
) -> AppResult<Markup> {
    let s = load_subject(&state.pool, id).await?;
    feature_registry::disable(&state.pool, id, &key).await?;
    render_feature_area(&state.pool, &s).await
}

#[derive(Debug, Deserialize)]
pub struct ChecklistQuery {
    pub checkpoint: Option<i32>,
}

/// Resolve which checkpoint to show. Validates an explicit `?checkpoint=`; falls
/// back to the subject's computed checkpoint, then the first checkpoint.
fn resolve_checkpoint(s: &Subject, requested: Option<i32>) -> AppResult<(i32, Option<i32>)> {
    let computed = tracker_for(s).map(|t| t.checkpoint);
    let checkpoint = match requested {
        Some(cp) => {
            if !milestones::CHECKPOINTS.contains(&cp) {
                return Err(AppError::BadRequest(format!("unknown checkpoint: {cp}")));
            }
            cp
        }
        None => computed.unwrap_or(milestones::CHECKPOINTS[0]),
    };
    Ok((checkpoint, computed))
}

/// `GET /subjects/{id}/milestones/checklist?checkpoint=N` — the swappable
/// checklist partial (checkpoint stepper).
pub async fn checklist(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ChecklistQuery>,
) -> AppResult<Markup> {
    let s = load_subject(&state.pool, id).await?;
    let (checkpoint, computed) = resolve_checkpoint(&s, q.checkpoint)?;
    let responses = load_responses(&state.pool, id).await?;
    Ok(views::milestones::checklist(id, checkpoint, computed, &responses))
}

#[derive(Debug, Deserialize)]
pub struct MarkForm {
    pub response: String,
    #[serde(default)]
    pub observed_on: String,
    #[serde(default)]
    pub checkpoint: String,
    #[serde(default)]
    pub note: String,
}

/// `POST /subjects/{id}/milestones/{key}` — record (upsert) a milestone response,
/// then swap the checklist back in. Captures the point-in-time age snapshot
/// (basis + chronological age in days) per the queryability spec.
pub async fn mark(
    State(state): State<AppState>,
    Path((id, key)): Path<(Uuid, String)>,
    Form(f): Form<MarkForm>,
) -> AppResult<Markup> {
    if !milestones::RESPONSES.contains(&f.response.as_str()) {
        return Err(AppError::BadRequest(format!("invalid response: {}", f.response)));
    }
    let m = milestones::by_key(&key)
        .ok_or_else(|| AppError::BadRequest(format!("unknown milestone: {key}")))?;

    let s = load_subject(&state.pool, id).await?;
    // Marking requires a DOB — the age snapshot is part of the record.
    let dob = s
        .dob
        .ok_or_else(|| AppError::BadRequest("set date of birth before marking milestones".into()))?;
    let today = peds::today();
    let tracker = milestone_age::tracker_age(dob, s.gestational_age_weeks, today);
    let chronological_age_days = (today.to_julian_day() - dob.to_julian_day()).max(0);

    // observed_on is meaningful only for a met ("yes") milestone; default to today
    // when the user didn't pick a date. Cleared for not_yet / no.
    let observed_on = if f.response == "yes" {
        Some(parse_date(&f.observed_on).map_err(AppError::BadRequest)?.unwrap_or(today))
    } else {
        None
    };

    sqlx::query(
        "insert into milestone_responses
           (id, subject_id, milestone_key, domain, expected_age_months, response,
            observed_on, age_basis_used, chronological_age_days, note, answered_at)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10, now())
         on conflict (subject_id, milestone_key) do update set
            response = excluded.response,
            domain = excluded.domain,
            expected_age_months = excluded.expected_age_months,
            observed_on = excluded.observed_on,
            age_basis_used = excluded.age_basis_used,
            chronological_age_days = excluded.chronological_age_days,
            note = excluded.note,
            answered_at = now()",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(&key)
    .bind(m.domain)
    .bind(m.checkpoint_months as i16)
    .bind(&f.response)
    .bind(observed_on)
    .bind(tracker.basis.as_str())
    .bind(chronological_age_days)
    .bind(empty_to_none(f.note))
    .execute(&state.pool)
    .await?;

    // Re-render the same checkpoint the user is looking at.
    let requested: Option<i32> = f.checkpoint.trim().parse().ok();
    let (checkpoint, computed) = resolve_checkpoint(&s, requested)?;
    let responses = load_responses(&state.pool, id).await?;
    Ok(views::milestones::checklist(id, checkpoint, computed, &responses))
}

/// `GET /subjects/{id}/milestones/progress` — the dated-progress view (PEMR-44).
pub async fn progress(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = load_subject(&state.pool, id).await?;
    let tracker = tracker_for(&s);
    let responses = load_responses(&state.pool, id).await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(views::milestones::progress_page(&nav, &s, tracker, &responses))
}

/// `GET /subjects/{id}/milestones/summary` — the printable, attachable summary
/// (PEMR-42).
pub async fn summary(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = load_subject(&state.pool, id).await?;
    let tracker = tracker_for(&s);
    let checkpoint = tracker.map(|t| t.checkpoint).unwrap_or(milestones::CHECKPOINTS[0]);
    let responses = load_responses(&state.pool, id).await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(views::milestones::summary_page(&nav, &s, tracker, checkpoint, &responses))
}
