//! Per-subject clinical-entry CRUD (PEMR-3 UI). Manual add forms for allergies,
//! medications, conditions, immunizations, observations. Form POST → redirect to
//! the chart. Mirrors the API write validation; no provenance (manual entry).

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{
    ALLERGY_CATEGORIES, ALLERGY_SEVERITIES, ALLERGY_STATUSES, CONDITION_STATUSES,
    IMMUNIZATION_STATUSES, MEDICATION_STATUSES, OBSERVATION_CATEGORIES, Subject, empty_to_none,
    parse_date,
};
use crate::viewer::ViewerContext;
use crate::views::clinical_entry;
use crate::views::layout::Nav;

fn back(sid: Uuid) -> Response {
    Redirect::to(&format!("/subjects/{sid}")).into_response()
}

fn status_or(default: &str, val: &str, allowed: &[&str]) -> AppResult<String> {
    let v = val.trim();
    let v = if v.is_empty() { default } else { v };
    if !allowed.contains(&v) {
        return Err(AppError::BadRequest(format!("invalid value: {v}")));
    }
    Ok(v.to_string())
}

fn opt_vocab(field: &str, val: &str, allowed: &[&str]) -> AppResult<Option<String>> {
    match empty_to_none(val.to_string()) {
        None => Ok(None),
        Some(v) => {
            if !allowed.contains(&v.as_str()) {
                return Err(AppError::BadRequest(format!("invalid {field}: {v}")));
            }
            Ok(Some(v))
        }
    }
}

fn opt_f64(val: &str) -> AppResult<Option<f64>> {
    match empty_to_none(val.to_string()) {
        None => Ok(None),
        Some(v) => v
            .parse::<f64>()
            .map(Some)
            .map_err(|_| AppError::BadRequest("value must be a number".into())),
    }
}

fn opt_i32(val: &str) -> AppResult<Option<i32>> {
    match empty_to_none(val.to_string()) {
        None => Ok(None),
        Some(v) => v
            .parse::<i32>()
            .map(Some)
            .map_err(|_| AppError::BadRequest("dose number must be an integer".into())),
    }
}

pub async fn page(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(clinical_entry::page(&nav, &s))
}

#[derive(Debug, Deserialize)]
pub struct AllergyForm {
    pub substance: String,
    #[serde(default)] pub category: String,
    #[serde(default)] pub severity: String,
    #[serde(default)] pub reaction: String,
    #[serde(default)] pub status: String,
    #[serde(default)] pub onset_date: String,
    #[serde(default)] pub notes: String,
}

pub async fn add_allergy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(f): Form<AllergyForm>,
) -> AppResult<Response> {
    let substance = f.substance.trim().to_string();
    if substance.is_empty() {
        return Err(AppError::BadRequest("substance required".into()));
    }
    let status = status_or("active", &f.status, ALLERGY_STATUSES)?;
    let category = opt_vocab("category", &f.category, ALLERGY_CATEGORIES)?;
    let severity = opt_vocab("severity", &f.severity, ALLERGY_SEVERITIES)?;
    let onset = parse_date(&f.onset_date).map_err(AppError::BadRequest)?;
    sqlx::query(
        "insert into allergies (id, subject_id, substance, category, severity, reaction, status, onset_date, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(Uuid::now_v7()).bind(id).bind(&substance).bind(category).bind(severity)
    .bind(empty_to_none(f.reaction)).bind(&status).bind(onset).bind(f.notes)
    .execute(&state.pool).await?;
    Ok(back(id))
}

#[derive(Debug, Deserialize)]
pub struct MedicationForm {
    pub name: String,
    #[serde(default)] pub dose: String,
    #[serde(default)] pub frequency: String,
    #[serde(default)] pub status: String,
    #[serde(default)] pub started_on: String,
    #[serde(default)] pub reason: String,
    #[serde(default)] pub notes: String,
}

pub async fn add_medication(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(f): Form<MedicationForm>,
) -> AppResult<Response> {
    let name = f.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }
    let status = status_or("active", &f.status, MEDICATION_STATUSES)?;
    let started = parse_date(&f.started_on).map_err(AppError::BadRequest)?;
    sqlx::query(
        "insert into medications (id, subject_id, name, dose, frequency, status, started_on, reason, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(Uuid::now_v7()).bind(id).bind(&name).bind(empty_to_none(f.dose)).bind(empty_to_none(f.frequency))
    .bind(&status).bind(started).bind(empty_to_none(f.reason)).bind(f.notes)
    .execute(&state.pool).await?;
    Ok(back(id))
}

#[derive(Debug, Deserialize)]
pub struct ConditionForm {
    pub name: String,
    #[serde(default)] pub status: String,
    #[serde(default)] pub onset_date: String,
    #[serde(default)] pub severity: String,
    #[serde(default)] pub notes: String,
}

pub async fn add_condition(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(f): Form<ConditionForm>,
) -> AppResult<Response> {
    let name = f.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }
    let status = status_or("active", &f.status, CONDITION_STATUSES)?;
    let onset = parse_date(&f.onset_date).map_err(AppError::BadRequest)?;
    sqlx::query(
        "insert into conditions (id, subject_id, name, status, onset_date, severity, notes)
         values ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(Uuid::now_v7()).bind(id).bind(&name).bind(&status).bind(onset)
    .bind(empty_to_none(f.severity)).bind(f.notes)
    .execute(&state.pool).await?;
    Ok(back(id))
}

#[derive(Debug, Deserialize)]
pub struct ImmunizationForm {
    pub vaccine: String,
    #[serde(default)] pub code: String,
    #[serde(default)] pub occurred_at: String,
    #[serde(default)] pub dose_number: String,
    #[serde(default)] pub status: String,
    #[serde(default)] pub notes: String,
}

pub async fn add_immunization(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(f): Form<ImmunizationForm>,
) -> AppResult<Response> {
    let vaccine = f.vaccine.trim().to_string();
    if vaccine.is_empty() {
        return Err(AppError::BadRequest("vaccine required".into()));
    }
    let status = status_or("completed", &f.status, IMMUNIZATION_STATUSES)?;
    let occurred = parse_date(&f.occurred_at).map_err(AppError::BadRequest)?;
    let dose = opt_i32(&f.dose_number)?;
    let code_system = if empty_to_none(f.code.clone()).is_some() { Some("CVX".to_string()) } else { None };
    sqlx::query(
        "insert into immunizations (id, subject_id, vaccine, code, code_system, occurred_at, dose_number, status, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(Uuid::now_v7()).bind(id).bind(&vaccine).bind(empty_to_none(f.code)).bind(code_system)
    .bind(occurred).bind(dose).bind(&status).bind(f.notes)
    .execute(&state.pool).await?;
    Ok(back(id))
}

#[derive(Debug, Deserialize)]
pub struct ObservationForm {
    pub display: String,
    #[serde(default)] pub category: String,
    #[serde(default)] pub code: String,
    #[serde(default)] pub value_num: String,
    #[serde(default)] pub unit: String,
    #[serde(default)] pub effective_on: String,
    #[serde(default)] pub notes: String,
}

pub async fn add_observation(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(f): Form<ObservationForm>,
) -> AppResult<Response> {
    let display = f.display.trim().to_string();
    if display.is_empty() {
        return Err(AppError::BadRequest("display required".into()));
    }
    let category = status_or("vital", &f.category, OBSERVATION_CATEGORIES)?;
    let value_num = opt_f64(&f.value_num)?;
    let effective_on = parse_date(&f.effective_on)
        .map_err(AppError::BadRequest)?
        .ok_or_else(|| AppError::BadRequest("effective date required".into()))?;
    let code_system = if empty_to_none(f.code.clone()).is_some() { Some("LOINC".to_string()) } else { None };
    sqlx::query(
        "insert into observations (id, subject_id, category, code, code_system, display, value_num, unit, effective_on, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(Uuid::now_v7()).bind(id).bind(&category).bind(empty_to_none(f.code)).bind(code_system)
    .bind(&display).bind(value_num).bind(empty_to_none(f.unit)).bind(effective_on).bind(f.notes)
    .execute(&state.pool).await?;
    Ok(back(id))
}
