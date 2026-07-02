use axum::Json;
use serde_json::{Value, json};

use crate::api_auth::ApiKeyContext;
use crate::models;

/// `GET /api/v1` — discovery doc for the assistant agent.
pub async fn index(_ctx: ApiKeyContext) -> Json<Value> {
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
        "endpoints": [
            {"method": "GET",  "path": "/api/v1",                          "summary": "this discovery document"},
            {"method": "GET",  "path": "/api/v1/me",                       "summary": "which API key this request is authenticated as"},
            {"method": "GET",  "path": "/api/v1/subjects",                 "summary": "list subjects (people)"},
            {"method": "GET",  "path": "/api/v1/subjects/{id}",            "summary": "one subject"},
            {"method": "GET",  "path": "/api/v1/incidents",               "summary": "list incidents (?subject=)"},
            {"method": "GET",  "path": "/api/v1/incidents/{id}",          "summary": "one incident + linked records"},
            {"method": "GET",  "path": "/api/v1/records",                 "summary": "list records (?subject=&kind=)"},
            {"method": "GET",  "path": "/api/v1/records/{id}",            "summary": "one record (metadata)"},
            {"method": "GET",  "path": "/api/v1/records/{id}/file",       "summary": "stream the original file bytes"},
            {"method": "GET",  "path": "/api/v1/records/{id}/preview",    "summary": "stream the preview (DICOM → PNG)"},
            {"method": "GET",  "path": "/api/v1/records/{id}/thumbnail",  "summary": "stream the thumbnail (webp)"},
            {"method": "GET",  "path": "/api/v1/sources",                 "summary": "list sources (EMR portals, clinics)"},
            {"method": "POST", "path": "/api/v1/sources",                 "summary": "create/update a source (clinic, portal); dedup on name"},
            {"method": "GET",  "path": "/api/v1/sources/{id}",            "summary": "one source"},
            {"method": "GET",  "path": "/api/v1/providers",               "summary": "list clinicians (reference data; not subject-scoped)"},
            {"method": "POST", "path": "/api/v1/providers",               "summary": "upsert a clinician; dedup on npi, else (source_id, external_id)"},
            {"method": "GET",  "path": "/api/v1/providers/{id}",          "summary": "one provider"},
            {"method": "GET",  "path": "/api/v1/appointments",            "summary": "list appointments (?subject=)"},
            {"method": "POST", "path": "/api/v1/appointments",            "summary": "upsert an appointment (req: subject_id, title, starts_at)"},
            {"method": "GET",  "path": "/api/v1/appointments/{id}",       "summary": "one appointment"},
            {"method": "GET",  "path": "/api/v1/allergies",               "summary": "list allergies (?subject=)"},
            {"method": "POST", "path": "/api/v1/allergies",               "summary": "upsert an allergy (req: subject_id, substance)"},
            {"method": "GET",  "path": "/api/v1/allergies/{id}",          "summary": "one allergy"},
            {"method": "GET",  "path": "/api/v1/medications",             "summary": "list medications (?subject=)"},
            {"method": "POST", "path": "/api/v1/medications",             "summary": "upsert a medication (req: subject_id, name)"},
            {"method": "GET",  "path": "/api/v1/medications/{id}",        "summary": "one medication"},
            {"method": "GET",  "path": "/api/v1/conditions",              "summary": "list conditions / problem list (?subject=)"},
            {"method": "POST", "path": "/api/v1/conditions",              "summary": "upsert a condition (req: subject_id, name)"},
            {"method": "GET",  "path": "/api/v1/conditions/{id}",         "summary": "one condition"},
            {"method": "GET",  "path": "/api/v1/immunizations",           "summary": "list immunizations (?subject=)"},
            {"method": "POST", "path": "/api/v1/immunizations",           "summary": "upsert an immunization (req: subject_id, vaccine)"},
            {"method": "GET",  "path": "/api/v1/immunizations/{id}",      "summary": "one immunization"},
            {"method": "GET",  "path": "/api/v1/observations",            "summary": "list vitals + labs (?subject=&code=<LOINC>)"},
            {"method": "POST", "path": "/api/v1/observations",            "summary": "upsert an observation (req: subject_id, display, effective_on); BP = two rows; growth vitals use canonical LOINC"},
            {"method": "GET",  "path": "/api/v1/observations/{id}",       "summary": "one observation"},
            {"method": "GET",  "path": "/api/v1/care-reminders",          "summary": "list care reminders / what's due (?subject=)"},
            {"method": "POST", "path": "/api/v1/care-reminders",          "summary": "create a care reminder (req: subject_id, title)"},
            {"method": "GET",  "path": "/api/v1/care-reminders/{id}",     "summary": "one care reminder"},
            {"method": "GET",  "path": "/api/v1/subject-identifiers",     "summary": "list cross-system identifiers / MRNs (?subject=)"},
            {"method": "POST", "path": "/api/v1/subject-identifiers",     "summary": "upsert an identifier (req: subject_id, source_id, value)"},
            {"method": "GET",  "path": "/api/v1/subject-identifiers/{id}", "summary": "one identifier"},
            {"method": "GET",  "path": "/api/v1/subject-providers",       "summary": "list care-team links (?subject=)"},
            {"method": "POST", "path": "/api/v1/subject-providers",       "summary": "upsert a care-team link (req: subject_id, provider_id)"},
            {"method": "GET",  "path": "/api/v1/subject-relationships",   "summary": "list family graph edges (?subject=)"},
            {"method": "POST", "path": "/api/v1/subject-relationships",   "summary": "upsert a relationship (req: subject_id, related_subject_id, relationship)"},
            {"method": "GET",  "path": "/api/v1/insurance-plans",         "summary": "list insurance cards/policies (reference data; not subject-scoped)"},
            {"method": "POST", "path": "/api/v1/insurance-plans",         "summary": "upsert an insurance plan (req: payer_name); dedup on (source_id, external_id)"},
            {"method": "GET",  "path": "/api/v1/insurance-plans/{id}",    "summary": "one insurance plan"},
            {"method": "GET",  "path": "/api/v1/subject-insurance",       "summary": "list coverage links / who's on which card (?subject=)"},
            {"method": "POST", "path": "/api/v1/subject-insurance",       "summary": "upsert a coverage link (req: subject_id, plan_id)"},
            {"method": "GET",  "path": "/api/v1/search?q=...&subject=...", "summary": "full-text search across incidents + records"},
            {"method": "POST", "path": "/api/v1/import/fhir?subject=<uuid>&source=<name>", "summary": "import a FHIR R4 Bundle (Apple Health clinical records / Epic FHIR export); idempotent on resource id"},
            {"method": "POST", "path": "/api/v1/import/ccda?subject=<uuid>&source=<name>", "summary": "import a C-CDA XML document (MyChart 'Download My Record'); body is the raw XML"},
        ],
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
