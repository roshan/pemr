//! `/api/v1/incidents` — read + idempotent-upsert write.
//!
//! Unlike every other write endpoint, incidents do **not** use
//! `provenance_conflict`: the provenance 5-tuple was dropped from this table in
//! migration `0003` because an incident is a real-world *event*, not a digital
//! artifact (its records carry the provenance instead). So the upsert keys on
//! content — `(subject_id, lower(title), occurred_at)` — which is the same key
//! `importer.rs` dedups on, so an API POST and a C-CDA/EHI import of the same
//! event converge on one row instead of racing to two.
//!
//! Two consequences of a content key worth knowing as a caller: the title match
//! is case-insensitive but the *stored* title is overwritten with whatever the
//! latest POST sent (last-write-wins on casing), and two genuinely distinct
//! same-day events with the same title collapse into one row — give them
//! distinguishing titles, or create the second one in the UI.

use axum::extract::{State};
use axum::response::Json;
use serde::{Deserialize, Serialize};
use time::Date;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, validate_in, write_err};
use crate::models::{DATE_PRECISIONS, Incident, Record, parse_subject_filter};

/// Every column of `incidents`, in `Incident`'s field order — the same list the
/// `select`s above use, so the read and write shapes stay identical. (`*` would
/// also work: the generated `search_tsv` is simply never decoded, since sqlx's
/// `FromRow` reads only the columns the struct names. Verified, not assumed.)
const COLS: &str = "id, subject_id, title, narrative, occurred_at, occurred_precision,
                    ended_at, ended_precision, created_at, updated_at";

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub subject: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Vec<Incident>>> {
    let subject = parse_subject_filter(q.subject.as_deref())
        .map_err(ApiError::bad_request)?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                ended_at, ended_precision, created_at, updated_at
           from incidents
          where ($1::uuid is null or subject_id = $1)
          order by occurred_at desc nulls last, created_at desc
          limit $2 offset $3",
    )
    .bind(subject)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize)]
pub struct IncidentDetail {
    #[serde(flatten)]
    pub incident: Incident,
    pub records: Vec<Record>,
}

pub async fn detail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<IncidentDetail>> {
    let incident = sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                ended_at, ended_precision, created_at, updated_at
           from incidents where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(ApiError::not_found)?;

    let records = sqlx::query_as::<_, Record>(
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
          order by r.occurred_at desc nulls last, r.created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(IncidentDetail { incident, records }))
}

/// Create body. Only `subject_id` + `title` are required. Every optional field
/// is `Option` rather than defaulted so that on an upsert-match we can tell
/// "omitted" (keep what's there) from "explicitly set" (overwrite) — re-POSTing
/// a partial body enriches an event instead of blanking the fields it left out.
#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub narrative: Option<String>,
    /// Start of the event. `Date` serializes as ISO 8601 (`2026-07-19`).
    #[serde(default)]
    pub occurred_at: Option<Date>,
    #[serde(default)]
    pub occurred_precision: Option<String>,
    /// End of a multi-day event; null = point-in-time (migration `0011`).
    #[serde(default)]
    pub ended_at: Option<Date>,
    #[serde(default)]
    pub ended_precision: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<Incident>> {
    let title = c.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::bad_request("title required"));
    }
    if let Some(p) = &c.occurred_precision {
        validate_in("occurred_precision", p, DATE_PRECISIONS)?;
    }
    if let Some(p) = &c.ended_precision {
        validate_in("ended_precision", p, DATE_PRECISIONS)?;
    }
    // An end with no start has nothing to anchor the span to, and an end before
    // its start is a data-entry error rather than a span — reject both here so
    // they surface as a 400 instead of landing as a nonsensical timeline row.
    if c.ended_at.is_some() && c.occurred_at.is_none() {
        return Err(ApiError::bad_request("ended_at requires occurred_at"));
    }
    if let (Some(start), Some(end)) = (c.occurred_at, c.ended_at)
        && end < start
    {
        return Err(ApiError::bad_request("ended_at is before occurred_at"));
    }

    // Content dedup (see module docs) — the same event re-POSTed updates in
    // place. `is not distinct from` so two undated events with the same title
    // match each other (plain `=` is null-false and would insert a duplicate).
    let existing: Option<Uuid> = sqlx::query_scalar(
        "select id from incidents
          where subject_id = $1 and lower(title) = lower($2)
            and occurred_at is not distinct from $3
          limit 1",
    )
    .bind(c.subject_id)
    .bind(&title)
    .bind(c.occurred_at)
    .fetch_optional(&state.pool)
    .await?;

    let row = match existing {
        // `coalesce` on every optional field: an omitted field is null, which
        // keeps the stored value. An explicit `""`/value still overwrites.
        Some(id) => sqlx::query_as::<_, Incident>(&format!(
            "update incidents
                set title = $2,
                    narrative = coalesce($3, narrative),
                    occurred_precision = coalesce($4, occurred_precision),
                    ended_at = coalesce($5, ended_at),
                    ended_precision = coalesce($6, ended_precision),
                    updated_at = now()
              where id = $1
          returning {COLS}"
        ))
        .bind(id)
        .bind(&title)
        .bind(&c.narrative)
        .bind(&c.occurred_precision)
        .bind(c.ended_at)
        .bind(&c.ended_precision)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?,

        None => sqlx::query_as::<_, Incident>(&format!(
            "insert into incidents
                 (id, subject_id, title, narrative, occurred_at,
                  occurred_precision, ended_at, ended_precision)
             values ($1, $2, $3, coalesce($4, ''), $5,
                     coalesce($6, 'day'), $7, coalesce($8, 'day'))
          returning {COLS}"
        ))
        .bind(Uuid::now_v7())
        .bind(c.subject_id)
        .bind(&title)
        .bind(&c.narrative)
        .bind(c.occurred_at)
        .bind(&c.occurred_precision)
        .bind(c.ended_at)
        .bind(&c.ended_precision)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?,
    };
    Ok(Json(row))
}
