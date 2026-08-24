use serde::Serialize;
use sqlx::FromRow;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Subject {
    pub id: Uuid,
    pub full_name: String,
    pub given_name: String,
    pub family_name: String,
    pub dob: Option<Date>,
    pub sex_at_birth: Option<String>,
    pub blood_type: Option<String>,
    pub notes: String,
    pub cf_access_email: Option<String>,
    // 0008: positive "no known allergies" assertion (distinct from "no data").
    pub no_known_allergies: bool,
    // 0018: weeks of gestation at birth (null = unknown → treated as term). Feeds
    // the silent corrected-age computation for the milestone tracker (PEMR-35).
    // `#[sqlx(default)]` so any partial SELECT still maps (comes back None).
    #[sqlx(default)]
    pub gestational_age_weeks: Option<i16>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Source {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    pub notes: String,
    // 0007: a clinic IS a source, so sources carry facility contact info.
    pub phone: Option<String>,
    pub address: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Incident {
    pub id: Uuid,
    pub subject_id: Uuid,
    pub title: String,
    pub narrative: String,
    pub occurred_at: Option<Date>,
    pub occurred_precision: String,
    /// Optional end of a multi-day event (a hospital stay, a trip). Null = a
    /// point-in-time event. `#[sqlx(default)]` so older SELECTs that don't list
    /// these columns still map (they just come back null/"day") instead of
    /// failing at runtime.
    #[sqlx(default)]
    pub ended_at: Option<Date>,
    #[sqlx(default)]
    #[serde(default)]
    pub ended_precision: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Record {
    pub id: Uuid,
    pub subject_id: Uuid,
    pub kind: String,
    pub title: String,
    pub notes: String,
    pub occurred_at: Option<Date>,
    pub occurred_precision: String,
    pub file_path: Option<String>,
    pub content_type: Option<String>,
    pub byte_size: Option<i64>,
    pub sha256: Option<String>,
    pub preview_path: Option<String>,
    pub preview_content_type: Option<String>,
    pub thumbnail_path: Option<String>,
    pub thumbnail_content_type: Option<String>,
    pub study_instance_uid: Option<String>,
    pub dicom_metadata: Option<serde_json::Value>,
    pub instance_number: Option<i32>,
    pub source_id: Option<Uuid>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub source_synced_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A per-subject feature enablement (PEMR-45). The catalogue of features is an
/// in-code registry (`feature_registry.rs`); this row records that `subject_id`
/// has turned `feature_key` on. The registry itself works with scalar queries;
/// this FromRow model exists to mirror the table (consistent with the other model
/// structs) for a future API/read consumer.
#[allow(dead_code)]
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SubjectFeature {
    pub id: Uuid,
    pub subject_id: Uuid,
    pub feature_key: String,
    pub config: Option<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub enabled_at: OffsetDateTime,
}

/// A user-entered developmental-milestone assessment (PEMR-37). No provenance
/// 5-tuple — this is hand-entered, not imported. `expected_age_months` + `domain`
/// are denormalized from the vendored dataset at write time; `age_basis_used` +
/// `chronological_age_days` are a point-in-time snapshot (see migration 0018).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct MilestoneResponse {
    pub id: Uuid,
    pub subject_id: Uuid,
    pub milestone_key: String,
    pub domain: String,
    pub expected_age_months: i16,
    pub response: String,
    pub observed_on: Option<Date>,
    pub age_basis_used: String,
    pub chronological_age_days: i32,
    pub note: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub answered_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    // Populated by sqlx::FromRow but never read directly — the hash is
    // looked up by `api_auth::middleware` with its own scalar query, and
    // we never want to leak it through serialization.
    #[allow(dead_code)]
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub token_prefix: String,
    pub owner_subject_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub revoked_at: Option<OffsetDateTime>,
}

/// One recent observation, for the subject chart's vitals panel. `value_num`
/// must be projected `::float8` (numeric → f64) by the query.
#[derive(Debug, FromRow)]
pub struct VitalRow {
    pub display: String,
    pub value_num: Option<f64>,
    pub value_text: Option<String>,
    pub unit: Option<String>,
    pub effective_on: Date,
    pub abnormal_flag: Option<String>,
}

/// A full observation row for the vitals & labs list. `value_num`/`ref_low`/
/// `ref_high` must be projected `::float8` by the query.
#[derive(Debug, FromRow)]
pub struct ObservationRow {
    pub effective_on: Date,
    pub category: String,
    pub display: String,
    pub value_num: Option<f64>,
    pub value_text: Option<String>,
    pub unit: Option<String>,
    pub ref_low: Option<f64>,
    pub ref_high: Option<f64>,
    pub abnormal_flag: Option<String>,
}

/// A care-team member joined for the subject chart (subject_providers → providers).
#[derive(Debug, FromRow)]
pub struct CareTeamMember {
    pub role: String,
    pub full_name: String,
    pub specialty: Option<String>,
}

/// A subject's insurance coverage joined for the chart (subject_insurance →
/// insurance_plans). `member_id` coalesces the per-person id over the card's.
#[derive(Debug, FromRow)]
pub struct InsuranceCoverageRow {
    pub payer_name: String,
    pub plan_name: Option<String>,
    pub plan_kind: String,
    pub relationship: String,
    pub member_id: Option<String>,
}

pub const RECORD_KINDS: &[&str] = &[
    "xray", "mri", "ct", "ultrasound", "report", "lab", "note", "prescription", "photo", "document", "other",
];

/// Kinds whose `preview` (or original `file`) is a renderable image and
/// therefore deserves an inline thumbnail / hero rendering.
pub const IMAGE_RECORD_KINDS: &[&str] =
    &["xray", "mri", "ct", "ultrasound", "photo"];

pub fn is_image_kind(k: &str) -> bool {
    IMAGE_RECORD_KINDS.contains(&k)
}

pub const SOURCE_KINDS: &[&str] = &[
    "mychart",
    "athena",
    "quest",
    "labcorp",
    "hospital",
    "clinic",
    "insurance",
    "manual",
    "other",
];

pub fn record_kind_label(k: &str) -> &'static str {
    match k {
        "xray" => "X-ray",
        "mri" => "MRI",
        "ct" => "CT",
        "ultrasound" => "Ultrasound",
        "report" => "Report",
        "lab" => "Lab",
        "note" => "Note",
        "prescription" => "Prescription",
        "photo" => "Photo",
        "document" => "Document",
        _ => "Other",
    }
}

pub fn source_kind_label(k: &str) -> &'static str {
    match k {
        "mychart" => "MyChart",
        "athena" => "Athena",
        "quest" => "Quest",
        "labcorp" => "LabCorp",
        "hospital" => "Hospital",
        "clinic" => "Clinic",
        "insurance" => "Insurance",
        "manual" => "Manual entry",
        _ => "Other",
    }
}

/// Parse `<input type="date">` into Option<Date>. An empty string is None.
pub fn parse_date(s: &str) -> Result<Option<Date>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(s, fmt).map(Some).map_err(|e| e.to_string())
}

/// Parse "all" or a uuid; returns None for "all" / empty / missing.
pub fn parse_subject_filter(s: Option<&str>) -> Result<Option<Uuid>, String> {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some("all") => Ok(None),
        Some(s) => Uuid::parse_str(s).map(Some).map_err(|e| e.to_string()),
    }
}

pub fn empty_to_none(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

// Forward-declared schema types + vocabularies for migrations 0007–0009. They
// are defined ahead of their consumers (PEMR-17/18 handlers+views+API, and the
// PEMR-3/19 follow-ons), so `dead_code` is allowed for this module until those
// land. Re-exported flat so the path stays `models::Provider`, `models::…`.
#[allow(unused_imports)] // consumers land in PEMR-17/18/3/19
pub use clinical_model::*;

#[allow(dead_code)]
mod clinical_model {
    use serde::Serialize;
    use sqlx::FromRow;
    use time::{Date, OffsetDateTime};
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Clinical data model (migrations 0007–0009). Design-of-record:
    // docs/data-model-plan.md, KB:PEMR:data-model. These structs exist so the
    // migrations have matching Rust types; handlers/API that query them land in
    // PEMR-17 / PEMR-18 / PEMR-3 / PEMR-19.
    //
    // `numeric` columns (observations.value_num / ref_low / ref_high) are typed
    // `f64` here. sqlx is built WITHOUT a decimal feature (bigdecimal/
    // rust_decimal) by supply-chain choice, so a query feeding these structs
    // MUST cast the numeric columns to float8, e.g.
    // `select ..., value_num::float8 as value_num`.
    // -----------------------------------------------------------------------

    /// Phase 1 — shared clinician directory. Reference data: NO subject_id.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct Provider {
        pub id: Uuid,
        pub full_name: String,
        pub specialty: Option<String>,
        pub npi: Option<String>,
        pub facility_id: Option<Uuid>,
        pub phone: Option<String>,
        pub email: Option<String>,
        pub notes: String,
        pub source_id: Option<Uuid>,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 1 — care-team membership ("Dr. Kelly is Astra's PCP").
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct SubjectProvider {
        pub subject_id: Uuid,
        pub provider_id: Uuid,
        pub role: String,
        pub active: bool,
        pub since: Option<Date>,
        pub notes: String,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
    }

    /// Phase 1 — cross-system identity reconciliation (the sync hook).
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct SubjectIdentifier {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub source_id: Uuid,
        pub id_type: String,
        pub value: String,
        pub notes: String,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 1 — calendar event with a status lifecycle.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct Appointment {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub provider_id: Option<Uuid>,
        pub source_id: Option<Uuid>,
        pub incident_id: Option<Uuid>,
        #[serde(with = "time::serde::rfc3339")]
        pub starts_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339::option")]
        pub ends_at: Option<OffsetDateTime>,
        pub all_day: bool,
        pub status: String,
        pub title: String,
        pub location: Option<String>,
        pub notes: String,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 2 — allergy/intolerance.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct Allergy {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub substance: String,
        pub code: Option<String>,
        pub code_system: Option<String>,
        pub category: Option<String>,
        pub reaction: Option<String>,
        pub reaction_code: Option<String>,
        pub reaction_code_system: Option<String>,
        pub criticality: Option<String>,
        pub severity: Option<String>,
        pub status: String,
        pub onset_date: Option<Date>,
        pub noted_date: Option<Date>,
        pub notes: String,
        pub source_id: Option<Uuid>,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 2 — medication.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct Medication {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub name: String,
        pub code: Option<String>,
        pub code_system: Option<String>,
        pub dose: Option<String>,
        pub route: Option<String>,
        pub frequency: Option<String>,
        pub status: String,
        pub started_on: Option<Date>,
        pub ended_on: Option<Date>,
        pub reason: Option<String>,
        pub prescriber_id: Option<Uuid>,
        pub notes: String,
        pub source_id: Option<Uuid>,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 2 — problem-list condition (distinct from incidents).
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct Condition {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub name: String,
        pub code: Option<String>,
        pub code_system: Option<String>,
        pub status: String,
        pub onset_date: Option<Date>,
        pub onset_precision: String,
        pub resolved_date: Option<Date>,
        pub severity: Option<String>,
        pub notes: String,
        pub source_id: Option<Uuid>,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 2 — immunization.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct Immunization {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub vaccine: String,
        pub code: Option<String>,
        pub code_system: Option<String>,
        pub occurred_at: Option<Date>,
        pub dose_number: Option<i32>,
        pub lot_number: Option<String>,
        pub site: Option<String>,
        pub route: Option<String>,
        pub status: String,
        pub provider_id: Option<Uuid>,
        pub appointment_id: Option<Uuid>,
        pub incident_id: Option<Uuid>,
        pub notes: String,
        pub source_id: Option<Uuid>,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 2 — vitals + discrete lab results (one table). See the
    /// numeric/float8 note above before querying `value_num`/`ref_*`.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct Observation {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub category: String,
        pub code: Option<String>,
        pub code_system: Option<String>,
        pub display: String,
        pub value_num: Option<f64>,
        pub value_text: Option<String>,
        pub unit: Option<String>,
        pub ref_low: Option<f64>,
        pub ref_high: Option<f64>,
        pub abnormal_flag: Option<String>,
        pub effective_on: Date,
        pub effective_precision: String,
        #[serde(with = "time::serde::rfc3339::option")]
        pub effective_at: Option<OffsetDateTime>,
        pub panel_id: Option<Uuid>,
        pub record_id: Option<Uuid>,
        pub appointment_id: Option<Uuid>,
        pub incident_id: Option<Uuid>,
        pub notes: String,
        pub source_id: Option<Uuid>,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 3 — "what's due" reminder. overdue is DERIVED (due_on < today).
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct CareReminder {
        pub id: Uuid,
        pub subject_id: Uuid,
        pub title: String,
        pub kind: String,
        pub due_on: Option<Date>,
        pub status: String,
        pub recommended_by: Option<Uuid>,
        pub satisfied_by_appointment_id: Option<Uuid>,
        pub notes: String,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Phase 3 — family graph / guardianship edge.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct SubjectRelationship {
        pub subject_id: Uuid,
        pub related_subject_id: Uuid,
        pub relationship: String,
        pub notes: String,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
    }

    /// An insurance card / policy. Shared reference data (a family shares one
    /// card), so NO subject_id — covered people are linked via `SubjectInsurance`.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct InsurancePlan {
        pub id: Uuid,
        pub payer_name: String,
        pub plan_name: Option<String>,
        pub plan_type: Option<String>,
        pub member_id: Option<String>,
        pub group_number: Option<String>,
        pub subscriber_name: Option<String>,
        pub plan_kind: String,
        pub rx_bin: Option<String>,
        pub rx_pcn: Option<String>,
        pub rx_group: Option<String>,
        pub payer_phone: Option<String>,
        pub effective_date: Option<Date>,
        pub expiration_date: Option<Date>,
        pub notes: String,
        pub source_id: Option<Uuid>,
        pub external_id: Option<String>,
        pub external_url: Option<String>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub source_synced_at: Option<OffsetDateTime>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// A scanned insurance card image. Bytes live under `FILES_DIR`
    /// content-addressed (same as records/DICOM); this row is the metadata.
    /// `superseded_at is null` means "this is the card in your wallet today" --
    /// the database enforces at most one current row per (plan, side).
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct InsuranceCard {
        pub id: Uuid,
        pub plan_id: Uuid,
        pub side: String,
        pub file_path: String,
        pub content_type: Option<String>,
        pub byte_size: Option<i64>,
        pub sha256: Option<String>,
        pub thumbnail_path: Option<String>,
        pub thumbnail_content_type: Option<String>,
        pub effective_date: Option<Date>,
        #[serde(with = "time::serde::rfc3339::option")]
        pub superseded_at: Option<OffsetDateTime>,
        pub notes: String,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
        #[serde(with = "time::serde::rfc3339")]
        pub updated_at: OffsetDateTime,
    }

    /// Coverage link (subject ↔ insurance plan). Mirrors `SubjectProvider`:
    /// composite PK (subject_id, plan_id), relationship + per-person id as attrs.
    #[derive(Debug, Clone, FromRow, Serialize)]
    pub struct SubjectInsurance {
        pub subject_id: Uuid,
        pub plan_id: Uuid,
        pub relationship: String,
        pub member_id: Option<String>,
        pub is_primary: bool,
        pub effective_date: Option<Date>,
        pub expiration_date: Option<Date>,
        pub notes: String,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: OffsetDateTime,
    }

    // --- Vocabularies (text + const &[&str], never pg enums) ---

    /// Date-precision values used by `*_precision` columns (onset, effective, …).
    pub const DATE_PRECISIONS: &[&str] = &["day", "month", "year"];

    /// Standard code systems that ride alongside display strings (not a
    /// terminology server — the code is just stored next to the text).
    pub const CODE_SYSTEMS: &[&str] = &["CVX", "RxNorm", "LOINC", "ICD-10", "SNOMED", "UNII"];

    // Phase 1
    pub const APPOINTMENT_STATUSES: &[&str] = &["scheduled", "completed", "cancelled", "no_show"];
    pub const SUBJECT_PROVIDER_ROLES: &[&str] =
        &["pcp", "specialist", "dentist", "therapist", "care", "other"];
    pub const SUBJECT_IDENTIFIER_TYPES: &[&str] =
        &["mrn", "member_id", "cair_id", "portal_login", "other"];

    // Phase 2
    pub const ALLERGY_CATEGORIES: &[&str] = &["drug", "food", "environmental", "other"];
    // Reaction severity — SNOMED CT clinical-severity value set (what C-CDA / Epic
    // emit). Distinct from criticality (`ALLERGY_CRITICALITIES`).
    pub const ALLERGY_SEVERITIES: &[&str] =
        &["mild", "moderate", "severe", "life-threatening", "fatal"];
    // FHIR AllergyIntolerance.criticality value set.
    pub const ALLERGY_CRITICALITIES: &[&str] = &["high", "low", "unable-to-assess"];
    pub const ALLERGY_STATUSES: &[&str] = &["active", "inactive", "resolved", "entered_in_error"];
    pub const MEDICATION_STATUSES: &[&str] =
        &["active", "completed", "stopped", "on_hold", "entered_in_error"];
    pub const CONDITION_STATUSES: &[&str] =
        &["active", "resolved", "remission", "entered_in_error"];
    pub const IMMUNIZATION_STATUSES: &[&str] = &["completed", "not_given", "entered_in_error"];
    pub const OBSERVATION_CATEGORIES: &[&str] = &["vital", "lab", "measurement"];
    pub const OBSERVATION_ABNORMAL_FLAGS: &[&str] = &["normal", "high", "low", "abnormal"];

    // Phase 3
    pub const CARE_REMINDER_KINDS: &[&str] =
        &["vaccine", "well_visit", "screening", "dental", "med_refill", "other"];
    pub const CARE_REMINDER_STATUSES: &[&str] = &["due", "done", "dismissed"];
    pub const SUBJECT_RELATIONSHIP_KINDS: &[&str] = &[
        "parent",
        "guardian",
        "child",
        "sibling",
        "spouse",
        "emergency_contact",
        "other",
    ];

    // Insurance
    pub const INSURANCE_PLAN_TYPES: &[&str] =
        &["ppo", "hmo", "epo", "pos", "hdhp", "medicare", "medicaid", "tricare", "other"];
    pub const INSURANCE_PLAN_KINDS: &[&str] =
        &["medical", "dental", "vision", "pharmacy", "other"];
    /// Beneficiary's relationship to the policyholder (FHIR Coverage.relationship).
    pub const INSURANCE_RELATIONSHIPS: &[&str] =
        &["self", "spouse", "child", "dependent", "other"];
    /// Which face of the card an image shows. Deliberately just two: a card has
    /// a front and a back, and `insurance_cards_current_side_uk` keys on this.
    pub const INSURANCE_CARD_SIDES: &[&str] = &["front", "back"];
}
