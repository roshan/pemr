use std::future::Future;
use std::pin::Pin;

use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::time::Duration;

pub mod vaccine;

pub type TaskFn =
    fn(PgPool) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

pub struct TaskDef {
    pub name: &'static str,
    pub schedule_hours: i64,
    pub run: TaskFn,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SyncJob {
    pub name: String,
    pub schedule_hours: i32,
    pub last_started_at: Option<OffsetDateTime>,
    pub last_finished_at: Option<OffsetDateTime>,
    pub last_status: Option<String>,
    pub last_message: Option<String>,
    pub next_run_at: OffsetDateTime,
}

/// Registered scheduled tasks. Add future periodic tasks here (e.g. insurance
/// card refresh, lab portal sync). The vaccine import is NOT here — CDPH URLs
/// expire in 24 h so it's triggered manually via the import form, not scheduled.
static ALL_TASKS: &[TaskDef] = &[];

pub async fn all_jobs(pool: &PgPool) -> Result<Vec<SyncJob>, sqlx::Error> {
    sqlx::query_as::<_, SyncJob>("select * from sync_jobs order by name")
        .fetch_all(pool)
        .await
}

/// Records the result of a manually-triggered import into sync_jobs.
/// Creates the row if it doesn't exist yet.
pub async fn record_import(pool: &PgPool, name: &str, status: &str, message: &str) {
    // next_run_at = far future so the scheduler never picks this up, but we
    // can't use PostgreSQL 'infinity' — time::OffsetDateTime can't represent it
    // and sqlx will error on deserialization.
    let _ = sqlx::query(
        "insert into sync_jobs (name, schedule_hours, last_started_at, last_finished_at,
                                last_status, last_message, next_run_at)
         values ($1, 0, now(), now(), $2, $3, now() + interval '10 years')
         on conflict (name) do update set
             last_started_at  = now(),
             last_finished_at = now(),
             last_status      = $2,
             last_message     = $3",
    )
    .bind(name)
    .bind(status)
    .bind(message)
    .execute(pool)
    .await;
}

pub async fn run_loop(pool: PgPool, mut trigger_rx: mpsc::Receiver<String>) {
    for task in ALL_TASKS {
        let _ = sqlx::query(
            "insert into sync_jobs (name, schedule_hours)
             values ($1, $2)
             on conflict (name) do nothing",
        )
        .bind(task.name)
        .bind(task.schedule_hours as i32)
        .execute(&pool)
        .await;
    }

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                check_due(&pool).await;
            }
            Some(name) = trigger_rx.recv() => {
                if let Some(task) = ALL_TASKS.iter().find(|t| t.name == name) {
                    execute(&pool, task).await;
                }
            }
        }
    }
}

async fn check_due(pool: &PgPool) {
    for task in ALL_TASKS {
        let due = sqlx::query_scalar::<_, bool>(
            "select next_run_at <= now() from sync_jobs where name = $1",
        )
        .bind(task.name)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(false);

        if due {
            execute(pool, task).await;
        }
    }
}

async fn execute(pool: &PgPool, task: &TaskDef) {
    tracing::info!(task = task.name, "sync task starting");
    let _ = sqlx::query(
        "update sync_jobs
            set last_started_at = now(),
                last_status = 'running',
                next_run_at = now() + ($1::integer * interval '1 hour')
          where name = $2",
    )
    .bind(task.schedule_hours as i32)
    .bind(task.name)
    .execute(pool)
    .await;

    let result = (task.run)(pool.clone()).await;

    let (status, message) = match &result {
        Ok(msg) => {
            tracing::info!(task = task.name, %msg, "sync task ok");
            ("ok", msg.clone())
        }
        Err(msg) => {
            tracing::error!(task = task.name, %msg, "sync task error");
            ("error", msg.clone())
        }
    };

    let _ = sqlx::query(
        "update sync_jobs
            set last_finished_at = now(),
                last_status = $1,
                last_message = $2
          where name = $3",
    )
    .bind(status)
    .bind(&message)
    .bind(task.name)
    .execute(pool)
    .await;
}
