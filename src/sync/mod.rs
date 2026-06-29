use std::future::Future;
use std::pin::Pin;

use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::time::Duration;

mod vaccine;

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
    pub schedule_hours: i64,
    pub last_started_at: Option<OffsetDateTime>,
    pub last_finished_at: Option<OffsetDateTime>,
    pub last_status: Option<String>,
    pub last_message: Option<String>,
    pub next_run_at: OffsetDateTime,
}

static ALL_TASKS: &[TaskDef] = &[TaskDef {
    name: "vaccine_records",
    schedule_hours: 168,
    run: |pool| Box::pin(vaccine::run(pool)),
}];

pub async fn all_jobs(pool: &PgPool) -> Result<Vec<SyncJob>, sqlx::Error> {
    sqlx::query_as::<_, SyncJob>("select * from sync_jobs order by name")
        .fetch_all(pool)
        .await
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
