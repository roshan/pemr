//! Insurance directory + coverage CRUD. Shared reference data (a family shares
//! one card): plans live at `/insurance`; covered people are linked per-plan.
//! Plain form POST → redirect, matching the providers/sources handlers.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{
    INSURANCE_PLAN_KINDS, INSURANCE_PLAN_TYPES, INSURANCE_RELATIONSHIPS, InsurancePlan, Source,
    SubjectInsurance, empty_to_none, parse_date,
};
use crate::viewer::ViewerContext;
use crate::views::insurance;
use crate::views::layout::Nav;

async fn load_sources(pool: &sqlx::PgPool) -> Result<Vec<Source>, sqlx::Error> {
    sqlx::query_as::<_, Source>("select * from sources order by name")
        .fetch_all(pool)
        .await
}

fn parse_opt_uuid(s: String, field: &str) -> AppResult<Option<Uuid>> {
    match empty_to_none(s) {
        None => Ok(None),
        Some(v) => Uuid::parse_str(&v)
            .map(Some)
            .map_err(|_| AppError::BadRequest(format!("invalid {field}"))),
    }
}

pub async fn list(State(state): State<AppState>, viewer: ViewerContext) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let plans =
        sqlx::query_as::<_, InsurancePlan>("select * from insurance_plans order by payer_name, plan_name")
            .fetch_all(&state.pool)
            .await?;
    let counts = sqlx::query_as::<_, (Uuid, i64)>(
        "select plan_id, count(*) from subject_insurance group by plan_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let sources = load_sources(&state.pool).await?;
    let nav = Nav {
        title: "Insurance",
        current_path: "/insurance",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(insurance::list_page(&nav, &plans, &counts, &sources))
}

#[derive(Debug, Deserialize)]
pub struct PlanForm {
    pub payer_name: String,
    #[serde(default)]
    pub plan_name: String,
    #[serde(default)]
    pub plan_kind: String,
    #[serde(default)]
    pub plan_type: String,
    #[serde(default)]
    pub member_id: String,
    #[serde(default)]
    pub group_number: String,
    #[serde(default)]
    pub subscriber_name: String,
    #[serde(default)]
    pub rx_bin: String,
    #[serde(default)]
    pub rx_pcn: String,
    #[serde(default)]
    pub rx_group: String,
    #[serde(default)]
    pub payer_phone: String,
    #[serde(default)]
    pub effective_date: String,
    #[serde(default)]
    pub expiration_date: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub notes: String,
}

/// Validate + normalize the plan-kind / plan-type pair from a form.
fn validate_plan_kinds(kind: &str, plan_type: &str) -> AppResult<(String, Option<String>)> {
    let kind = if kind.trim().is_empty() { "medical" } else { kind.trim() };
    if !INSURANCE_PLAN_KINDS.contains(&kind) {
        return Err(AppError::BadRequest(format!("unknown coverage kind: {kind}")));
    }
    let plan_type = empty_to_none(plan_type.to_string());
    if let Some(t) = &plan_type {
        if !INSURANCE_PLAN_TYPES.contains(&t.as_str()) {
            return Err(AppError::BadRequest(format!("unknown plan type: {t}")));
        }
    }
    Ok((kind.to_string(), plan_type))
}

pub async fn create(State(state): State<AppState>, Form(form): Form<PlanForm>) -> AppResult<Response> {
    let payer_name = form.payer_name.trim().to_string();
    if payer_name.is_empty() {
        return Err(AppError::BadRequest("payer_name required".into()));
    }
    let (plan_kind, plan_type) = validate_plan_kinds(&form.plan_kind, &form.plan_type)?;
    let source_id = parse_opt_uuid(form.source_id, "source")?;
    let effective_date = parse_date(&form.effective_date).map_err(AppError::BadRequest)?;
    let expiration_date = parse_date(&form.expiration_date).map_err(AppError::BadRequest)?;
    sqlx::query(
        "insert into insurance_plans (id, payer_name, plan_name, plan_kind, plan_type, member_id,
            group_number, subscriber_name, rx_bin, rx_pcn, rx_group, payer_phone,
            effective_date, expiration_date, source_id, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(Uuid::now_v7())
    .bind(&payer_name)
    .bind(empty_to_none(form.plan_name))
    .bind(&plan_kind)
    .bind(plan_type)
    .bind(empty_to_none(form.member_id))
    .bind(empty_to_none(form.group_number))
    .bind(empty_to_none(form.subscriber_name))
    .bind(empty_to_none(form.rx_bin))
    .bind(empty_to_none(form.rx_pcn))
    .bind(empty_to_none(form.rx_group))
    .bind(empty_to_none(form.payer_phone))
    .bind(effective_date)
    .bind(expiration_date)
    .bind(source_id)
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to("/insurance").into_response())
}

async fn load_plan(pool: &sqlx::PgPool, id: Uuid) -> AppResult<InsurancePlan> {
    sqlx::query_as::<_, InsurancePlan>("select * from insurance_plans where id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn detail(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let plan = load_plan(&state.pool, id).await?;
    let covered = sqlx::query_as::<_, SubjectInsurance>(
        "select * from subject_insurance where plan_id = $1 order by is_primary desc, created_at",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: &plan.payer_name,
        current_path: "/insurance",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(insurance::detail_page(&nav, &plan, &covered, &subjects))
}

pub async fn edit_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let plan = load_plan(&state.pool, id).await?;
    let sources = load_sources(&state.pool).await?;
    let nav = Nav {
        title: "Edit insurance plan",
        current_path: "/insurance",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(insurance::edit_form(&nav, &plan, &sources, None))
}

pub async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<PlanForm>,
) -> AppResult<Response> {
    let payer_name = form.payer_name.trim().to_string();
    if payer_name.is_empty() {
        return Err(AppError::BadRequest("payer_name required".into()));
    }
    let (plan_kind, plan_type) = validate_plan_kinds(&form.plan_kind, &form.plan_type)?;
    let source_id = parse_opt_uuid(form.source_id, "source")?;
    let effective_date = parse_date(&form.effective_date).map_err(AppError::BadRequest)?;
    let expiration_date = parse_date(&form.expiration_date).map_err(AppError::BadRequest)?;
    sqlx::query(
        "update insurance_plans set payer_name=$2, plan_name=$3, plan_kind=$4, plan_type=$5,
            member_id=$6, group_number=$7, subscriber_name=$8, rx_bin=$9, rx_pcn=$10, rx_group=$11,
            payer_phone=$12, effective_date=$13, expiration_date=$14, source_id=$15, notes=$16,
            updated_at=now()
          where id=$1",
    )
    .bind(id)
    .bind(&payer_name)
    .bind(empty_to_none(form.plan_name))
    .bind(&plan_kind)
    .bind(plan_type)
    .bind(empty_to_none(form.member_id))
    .bind(empty_to_none(form.group_number))
    .bind(empty_to_none(form.subscriber_name))
    .bind(empty_to_none(form.rx_bin))
    .bind(empty_to_none(form.rx_pcn))
    .bind(empty_to_none(form.rx_group))
    .bind(empty_to_none(form.payer_phone))
    .bind(effective_date)
    .bind(expiration_date)
    .bind(source_id)
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/insurance/{id}")).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CoverageForm {
    pub subject_id: Uuid,
    pub relationship: String,
    #[serde(default)]
    pub member_id: String,
    #[serde(default)]
    pub is_primary: Option<String>,
}

pub async fn cover_subject(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<CoverageForm>,
) -> AppResult<Response> {
    let relationship = form.relationship.trim().to_string();
    if !INSURANCE_RELATIONSHIPS.contains(&relationship.as_str()) {
        return Err(AppError::BadRequest(format!("unknown relationship: {relationship}")));
    }
    let is_primary = form.is_primary.is_some();
    sqlx::query(
        "insert into subject_insurance (subject_id, plan_id, relationship, member_id, is_primary)
         values ($1,$2,$3,$4,$5)
         on conflict (subject_id, plan_id) do update set
            relationship = excluded.relationship, member_id = excluded.member_id,
            is_primary = excluded.is_primary",
    )
    .bind(form.subject_id)
    .bind(id)
    .bind(&relationship)
    .bind(empty_to_none(form.member_id))
    .bind(is_primary)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/insurance/{id}")).into_response())
}

pub async fn uncover_subject(
    State(state): State<AppState>,
    Path((id, subject_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    sqlx::query("delete from subject_insurance where plan_id = $1 and subject_id = $2")
        .bind(id)
        .bind(subject_id)
        .execute(&state.pool)
        .await?;
    Ok(Redirect::to(&format!("/insurance/{id}")).into_response())
}
