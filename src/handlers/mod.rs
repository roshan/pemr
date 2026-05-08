use std::path::PathBuf;
use std::sync::Arc;

use sqlx::PgPool;

use crate::models::Subject;

pub mod dashboard;
pub mod incidents;
pub mod records;
pub mod search;
pub mod sources;
pub mod subjects;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub files_dir: Arc<PathBuf>,
}

pub async fn load_subjects(pool: &PgPool) -> Result<Vec<Subject>, sqlx::Error> {
    sqlx::query_as::<_, Subject>(
        "select * from subjects order by family_name, given_name",
    )
    .fetch_all(pool)
    .await
}
