use axum::Json;
use serde_json::{Value, json};

use crate::api_auth::ApiKeyContext;
use crate::models;

/// `GET /api/v1` — discovery doc for the assistant agent. The endpoint rows
/// come from the `api_routes` registry — the same one `main.rs` wires the
/// routes from — so the doc can't drift from the actual surface.
pub async fn index(_ctx: ApiKeyContext) -> Json<Value> {
    let endpoints: Vec<Value> = crate::api_routes::routes()
        .iter()
        .flat_map(|r| {
            let path = r.doc_path.unwrap_or(r.path);
            r.docs
                .iter()
                .map(move |(method, summary)| {
                    json!({"method": method, "path": path, "summary": summary})
                })
                .collect::<Vec<_>>()
        })
        .collect();
    Json(json!({
        "name": "personal-emr",
        "version": "v1",
        "description": "JSON API for the personal EMR. Read (GET) across all resources; \
            write (POST) for the clinical resources, intended for an agent uploading \
            structured data parsed from records (PDFs, portal exports). POST is an \
            idempotent UPSERT keyed on (source_id, external_id): supply both to make a \
            re-upload update-in-place instead of duplicating; omit them and every POST \
            inserts. Authenticate with `Authorization: Bearer <token>` from /settings/api-keys. \
            Errors are JSON: {\"error\": \"...\"} with the right status (400 bad input, \
            401 bad token, 404 missing, 409 conflict).",
        "endpoints": endpoints,
        "provenance": "On any POST that accepts source provenance, supply source_id + \
            external_id (e.g. a stable hash of the source document + item) to get \
            idempotent upserts. external_url, source_synced_at, and source_payload (raw \
            jsonb) are also accepted. Codes ride alongside display strings: code + \
            code_system (CVX, RxNorm, LOINC, ICD-10/SNOMED, UNII).",
        "record_kinds": models::RECORD_KINDS,
        "source_kinds": models::SOURCE_KINDS,
        "vocabularies": {
            "appointment_status": models::APPOINTMENT_STATUSES,
            "subject_provider_role": models::SUBJECT_PROVIDER_ROLES,
            "subject_identifier_type": models::SUBJECT_IDENTIFIER_TYPES,
            "allergy_category": models::ALLERGY_CATEGORIES,
            "allergy_severity": models::ALLERGY_SEVERITIES,
            "allergy_criticality": models::ALLERGY_CRITICALITIES,
            "allergy_status": models::ALLERGY_STATUSES,
            "medication_status": models::MEDICATION_STATUSES,
            "condition_status": models::CONDITION_STATUSES,
            "immunization_status": models::IMMUNIZATION_STATUSES,
            "observation_category": models::OBSERVATION_CATEGORIES,
            "observation_abnormal_flag": models::OBSERVATION_ABNORMAL_FLAGS,
            "care_reminder_kind": models::CARE_REMINDER_KINDS,
            "care_reminder_status": models::CARE_REMINDER_STATUSES,
            "subject_relationship": models::SUBJECT_RELATIONSHIP_KINDS,
            "insurance_plan_type": models::INSURANCE_PLAN_TYPES,
            "insurance_plan_kind": models::INSURANCE_PLAN_KINDS,
            "insurance_relationship": models::INSURANCE_RELATIONSHIPS,
            "date_precision": models::DATE_PRECISIONS,
            "code_system": models::CODE_SYSTEMS,
        },
    }))
}
