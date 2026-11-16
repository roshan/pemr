//! `/api/v1/allergies` — read + idempotent-upsert write (see `api::mod`).

use axum::extract::{State};
use axum::response::Json;
use serde::Deserialize;
use serde_json::Value;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, clamp_limit, clamp_offset, provenance_conflict, validate_in, write_err};
use crate::models::{
    ALLERGY_CATEGORIES, ALLERGY_CRITICALITIES, ALLERGY_SEVERITIES, ALLERGY_STATUSES, Allergy,
    CODE_SYSTEMS, parse_subject_filter,
};

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
) -> ApiResult<Json<Vec<Allergy>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, Allergy>(
        "select * from allergies
          where ($1::uuid is null or subject_id = $1)
          order by created_at desc limit $2 offset $3",
    )
    .bind(subject)
    .bind(clamp_limit(q.limit))
    .bind(clamp_offset(q.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn detail(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Allergy>> {
    let row = sqlx::query_as::<_, Allergy>("select * from allergies where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(row))
}

/// `deny_unknown_fields`: an unrecognised key is a 400, not a silent drop.
/// Before PEMR-60 this struct lacked the reaction-code/criticality columns and
/// serde ignored them, so a POST returned 200 with those fields null.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub subject_id: Uuid,
    pub substance: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub code_system: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub reaction: Option<String>,
    #[serde(default)]
    pub reaction_code: Option<String>,
    #[serde(default)]
    pub reaction_code_system: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub criticality: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub onset_date: Option<Date>,
    #[serde(default)]
    pub noted_date: Option<Date>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub source_id: Option<Uuid>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub external_url: Option<String>,
    #[serde(default)]
    #[serde(with = "time::serde::rfc3339::option")]
    pub source_synced_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub source_payload: Option<Value>,
}

/// Field checks for `create`, split out so they're testable without a DB.
/// Returns the trimmed substance and the defaulted status.
fn validate(c: &Create) -> ApiResult<(String, String)> {
    let substance = c.substance.trim().to_string();
    if substance.is_empty() {
        return Err(ApiError::bad_request("substance required"));
    }
    let status = c.status.clone().unwrap_or_else(|| "active".into());
    validate_in("status", &status, ALLERGY_STATUSES)?;
    if let Some(cat) = &c.category {
        validate_in("category", cat, ALLERGY_CATEGORIES)?;
    }
    if let Some(sev) = &c.severity {
        validate_in("severity", sev, ALLERGY_SEVERITIES)?;
    }
    if let Some(crit) = &c.criticality {
        validate_in("criticality", crit, ALLERGY_CRITICALITIES)?;
    }
    if let Some(sys) = &c.reaction_code_system {
        validate_in("reaction_code_system", sys, CODE_SYSTEMS)?;
    }
    if c.reaction_code.is_some() != c.reaction_code_system.is_some() {
        return Err(ApiError::bad_request(
            "reaction_code and reaction_code_system must be sent together",
        ));
    }
    Ok((substance, status))
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<Allergy>> {
    let (substance, status) = validate(&c)?;
    let notes = c.notes.unwrap_or_default();
    let has_keys = c.source_id.is_some() && c.external_id.is_some();
    let set = "subject_id=excluded.subject_id, substance=excluded.substance, code=excluded.code, \
        code_system=excluded.code_system, category=excluded.category, reaction=excluded.reaction, \
        reaction_code=excluded.reaction_code, reaction_code_system=excluded.reaction_code_system, \
        severity=excluded.severity, criticality=excluded.criticality, status=excluded.status, onset_date=excluded.onset_date, \
        noted_date=excluded.noted_date, notes=excluded.notes, source_id=excluded.source_id, \
        external_id=excluded.external_id, external_url=excluded.external_url, \
        source_synced_at=excluded.source_synced_at, source_payload=excluded.source_payload";
    let sql = format!(
        "insert into allergies (id, subject_id, substance, code, code_system, category, reaction, \
            reaction_code, reaction_code_system, severity, criticality, status, onset_date, \
            noted_date, notes, source_id, external_id, external_url, source_synced_at, \
            source_payload) \
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20){} \
         returning *",
        provenance_conflict(has_keys, set),
    );
    let row = sqlx::query_as::<_, Allergy>(&sql)
        .bind(Uuid::now_v7())
        .bind(c.subject_id)
        .bind(&substance)
        .bind(c.code)
        .bind(c.code_system)
        .bind(c.category)
        .bind(c.reaction)
        .bind(c.reaction_code)
        .bind(c.reaction_code_system)
        .bind(c.severity)
        .bind(c.criticality)
        .bind(&status)
        .bind(c.onset_date)
        .bind(c.noted_date)
        .bind(&notes)
        .bind(c.source_id)
        .bind(c.external_id)
        .bind(c.external_url)
        .bind(c.source_synced_at)
        .bind(c.source_payload)
        .fetch_one(&state.pool)
        .await
        .map_err(write_err)?;
    Ok(Json(row))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: serde_json::Value) -> Result<Create, serde_json::Error> {
        serde_json::from_value(v)
    }

    const SUBJ: &str = "01970000-0000-7000-8000-000000000001";

    /// PEMR-60: these three fields used to be silently dropped.
    #[test]
    fn reaction_code_and_criticality_deserialize() {
        let c = parse(serde_json::json!({
            "subject_id": SUBJ, "substance": "Modafinil",
            "reaction_code": "126485001", "reaction_code_system": "SNOMED",
            "criticality": "low",
        }))
        .unwrap();
        assert_eq!(c.reaction_code.as_deref(), Some("126485001"));
        assert_eq!(c.reaction_code_system.as_deref(), Some("SNOMED"));
        assert_eq!(c.criticality.as_deref(), Some("low"));
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn unknown_field_is_rejected_not_dropped() {
        let e = parse(serde_json::json!({
            "subject_id": SUBJ, "substance": "X", "reaction_cod": "1",
        }))
        .unwrap_err();
        assert!(e.to_string().contains("reaction_cod"), "{e}");
    }

    #[test]
    fn bad_vocab_values_rejected() {
        let base = serde_json::json!({"subject_id": SUBJ, "substance": "X"});
        let with = |k: &str, v: &str| {
            let mut b = base.clone();
            b[k] = v.into();
            if k == "reaction_code_system" { b["reaction_code"] = "1".into(); }
            if k == "reaction_code" { b["reaction_code_system"] = "SNOMED".into(); }
            parse(b).unwrap()
        };
        assert!(validate(&with("criticality", "severe")).is_err());
        assert!(validate(&with("reaction_code_system", "snomed-ct")).is_err());
        assert!(validate(&with("criticality", "unable-to-assess")).is_ok());
        // Code without its system (or vice versa) is meaningless.
        let mut half = base.clone();
        half["reaction_code"] = "126485001".into();
        assert!(validate(&parse(half).unwrap()).is_err());
    }
}
