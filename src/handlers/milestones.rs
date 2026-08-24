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
use crate::models::{MilestoneResponse, Subject, parse_date};
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

/// Default observed-on date for marking a milestone "yes": the latest date the
/// child was within that milestone's age `period` (today for the current period).
/// Falls back to today with no DOB (the upsert rejects a DOB-less mark anyway).
fn default_observed_date(s: &Subject, period: i32) -> time::Date {
    let today = peds::today();
    match s.dob {
        Some(dob) => milestone_age::latest_date_in_period(dob, s.gestational_age_weeks, period, today),
        None => today,
    }
}

/// Milestones marked "yes" out of the total, at one checkpoint.
fn checkpoint_stats(checkpoint: i32, responses: &[MilestoneResponse]) -> (usize, usize) {
    let items = milestones::by_checkpoint(checkpoint);
    let met = items
        .iter()
        .filter(|m| {
            responses
                .iter()
                .any(|r| r.milestone_key == m.key && r.response == "yes")
        })
        .count();
    (met, items.len())
}

/// Per-period completion: `(checkpoint, met, total)` for every checkpoint.
fn per_period_stats(responses: &[MilestoneResponse]) -> Vec<(i32, usize, usize)> {
    milestones::CHECKPOINTS
        .iter()
        .map(|&cp| {
            let (met, total) = checkpoint_stats(cp, responses);
            (cp, met, total)
        })
        .collect()
}

/// The milestone surface for the chart feature area: a foldable per-period
/// completion card linking to the detail page. The interactive checklist is NOT
/// here.
async fn milestone_summary_card(pool: &sqlx::PgPool, s: &Subject) -> AppResult<Markup> {
    let tracker = tracker_for(s);
    let responses = load_responses(pool, s.id).await?;
    let per_period = per_period_stats(&responses);
    Ok(views::milestones::summary_card(s, tracker, &per_period))
}

/// Render the `#subject-features` area: every enabled feature's surface + the
/// "Add feature" picker. Shared by the chart page and the add/remove handlers.
pub async fn render_feature_area(pool: &sqlx::PgPool, s: &Subject) -> AppResult<Markup> {
    let enabled = feature_registry::enabled_keys(pool, s.id).await?;
    let mut surfaces: Vec<Markup> = Vec::new();
    for key in &enabled {
        match key.as_str() {
            "milestones" => surfaces.push(milestone_summary_card(pool, s).await?),
            "growth" => surfaces.push(crate::views::growth::feature_card(s)),
            _ => {}
        }
    }
    let available = feature_registry::available_to_add(pool, s.id).await?;
    Ok(views::milestones::feature_area(s.id, surfaces, &available))
}

/// `GET /subjects/{id}/milestones` — the dedicated milestone detail page (the
/// interactive checklist). Feature-gated surfaces are absent from the chart, but
/// the page itself renders whatever the subject's DOB implies (a "set DOB" notice
/// if none), so a direct link/bookmark still works.
pub async fn detail(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = load_subject(&state.pool, id).await?;
    let tracker = tracker_for(&s);
    let inner = match tracker {
        Some(t) => {
            let responses = load_responses(&state.pool, id).await?;
            views::milestones::checklist(id, t.checkpoint, Some(t.checkpoint), &responses)
        }
        None => views::milestones::needs_dob(&s),
    };
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(views::milestones::detail_page(&nav, &s, tracker, inner))
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

/// Upsert one milestone response, capturing the point-in-time age snapshot
/// (basis + chronological age in days) per the queryability spec. `note` is not
/// touched on update (there's no note UI; the column stays for future/API use).
async fn upsert_response(
    pool: &sqlx::PgPool,
    s: &Subject,
    m: milestones::Milestone,
    response: &str,
    observed_on: Option<time::Date>,
) -> AppResult<()> {
    // Marking requires a DOB — the age snapshot is part of the record.
    let dob = s
        .dob
        .ok_or_else(|| AppError::BadRequest("set date of birth before marking milestones".into()))?;
    let today = peds::today();
    let tracker = milestone_age::tracker_age(dob, s.gestational_age_weeks, today);
    let chronological_age_days = (today.to_julian_day() - dob.to_julian_day()).max(0);

    sqlx::query(
        "insert into milestone_responses
           (id, subject_id, milestone_key, domain, expected_age_months, response,
            observed_on, age_basis_used, chronological_age_days, answered_at)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9, now())
         on conflict (subject_id, milestone_key) do update set
            response = excluded.response,
            domain = excluded.domain,
            expected_age_months = excluded.expected_age_months,
            observed_on = excluded.observed_on,
            age_basis_used = excluded.age_basis_used,
            chronological_age_days = excluded.chronological_age_days,
            answered_at = now()",
    )
    .bind(Uuid::now_v7())
    .bind(s.id)
    .bind(m.key)
    .bind(m.domain)
    .bind(m.checkpoint_months as i16)
    .bind(response)
    .bind(observed_on)
    .bind(tracker.basis.as_str())
    .bind(chronological_age_days)
    .execute(pool)
    .await?;
    Ok(())
}

/// `POST /subjects/{id}/milestones/mark/{key}/{response}?checkpoint=N` — record a
/// yes/not-yet/no answer (the answer is in the URL, so htmx's `new FormData(form)`
/// submit-button omission can't bite), then swap the checklist back in. For "yes"
/// the observed date is preserved if already set, else defaults to today; cleared
/// for not-yet / no.
pub async fn mark(
    State(state): State<AppState>,
    Path((id, key, response)): Path<(Uuid, String, String)>,
    Query(q): Query<ChecklistQuery>,
) -> AppResult<Markup> {
    if !milestones::RESPONSES.contains(&response.as_str()) {
        return Err(AppError::BadRequest(format!("invalid response: {response}")));
    }
    let m = milestones::by_key(&key)
        .ok_or_else(|| AppError::BadRequest(format!("unknown milestone: {key}")))?;
    let s = load_subject(&state.pool, id).await?;

    let observed_on = if response == "yes" {
        // Preserve an existing observed date; otherwise default to the latest date
        // in this milestone's age period (today for the current period).
        let existing: Option<time::Date> = sqlx::query_scalar(
            "select observed_on from milestone_responses where subject_id = $1 and milestone_key = $2",
        )
        .bind(id)
        .bind(&key)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
        Some(existing.unwrap_or_else(|| default_observed_date(&s, m.checkpoint_months)))
    } else {
        None
    };

    upsert_response(&state.pool, &s, m, &response, observed_on).await?;

    let (checkpoint, computed) = resolve_checkpoint(&s, q.checkpoint)?;
    let responses = load_responses(&state.pool, id).await?;
    Ok(views::milestones::checklist(id, checkpoint, computed, &responses))
}

#[derive(Debug, Deserialize)]
pub struct ObservedForm {
    #[serde(default)]
    pub observed_on: String,
}

/// `POST /subjects/{id}/milestones/observed/{key}?checkpoint=N` — set/edit the
/// observed-on date for a milestone (implies "yes"). Fired by the date input's
/// change, which sends its own `observed_on` value. Blank → today.
pub async fn set_observed(
    State(state): State<AppState>,
    Path((id, key)): Path<(Uuid, String)>,
    Query(q): Query<ChecklistQuery>,
    Form(f): Form<ObservedForm>,
) -> AppResult<Markup> {
    let m = milestones::by_key(&key)
        .ok_or_else(|| AppError::BadRequest(format!("unknown milestone: {key}")))?;
    let s = load_subject(&state.pool, id).await?;
    let observed_on = parse_date(&f.observed_on)
        .map_err(AppError::BadRequest)?
        .unwrap_or_else(|| default_observed_date(&s, m.checkpoint_months));

    upsert_response(&state.pool, &s, m, "yes", Some(observed_on)).await?;

    let (checkpoint, computed) = resolve_checkpoint(&s, q.checkpoint)?;
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
