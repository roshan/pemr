//! `/api/v1/subject-relationships` — family graph / guardianship edges.
//! Read (list) + idempotent-upsert keyed on the
//! (subject_id, related_subject_id, relationship) PK.

use axum::extract::{State};
use axum::response::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiJson, ApiQuery, ApiResult, clamp_limit, clamp_offset, validate_in, write_err};
use crate::models::{SUBJECT_RELATIONSHIP_KINDS, SubjectRelationship, parse_subject_filter};

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
) -> ApiResult<Json<Vec<SubjectRelationship>>> {
    let subject = parse_subject_filter(q.subject.as_deref()).map_err(ApiError::bad_request)?;
    let rows = sqlx::query_as::<_, SubjectRelationship>(
        "select * from subject_relationships
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

#[derive(Debug, Deserialize)]
pub struct Create {
    pub subject_id: Uuid,
    pub related_subject_id: Uuid,
    pub relationship: String,
    #[serde(default)]
    pub notes: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiJson(c): ApiJson<Create>,
) -> ApiResult<Json<SubjectRelationship>> {
    let relationship = c.relationship.trim().to_string();
    validate_in("relationship", &relationship, SUBJECT_RELATIONSHIP_KINDS)?;
    if c.subject_id == c.related_subject_id {
        return Err(ApiError::bad_request(
            "subject_id and related_subject_id must differ",
        ));
    }
    let row = sqlx::query_as::<_, SubjectRelationship>(
        "insert into subject_relationships (subject_id, related_subject_id, relationship, notes)
         values ($1,$2,$3,$4)
         on conflict (subject_id, related_subject_id, relationship) do update set
            notes = excluded.notes
         returning *",
    )
    .bind(c.subject_id)
    .bind(c.related_subject_id)
    .bind(&relationship)
    .bind(c.notes.unwrap_or_default())
    .fetch_one(&state.pool)
    .await
    .map_err(write_err)?;
    Ok(Json(row))
}
