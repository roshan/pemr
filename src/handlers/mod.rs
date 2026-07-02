use std::path::PathBuf;
use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::mpsc;

use crate::models::Subject;

pub mod api;
pub mod appointments;
pub mod care_team;
pub mod clinical;
pub mod dashboard;
pub mod incidents;
pub mod insurance;
pub mod providers;
pub mod records;
pub mod reminders;
pub mod search;
pub mod settings;
pub mod sources;
pub mod subjects;
pub mod sync;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub files_dir: Arc<PathBuf>,
    /// Trigger channel for on-demand sync task runs.
    pub sync_tx: mpsc::Sender<String>,
}

pub async fn load_subjects(pool: &PgPool) -> Result<Vec<Subject>, sqlx::Error> {
    sqlx::query_as::<_, Subject>(
        "select * from subjects order by family_name, given_name",
    )
    .fetch_all(pool)
    .await
}
