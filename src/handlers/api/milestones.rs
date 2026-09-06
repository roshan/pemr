//! `/api/v1` surface for the CDC LTSAE developmental-milestone tracker
//! (PEMR-57) — the read/write surface PEMR-35 deferred.
//!
//! Three endpoints: the canonical **catalogue** (the vendored CDC dataset), a
//! subject's **tracker + responses**, and **mark** (one milestone). Every one
//! delegates to the same code the UI uses (`handlers::milestones`) — there is
//! exactly one implementation of the age basis, the observed-on default and the
//! upsert, and this module must never grow a second.
//!
//! Two rules this surface exists to enforce:
//!
//! * **The catalogue is CDC text, full stop.** There is no way to POST a
//!   milestone that isn't in the vendored dataset — an unknown key is a 400.
//!   (On 2026-09-06 an agent that found no milestone resource invented its own
//!   30-item list and wrote it in as `observations`; that is what this closes.)
//! * **Writes require the subject's opt-in.** Marking against a subject who
//!   doesn't have the `milestones` feature enabled is a **409** naming the UI
//!   toggle, not a silent write into a chart nobody can see. Reads are the
//!   opposite: they return `feature_enabled: false` with an empty list, because
//!   "not enabled" is a fact about the subject, not an error.

use axum::extract::State;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, write_err};
use crate::models::{MilestoneResponse, parse_date};
use crate::{feature_registry, milestone_age, milestones};

const FEATURE_KEY: &str = "milestones";

fn milestone_json(m: &milestones::Milestone) -> Value {
    json!({
        "key": m.key,
        "checkpoint_months": m.checkpoint_months,
        "domain": m.domain,
        "text": m.text,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct CatalogueQuery {
    pub checkpoint: Option<i32>,
    pub domain: Option<String>,
}

/// `GET /api/v1/milestones` — the canonical CDC "Learn the Signs. Act Early."
/// 2022 catalogue (159 milestones × 12 checkpoints × 4 domains), optionally
/// narrowed by `?checkpoint=` / `?domain=`. Static: it comes from the vendored
/// TSV, not the database. The vocabularies and the disclaimer ride along so a
/// caller needs exactly one request to know what it may send.
pub async fn catalogue(
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<CatalogueQuery>,
) -> ApiResult<Json<Value>> {
    if let Some(cp) = q.checkpoint
        && !milestones::CHECKPOINTS.contains(&cp)
    {
        return Err(ApiError::bad_request(format!(
            "unknown checkpoint: {cp} (valid: {:?})",
            milestones::CHECKPOINTS
        )));
    }
    let domain = q.domain.as_deref().map(str::trim).filter(|d| !d.is_empty());
    if let Some(d) = domain
        && !milestones::DOMAINS.iter().any(|(k, _)| *k == d)
    {
        return Err(ApiError::bad_request(format!("unknown domain: {d}")));
    }

    let items: Vec<Value> = milestones::CHECKPOINTS
        .iter()
        .filter(|cp| q.checkpoint.is_none_or(|want| want == **cp))
        .flat_map(|cp| milestones::by_checkpoint(*cp))
        .filter(|m| domain.is_none_or(|d| d == m.domain))
        .map(|m| milestone_json(&m))
        .collect();

    Ok(Json(json!({
        "source": "CDC \"Learn the Signs. Act Early.\" milestone checklists, 2022 revision \
                   (cdc.gov/act-early/milestones) — US federal, public domain. Vendored; \
                   the API accepts no other milestone vocabulary.",
        "count": items.len(),
        "milestones": items,
        "checkpoints": milestones::CHECKPOINTS,
        "domains": milestones::DOMAINS
            .iter()
            .map(|(key, label)| json!({"key": key, "label": label}))
            .collect::<Vec<_>>(),
        "responses": milestones::RESPONSES,
        "disclaimer": milestones::DISCLAIMER,
    })))
}

/// The subject's tracker block: computed age, basis, current checkpoint. `null`
/// when they have no DOB — the checklist can't be placed without one, and
/// marking is rejected for the same reason.
fn tracker_json(s: &crate::models::Subject) -> Value {
    match crate::handlers::milestones::tracker_for(s) {
        Some(t) => json!({
            "computed_age_months": t.computed_months,
            "age_basis": t.basis.as_str(),
            "checkpoint": t.checkpoint,
            "checkpoint_label": milestone_age::fmt_months(t.checkpoint),
        }),
        None => Value::Null,
    }
}

async fn load_subject_or_404(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> ApiResult<crate::models::Subject> {
    crate::handlers::milestones::load_subject(pool, id).await.map_err(|e| match e {
        sqlx::Error::RowNotFound => ApiError::not_found(),
        other => write_err(other),
    })
}

/// `GET /api/v1/subjects/{id}/milestones` — one subject's tracker state and
/// every response recorded for them. A subject without the feature enabled is
/// `feature_enabled: false` + an empty list (200, not 404): absence of an opt-in
/// is a fact, not a failure. 404 is reserved for a subject that doesn't exist.
pub async fn subject_milestones(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Value>> {
    let s = load_subject_or_404(&state.pool, id).await?;
    let enabled = feature_registry::is_enabled(&state.pool, id, FEATURE_KEY)
        .await
        .map_err(write_err)?;
    let responses: Vec<MilestoneResponse> = if enabled {
        crate::handlers::milestones::load_responses(&state.pool, id).await.map_err(write_err)?
    } else {
        Vec::new()
    };
    Ok(Json(json!({
        "subject_id": s.id,
        "feature_enabled": enabled,
        "tracker": tracker_json(&s),
        "responses": responses,
        "disclaimer": milestones::DISCLAIMER,
    })))
}

#[derive(Debug, Deserialize)]
pub struct MarkBody {
    pub response: String,
    /// ISO date. Only meaningful with `response: "yes"`; omitted keeps an
    /// already-recorded date, else defaults to the end of that milestone's age
    /// period (`milestone_age::latest_date_in_period`), exactly as the UI does.
    #[serde(default)]
    pub observed_on: Option<String>,
}

/// `POST /api/v1/subjects/{id}/milestones/{key}` — mark one milestone. The same
/// upsert as the UI's `…/milestones/mark/{key}/{response}`, keyed
/// `(subject_id, milestone_key)`, so posting twice yields one row and the UI and
/// API can't diverge.
///
/// 400 for an unknown milestone key or an invalid response — the CDC catalogue
/// is the only accepted vocabulary. 409 when the subject hasn't got the feature
/// enabled (the message names the page that turns it on). 404 for an unknown
/// subject.
pub async fn mark(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath((id, key)): ApiPath<(Uuid, String)>,
    ApiJson(body): ApiJson<MarkBody>,
) -> ApiResult<Json<MilestoneResponse>> {
    let m = milestones::by_key(&key).ok_or_else(|| {
        ApiError::bad_request(format!(
            "unknown milestone: {key} — the catalogue at GET /api/v1/milestones is the only \
             accepted vocabulary (CDC LTSAE 2022)"
        ))
    })?;
    let response = body.response.trim();
    if !milestones::RESPONSES.contains(&response) {
        return Err(ApiError::bad_request(format!(
            "invalid response: {response} (valid: {:?})",
            milestones::RESPONSES
        )));
    }

    let s = load_subject_or_404(&state.pool, id).await?;
    if !feature_registry::is_enabled(&state.pool, id, FEATURE_KEY).await.map_err(write_err)? {
        return Err(ApiError::conflict(format!(
            "the milestones feature is not enabled for this subject; enable \
             \"Child milestones tracking (CDC)\" at /subjects/{id}/edit before writing responses"
        )));
    }

    let observed_on = if response == "yes" {
        let supplied = parse_date(body.observed_on.as_deref().unwrap_or(""))
            .map_err(ApiError::bad_request)?;
        Some(match supplied {
            Some(d) => d,
            None => crate::handlers::milestones::existing_observed(&state.pool, id, &key)
                .await
                .map_err(write_err)?
                .unwrap_or_else(|| {
                    crate::handlers::milestones::default_observed_date(&s, m.checkpoint_months)
                }),
        })
    } else {
        None
    };

    // Shares the UI's upsert: one age-basis snapshot rule, one dedup key.
    crate::handlers::milestones::upsert_response(&state.pool, &s, m, response, observed_on)
        .await
        .map_err(|e| match e {
            crate::error::AppError::BadRequest(msg) => ApiError::bad_request(msg),
            other => ApiError::internal(other.to_string()),
        })?;

    let row = sqlx::query_as::<_, MilestoneResponse>(
        "select * from milestone_responses where subject_id = $1 and milestone_key = $2",
    )
    .bind(id)
    .bind(&key)
    .fetch_one(&state.pool)
    .await
    .map_err(write_err)?;
    Ok(Json(row))
}
