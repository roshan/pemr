mod config;
mod db;
mod dicom_import;
mod error;
mod files;
mod handlers;
mod images;
mod models;
mod viewer;
mod views;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::handlers::AppState;
use crate::viewer::ViewerConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,personal_emr=debug".into()),
        )
        .with_target(false)
        .init();

    let cfg = Config::from_env().map_err(|e| {
        tracing::error!("config error: {e}");
        e
    })?;
    tracing::info!(?cfg, "starting personal-emr");

    tokio::fs::create_dir_all(&cfg.files_dir).await?;

    let pool = db::connect(&cfg.database_url).await?;

    let state = AppState {
        pool: pool.clone(),
        files_dir: Arc::new(cfg.files_dir.clone()),
    };

    let viewer_cfg = ViewerConfig {
        pool: pool.clone(),
        dev_viewer_email: cfg.dev_viewer_email.clone(),
    };

    let app = Router::new()
        .route("/", get(handlers::dashboard::index))
        .route("/timeline", get(handlers::dashboard::timeline_page))
        .route("/healthz", get(handlers::dashboard::healthz))
        .route("/search", get(handlers::search::search))
        // incidents
        .route("/incidents", get(handlers::incidents::list).post(handlers::incidents::create))
        .route("/incidents/new", get(handlers::incidents::new))
        .route("/incidents/{id}", get(handlers::incidents::detail))
        .route(
            "/incidents/{id}/edit",
            get(handlers::incidents::edit_form).post(handlers::incidents::edit),
        )
        .route("/incidents/{id}/link", post(handlers::incidents::link_record))
        .route(
            "/incidents/{id}/records/{rid}",
            delete(handlers::incidents::unlink_record),
        )
        // records
        .route("/records", get(handlers::records::list).post(handlers::records::create))
        .route("/records/new", get(handlers::records::new))
        .route(
            "/records/import",
            get(handlers::records::import_form).post(handlers::records::import),
        )
        .route("/records/{id}", get(handlers::records::detail))
        .route(
            "/records/{id}/edit",
            get(handlers::records::edit_form).post(handlers::records::edit),
        )
        .route("/records/{id}/file", get(handlers::records::file_route))
        .route("/records/{id}/preview", get(handlers::records::preview_route))
        .route(
            "/records/{id}/thumbnail",
            get(handlers::records::thumbnail_route),
        )
        // subjects
        .route("/subjects", get(handlers::subjects::list).post(handlers::subjects::create))
        .route("/subjects/{id}", get(handlers::subjects::detail))
        .route(
            "/subjects/{id}/edit",
            get(handlers::subjects::edit_form).post(handlers::subjects::edit),
        )
        // per-subject path-style scopes
        .route("/subjects/{id}/records", get(handlers::records::list_for_subject))
        .route("/subjects/{id}/incidents", get(handlers::incidents::list_for_subject))
        .route("/subjects/{id}/timeline", get(handlers::dashboard::timeline_for_subject))
        // sources
        .route("/sources", get(handlers::sources::list).post(handlers::sources::create))
        .route("/sources/{id}", get(handlers::sources::detail))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
        .layer(axum::middleware::from_fn_with_state(
            viewer_cfg,
            viewer::middleware,
        ))
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .layer(RequestBodyLimitLayer::new(1024 * 1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!("listening on http://{}", cfg.bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
