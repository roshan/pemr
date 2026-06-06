use axum::extract::{State};
use axum::response::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api_auth::ApiKeyContext;
use crate::handlers::AppState;
use crate::handlers::api::{ApiError, ApiQuery, ApiResult};
use crate::models::{Incident, Record, parse_subject_filter};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub subject: Option<String>,
    pub kind: Option<String>, // "incident" | "record" | "both" (default)
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub incidents: Vec<Incident>,
    pub records: Vec<Record>,
}

pub async fn search(
    State(state): State<AppState>,
    _ctx: ApiKeyContext,
    ApiQuery(q): ApiQuery<SearchQuery>,
) -> ApiResult<Json<SearchResponse>> {
    let query = q.q.unwrap_or_default();
    let subject = parse_subject_filter(q.subject.as_deref())
        .map_err(ApiError::bad_request)?;
    let kind = q.kind.unwrap_or_else(|| "both".to_string());
    let trimmed = query.trim();

    if trimmed.is_empty() {
        return Ok(Json(SearchResponse {
            query: String::new(),
            incidents: vec![],
            records: vec![],
        }));
    }

    let incidents = if kind == "record" {
        vec![]
    } else {
        search_incidents(&state.pool, trimmed, subject).await?
    };
    let records = if kind == "incident" {
        vec![]
    } else {
        search_records(&state.pool, trimmed, subject).await?
    };

    Ok(Json(SearchResponse {
        query: trimmed.to_string(),
        incidents,
        records,
    }))
}

async fn search_incidents(
    pool: &sqlx::PgPool,
    q: &str,
    subject: Option<Uuid>,
) -> Result<Vec<Incident>, sqlx::Error> {
    sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                created_at, updated_at
           from incidents
          where search_tsv @@ websearch_to_tsquery('english', $1)
            and ($2::uuid is null or subject_id = $2)
          order by ts_rank_cd(search_tsv, websearch_to_tsquery('english', $1)) desc,
                   created_at desc
          limit 50",
    )
    .bind(q)
    .bind(subject)
    .fetch_all(pool)
    .await
}

async fn search_records(
    pool: &sqlx::PgPool,
    q: &str,
    subject: Option<Uuid>,
) -> Result<Vec<Record>, sqlx::Error> {
    sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records
          where search_tsv @@ websearch_to_tsquery('english', $1)
            and ($2::uuid is null or subject_id = $2)
          order by ts_rank_cd(search_tsv, websearch_to_tsquery('english', $1)) desc,
                   created_at desc
          limit 50",
    )
    .bind(q)
    .bind(subject)
    .fetch_all(pool)
    .await
}
