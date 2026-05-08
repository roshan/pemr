use axum::extract::{Query, State};
use maud::Markup;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{Incident, Record, parse_subject_filter};
use crate::views::search::{self, SearchResults};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub subject: Option<String>,
    pub kind: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Markup> {
    let query = q.q.unwrap_or_default();
    let subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    let kind = q.kind.unwrap_or_else(|| "both".to_string());

    let subjects = load_subjects(&state.pool).await?;
    let trimmed = query.trim();

    let (incidents, records) = if trimmed.is_empty() {
        (vec![], vec![])
    } else {
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
        (incidents, records)
    };

    Ok(search::results_partial(&SearchResults {
        query: trimmed,
        incidents: &incidents,
        records: &records,
        subjects: &subjects,
    }))
}

async fn search_incidents(
    pool: &PgPool,
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
    pool: &PgPool,
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
