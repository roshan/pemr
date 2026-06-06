//! Care team + subject identifiers CRUD (PEMR-17). Form POST → redirect.

use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{
    Provider, SUBJECT_IDENTIFIER_TYPES, SUBJECT_PROVIDER_ROLES, SUBJECT_RELATIONSHIP_KINDS, Source,
    Subject, SubjectIdentifier, SubjectProvider, SubjectRelationship, parse_date,
};
use crate::viewer::ViewerContext;
use crate::views::care_team;
use crate::views::layout::Nav;

fn redirect(sid: Uuid) -> Response {
    Redirect::to(&format!("/subjects/{sid}/care-team")).into_response()
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
    let team = sqlx::query_as::<_, SubjectProvider>(
        "select * from subject_providers where subject_id = $1 order by created_at",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let providers = sqlx::query_as::<_, Provider>("select * from providers order by full_name")
        .fetch_all(&state.pool)
        .await?;
    let identifiers = sqlx::query_as::<_, SubjectIdentifier>(
        "select * from subject_identifiers where subject_id = $1 order by created_at",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let sources = sqlx::query_as::<_, Source>("select * from sources order by name")
        .fetch_all(&state.pool)
        .await?;
    let relationships = sqlx::query_as::<_, SubjectRelationship>(
        "select * from subject_relationships where subject_id = $1 order by created_at",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(care_team::page(
        &nav,
        &s,
        &team,
        &providers,
        &identifiers,
        &sources,
        &relationships,
    ))
}

#[derive(Debug, Deserialize)]
pub struct RelationshipForm {
    pub related_subject_id: Uuid,
    pub relationship: String,
}

pub async fn add_relationship(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<RelationshipForm>,
) -> AppResult<Response> {
    let rel = form.relationship.trim().to_string();
    if !SUBJECT_RELATIONSHIP_KINDS.contains(&rel.as_str()) {
        return Err(AppError::BadRequest(format!("unknown relationship: {rel}")));
    }
    if form.related_subject_id == id {
        return Err(AppError::BadRequest("cannot relate a subject to itself".into()));
    }
    sqlx::query(
        "insert into subject_relationships (subject_id, related_subject_id, relationship)
         values ($1,$2,$3)
         on conflict (subject_id, related_subject_id, relationship) do nothing",
    )
    .bind(id)
    .bind(form.related_subject_id)
    .bind(&rel)
    .execute(&state.pool)
    .await?;
    Ok(redirect(id))
}

#[derive(Debug, Deserialize)]
pub struct CareTeamForm {
    pub provider_id: Uuid,
    pub role: String,
    #[serde(default)]
    pub since: String,
}

pub async fn add_provider(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<CareTeamForm>,
) -> AppResult<Response> {
    let role = form.role.trim().to_string();
    if !SUBJECT_PROVIDER_ROLES.contains(&role.as_str()) {
        return Err(AppError::BadRequest(format!("unknown role: {role}")));
    }
    let since = parse_date(&form.since).map_err(AppError::BadRequest)?;
    sqlx::query(
        "insert into subject_providers (subject_id, provider_id, role, active, since)
         values ($1,$2,$3,true,$4)
         on conflict (subject_id, provider_id) do update set
            role = excluded.role, active = true, since = excluded.since",
    )
    .bind(id)
    .bind(form.provider_id)
    .bind(&role)
    .bind(since)
    .execute(&state.pool)
    .await?;
    Ok(redirect(id))
}

pub async fn remove_provider(
    State(state): State<AppState>,
    Path((id, provider_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    sqlx::query("delete from subject_providers where subject_id = $1 and provider_id = $2")
        .bind(id)
        .bind(provider_id)
        .execute(&state.pool)
        .await?;
    Ok(redirect(id))
}

#[derive(Debug, Deserialize)]
pub struct IdentifierForm {
    pub source_id: Uuid,
    pub id_type: String,
    pub value: String,
}

pub async fn add_identifier(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<IdentifierForm>,
) -> AppResult<Response> {
    let id_type = form.id_type.trim().to_string();
    if !SUBJECT_IDENTIFIER_TYPES.contains(&id_type.as_str()) {
        return Err(AppError::BadRequest(format!("unknown id_type: {id_type}")));
    }
    let value = form.value.trim().to_string();
    if value.is_empty() {
        return Err(AppError::BadRequest("value required".into()));
    }
    sqlx::query(
        "insert into subject_identifiers (id, subject_id, source_id, id_type, value)
         values ($1,$2,$3,$4,$5)
         on conflict (source_id, id_type, value) do update set
            subject_id = excluded.subject_id, updated_at = now()",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(form.source_id)
    .bind(&id_type)
    .bind(&value)
    .execute(&state.pool)
    .await?;
    Ok(redirect(id))
}

pub async fn remove_identifier(
    State(state): State<AppState>,
    Path((id, ident_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    sqlx::query("delete from subject_identifiers where id = $1 and subject_id = $2")
        .bind(ident_id)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(redirect(id))
}
