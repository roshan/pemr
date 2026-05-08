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
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Source {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    pub notes: String,
    pub created_at: OffsetDateTime,
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
    pub created_at: OffsetDateTime,
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
    pub source_synced_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
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
