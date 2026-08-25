//! Cross-source clinical dedup (PEMR-48).
//!
//! Today's only dedup is the same-source `(source_id, external_id)` upsert, so
//! two sources carrying the same clinical event (CAIR + EHI each list the same
//! vaccine shot; a portal + an EHI export each carry the same growth measurement)
//! land as two rows. This module is the **app-level runtime guard** — the same
//! convention as conditions (`src/importer.rs`, dedup by code) and incidents
//! (dedup by content) — NOT a DB constraint: a `SELECT id ... LIMIT 1` on the
//! natural key, then an in-place update that keeps the richest fields, else the
//! caller falls through to its normal provenance upsert. No unique index, so it
//! never blocks a migration on already-dirty data.
//!
//! Keying:
//! - **Immunizations**: `(subject_id, occurred_at, vaccine concept)`. The concept
//!   is the [`crate::peds::vaccine_families`] set — a source-independent
//!   clinical family (CVX codes OR display-name keywords). A raw `(code, date)`
//!   key can't work cross-source: CAIR carries no code, EHI carries NDC codes,
//!   FHIR carries CVX — so the numeric code is *vocabulary-specific*. Family-set
//!   overlap also lets one combination row (EHI "DTAP/HIB/IPV") absorb a source
//!   that lists the same shot per-disease-group (CAIR), collapsing back to one
//!   physical row. Unclassifiable vaccines (influenza, COVID) fall back to an
//!   exact lowercase-name match on the same date.
//! - **Observations**: `(subject_id, code, effective_on)` when coded (LOINC
//!   travels across sources — the growth-vitals case), else
//!   `(subject_id, lower(display), effective_on)`.
//!
//! Every import path (FHIR, C-CDA/EHI, CAIR sync) calls these before inserting;
//! a shared helper keeps the guard identical across executors (`&PgPool` for the
//! FHIR/CAIR paths, `&mut PgConnection` inside the C-CDA/EHI transaction).

use sqlx::{Executor, Postgres, Row};
use time::Date;
use uuid::Uuid;

use crate::peds;

/// Find an existing immunization that is the same physical shot as
/// `(subject, vaccine, code, occurred_at)`: same subject + date whose vaccine
/// family set overlaps the incoming row's — OR, for unclassifiable vaccines,
/// the same exact vaccine name on the same date. Returns the surviving row's id.
pub async fn find_immunization_match<'c, E>(
    ex: E,
    subject_id: Uuid,
    vaccine: &str,
    code: Option<&str>,
    occurred_at: Option<Date>,
) -> Result<Option<Uuid>, sqlx::Error>
where
    E: Executor<'c, Database = Postgres>,
{
    let Some(date) = occurred_at else {
        return Ok(None);
    };
    let in_families = peds::vaccine_families(code, vaccine);
    if in_families.is_empty() {
        // Unclassifiable — fall back to an exact name match on the same date.
        return sqlx::query_scalar(
            "select id from immunizations
              where subject_id = $1 and occurred_at = $2
                and lower(vaccine) = lower($3)
              limit 1",
        )
        .bind(subject_id)
        .bind(date)
        .bind(vaccine)
        .fetch_optional(ex)
        .await;
    }
    // Family overlap: fetch every row on the subject+date, classify each, and
    // return the first whose family set intersects the incoming row's.
    let rows = sqlx::query(
        "select id, code, vaccine from immunizations
          where subject_id = $1 and occurred_at = $2",
    )
    .bind(subject_id)
    .bind(date)
    .fetch_all(ex)
    .await?;
    let mut matched: Option<Uuid> = None;
    for row in rows {
        let id: Uuid = row.get(0);
        let existing_code: Option<String> = row.get(1);
        let existing_vaccine: String = row.get(2);
        let fams =
            peds::vaccine_families(existing_code.as_deref(), &existing_vaccine);
        if fams.iter().any(|f| in_families.contains(f)) {
            matched = Some(id);
            break;
        }
    }
    Ok(matched)
}

/// Find an existing observation that is the same clinical event: same subject +
/// effective date, matched by LOINC code when coded else by lowercase display.
pub async fn find_observation_match<'c, E>(
    ex: E,
    subject_id: Uuid,
    code: Option<&str>,
    display: &str,
    effective_on: Date,
) -> Result<Option<Uuid>, sqlx::Error>
where
    E: Executor<'c, Database = Postgres>,
{
    match code {
        Some(code) => sqlx::query_scalar(
            "select id from observations
              where subject_id = $1 and effective_on = $2 and lower(code) = lower($3)
              limit 1",
        )
        .bind(subject_id)
        .bind(effective_on)
        .bind(code)
        .fetch_optional(ex)
        .await,
        None => sqlx::query_scalar(
            "select id from observations
              where subject_id = $1 and effective_on = $2 and lower(display) = lower($3)
              limit 1",
        )
        .bind(subject_id)
        .bind(effective_on)
        .bind(display)
        .fetch_optional(ex)
        .await,
    }
}

/// Merge an incoming immunization into an existing row in place, keeping the
/// richest fields: every incoming non-null value fills a gap but never wipes
/// data the surviving row already carries. Blank never erases a real value.
pub async fn merge_immunization<'c, E>(
    ex: E,
    id: Uuid,
    vaccine: &str,
    code: Option<&str>,
    code_system: Option<&str>,
    occurred_at: Option<Date>,
    dose_number: Option<i32>,
    lot_number: Option<&str>,
    site: Option<&str>,
    route: Option<&str>,
) -> Result<(), sqlx::Error>
where
    E: Executor<'c, Database = Postgres>,
{
    sqlx::query(
        "update immunizations set
             vaccine     = coalesce($2, vaccine),
             code        = coalesce($3, code),
             code_system = coalesce($4, code_system),
             occurred_at = coalesce($5, occurred_at),
             dose_number = coalesce($6, dose_number),
             lot_number  = coalesce($7, lot_number),
             site        = coalesce($8, site),
             route       = coalesce($9, route),
             updated_at  = now()
           where id = $1",
    )
    .bind(id)
    .bind(vaccine)
    .bind(code)
    .bind(code_system)
    .bind(occurred_at)
    .bind(dose_number)
    .bind(lot_number)
    .bind(site)
    .bind(route)
    .execute(ex)
    .await?;
    Ok(())
}

/// Merge an incoming observation into an existing row in place, keeping the
/// richest fields (a real numeric value, unit, reference range, abnormal flag,
/// a vocabulary-specific code). Incoming non-null values fill gaps, never wipe.
pub async fn merge_observation<'c, E>(
    ex: E,
    id: Uuid,
    category: &str,
    code: Option<&str>,
    code_system: Option<&str>,
    display: &str,
    value_num: Option<f64>,
    value_text: Option<&str>,
    unit: Option<&str>,
    ref_low: Option<f64>,
    ref_high: Option<f64>,
    abnormal_flag: Option<&str>,
    panel_id: Option<&Uuid>,
    effective_on: Date,
) -> Result<(), sqlx::Error>
where
    E: Executor<'c, Database = Postgres>,
{
    sqlx::query(
        "update observations set
             category      = coalesce($2, category),
             code          = coalesce($3, code),
             code_system   = coalesce($4, code_system),
             display       = coalesce($5, display),
             value_num     = coalesce($6, value_num),
             value_text    = coalesce($7, value_text),
             unit          = coalesce($8, unit),
             ref_low       = coalesce($9, ref_low),
             ref_high      = coalesce($10, ref_high),
             abnormal_flag = coalesce($11, abnormal_flag),
             panel_id      = coalesce($12, panel_id),
             effective_on  = $13,
             updated_at    = now()
           where id = $1",
    )
    .bind(id)
    .bind(category)
    .bind(code)
    .bind(code_system)
    .bind(display)
    .bind(value_num)
    .bind(value_text)
    .bind(unit)
    .bind(ref_low)
    .bind(ref_high)
    .bind(abnormal_flag)
    .bind(panel_id)
    .bind(effective_on)
    .execute(ex)
    .await?;
    Ok(())
}
