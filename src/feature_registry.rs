//! Per-subject opt-in feature modules (PEMR-45). A subject starts clean; the user
//! adds a module from the chart and its surface pops in. The **catalogue** of
//! available features is this small in-code registry (a descriptor list, NOT a
//! plugin runtime — same inversion-of-control as `subject_modules` /
//! `subject_pages`); **enablement** is stored per-subject in the `subject_features`
//! table (migration 0018). Removing a feature deletes its enablement row (hides
//! the surface) but never touches the module's underlying data — disable, never
//! delete.
//!
//! A feature's surface may be an INLINE section on the chart (milestones) or a
//! nav button + subpage (growth, PEMR-47). `Surface` says which; gating applies
//! to whichever it is.
//!
//! Some descriptor fields + helpers here (`description`, `surface`, `is_enabled`)
//! are part of the registry's forward-looking surface — used by future modules
//! and callers, so `dead_code` is allowed module-wide (mirrors `components.rs`).
#![allow(dead_code)]

use sqlx::PgPool;
use uuid::Uuid;

/// How a feature shows up once enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Renders as an inline section in the chart's feature area.
    Inline,
    /// Adds a secondary nav button on the subject chart + a subpage under
    /// `/subjects/{id}/` (e.g. growth's "Growth charts").
    Nav,
}

/// A registry entry: what the feature is + how it surfaces.
pub struct FeatureDef {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub surface: Surface,
}

/// The ordered catalogue. Order == display order in the "Add feature" picker.
pub const FEATURES: &[FeatureDef] = &[
    FeatureDef {
        key: "milestones",
        label: "Child milestones tracking (CDC)",
        description: "Track developmental milestones from the CDC \u{201c}Learn the Signs. Act \
                      Early.\u{201d} checklists (2 months\u{2013}5 years), with silent \
                      corrected-age handling for preterm birth.",
        surface: Surface::Inline,
    },
    FeatureDef {
        key: "growth",
        label: "Growth charts (weight/height/head circumference)",
        description: "Pediatric growth charts: weight, length/height and \
                      head-circumference percentile bands vs. WHO/CDC standards \
                      (PEMR-47). Per-subject — only shows for subjects it is \
                      enabled on.",
        surface: Surface::Nav,
    },
];

pub fn by_key(key: &str) -> Option<&'static FeatureDef> {
    FEATURES.iter().find(|f| f.key == key)
}

/// The feature keys currently enabled for a subject, in registry order.
pub async fn enabled_keys(pool: &PgPool, subject_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<String> =
        sqlx::query_scalar("select feature_key from subject_features where subject_id = $1")
            .bind(subject_id)
            .fetch_all(pool)
            .await?;
    // Return in registry order so the surfaces render deterministically.
    Ok(FEATURES
        .iter()
        .map(|f| f.key.to_string())
        .filter(|k| rows.iter().any(|r| r == k))
        .collect())
}

pub async fn is_enabled(pool: &PgPool, subject_id: Uuid, key: &str) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from subject_features where subject_id = $1 and feature_key = $2",
    )
    .bind(subject_id)
    .bind(key)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

/// Enable a feature for a subject (idempotent). Ignores unknown keys at the DB
/// level via the caller's validation; here we upsert.
pub async fn enable(pool: &PgPool, subject_id: Uuid, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "insert into subject_features (id, subject_id, feature_key)
         values ($1, $2, $3)
         on conflict (subject_id, feature_key) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(subject_id)
    .bind(key)
    .execute(pool)
    .await?;
    Ok(())
}

/// Disable a feature (delete the enablement row). Underlying data is untouched.
pub async fn disable(pool: &PgPool, subject_id: Uuid, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query("delete from subject_features where subject_id = $1 and feature_key = $2")
        .bind(subject_id)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

/// Registry features NOT yet enabled for the subject — what the "Add feature"
/// picker offers.
pub async fn available_to_add(
    pool: &PgPool,
    subject_id: Uuid,
) -> Result<Vec<&'static FeatureDef>, sqlx::Error> {
    let enabled = enabled_keys(pool, subject_id).await?;
    Ok(FEATURES
        .iter()
        .filter(|f| !enabled.iter().any(|k| k == f.key))
        .collect())
}
