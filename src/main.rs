mod api_auth;
mod api_routes;
mod config;
mod db;
mod dedupe;
mod dicom_import;
mod error;
mod feature_registry;
mod files;
mod growth_ref;
mod handlers;
mod images;
mod import_cli;
mod importer;
mod milestone_age;
mod milestones;
mod models;
mod peds;
mod subject_modules;
mod subject_pages;
mod sync;
mod timeline_kinds;
mod viewer;
mod views;

use std::sync::Arc;

use axum::Router;
use axum::ServiceExt;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use tokio::sync::mpsc;
use tower::Layer;
use tower_http::compression::CompressionLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::normalize_path::NormalizePathLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::handlers::AppState;
use crate::viewer::ViewerConfig;

/// Explicit 404 for unmatched paths. Without this, axum's default fallback
/// inherits the API router's auth layer when the routers merge, and every
/// typo'd URL turns into a bare 401. `/api/v1/*` misses keep the API's JSON
/// error shape; everything else gets a small HTML page. (CF Access has
/// already authenticated the caller at the edge either way.)
async fn not_found(uri: axum::http::Uri) -> axum::response::Response {
    use axum::response::IntoResponse;
    if uri.path().starts_with("/api/v1") {
        handlers::api::ApiError::not_found().into_response()
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            views::layout::not_found_page(uri.path()),
        )
            .into_response()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Offline subcommand: `personal-emr import <path> ...` upserts a MyChart /
    // C-CDA / FHIR export straight into DATABASE_URL and exits — no server.
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("import") {
        if let Err(e) = import_cli::run(&argv[2..]).await {
            eprintln!("import error: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

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

    let (sync_tx, sync_rx) = mpsc::channel::<String>(32);
    tokio::spawn(sync::run_loop(pool.clone(), sync_rx));

    let state = AppState {
        pool: pool.clone(),
        files_dir: Arc::new(cfg.files_dir.clone()),
        sync_tx,
    };

    let viewer_cfg = ViewerConfig {
        pool: pool.clone(),
        dev_viewer_email: cfg.dev_viewer_email.clone(),
    };

    // The UI router. CF Access gates entry in production; the viewer
    // middleware reads `Cf-Access-Authenticated-User-Email` and sets a UI
    // default subject. It never gates access.
    // Per-subject pages (`/subjects/{id}/{slug}`) come from the `subject_pages`
    // registry — the same registry the chart's actions row iterates, so the
    // routes and buttons never drift.
    let ui = subject_pages::register(Router::<handlers::AppState>::new())
        .route("/", get(handlers::dashboard::index))
        .route("/timeline", get(handlers::dashboard::timeline_page))
        .route("/timeline/day", get(handlers::dashboard::timeline_day))
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
        .route(
            "/incidents/{id}/link-incident",
            post(handlers::incidents::link_incident),
        )
        .route(
            "/incidents/{id}/link-incident/candidates",
            get(handlers::incidents::link_incident_candidates),
        )
        .route(
            "/incidents/{id}/linked-incidents/{other_id}",
            delete(handlers::incidents::unlink_incident),
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
        // per-subject path-style scopes (`/subjects/{id}/records`, …) come from
        // `subject_pages::scoped_sections` — the same registry
        // `layout::subject_scoped_url` consults, so the switcher's URLs and the
        // routes never drift. Registered by `subject_pages::register` above.
        // clinical entry (PEMR-3 UI)
        .route("/subjects/{id}/clinical", get(handlers::clinical::page))
        // per-subject opt-in feature registry (PEMR-45): enable / disable a module
        .route(
            "/subjects/{id}/features/{key}",
            post(handlers::milestones::enable_feature),
        )
        .route(
            "/subjects/{id}/features/{key}/disable",
            post(handlers::milestones::disable_feature),
        )
        // developmental milestone tracker (PEMR-35). The checklist + mark endpoints
        // return HTMX partials; progress + summary are full pages. Feature-gated
        // surfaces, so these stay explicit sub-actions (not `subject_pages`).
        .route(
            "/subjects/{id}/milestones",
            get(handlers::milestones::detail),
        )
        .route(
            "/subjects/{id}/milestones/checklist",
            get(handlers::milestones::checklist),
        )
        .route(
            "/subjects/{id}/milestones/mark/{key}/{response}",
            post(handlers::milestones::mark),
        )
        .route(
            "/subjects/{id}/milestones/progress",
            get(handlers::milestones::progress),
        )
        .route(
            "/subjects/{id}/milestones/summary",
            get(handlers::milestones::summary),
        )
        .route("/subjects/{id}/allergies", post(handlers::clinical::add_allergy))
        .route("/subjects/{id}/medications", post(handlers::clinical::add_medication))
        .route(
            "/subjects/{id}/medications/{med_id}",
            get(handlers::clinical::medication_detail),
        )
        .route("/subjects/{id}/conditions", post(handlers::clinical::add_condition))
        .route("/subjects/{id}/observations", post(handlers::clinical::add_observation))
        .route(
            "/appointments/{id}/edit",
            get(handlers::appointments::edit_form).post(handlers::appointments::edit),
        )
        // care team + identifiers (PEMR-17) — the care-team page itself is in
        // `subject_pages`; these are its sub-actions
        .route(
            "/subjects/{id}/care-team/{pid}/remove",
            post(handlers::care_team::remove_provider),
        )
        .route(
            "/subjects/{id}/identifiers",
            post(handlers::care_team::add_identifier),
        )
        .route(
            "/subjects/{id}/identifiers/{iid}/remove",
            post(handlers::care_team::remove_identifier),
        )
        .route(
            "/subjects/{id}/relationships",
            post(handlers::care_team::add_relationship),
        )
        // care reminders (PEMR-19) — the reminders page is in `subject_pages`
        .route(
            "/subjects/{id}/reminders/{rid}/{status}",
            post(handlers::reminders::mark),
        )
        // sources
        .route("/sources", get(handlers::sources::list).post(handlers::sources::create))
        .route("/sources/{id}", get(handlers::sources::detail))
        // providers (PEMR-17)
        .route("/providers", get(handlers::providers::list).post(handlers::providers::create))
        .route("/providers/{id}/edit", get(handlers::providers::edit_form).post(handlers::providers::edit))
        // insurance (shared cards + per-subject coverage)
        .route("/insurance", get(handlers::insurance::list).post(handlers::insurance::create))
        .route("/insurance/{id}", get(handlers::insurance::detail))
        .route(
            "/insurance/{id}/edit",
            get(handlers::insurance::edit_form).post(handlers::insurance::edit),
        )
        .route("/insurance/{id}/cards", post(handlers::insurance::upload_card))
        .route(
            "/insurance/{id}/cards/{card_id}/supersede",
            post(handlers::insurance::supersede_card),
        )
        .route(
            "/insurance/cards/{card_id}/file",
            get(handlers::insurance::card_file),
        )
        .route(
            "/insurance/cards/{card_id}/thumbnail",
            get(handlers::insurance::card_thumbnail),
        )
        .route("/insurance/{id}/subjects", post(handlers::insurance::cover_subject))
        .route(
            "/insurance/{id}/subjects/{sid}/remove",
            post(handlers::insurance::uncover_subject),
        )
        // settings: API keys
        .route(
            "/settings/api-keys",
            get(handlers::settings::api_keys).post(handlers::settings::create_api_key),
        )
        .route(
            "/settings/api-keys/{id}/revoke",
            post(handlers::settings::revoke_api_key),
        )
        .route("/settings/sync", get(handlers::sync::page))
        .route(
            "/settings/sync/vaccine-import",
            post(handlers::sync::import_vaccines),
        )
        .route(
            "/settings/sync/{name}/run",
            post(handlers::sync::run_task),
        )
        .route(
            "/settings/import",
            get(handlers::import::page).post(handlers::import::upload),
        )
        .route("/settings/import/form", get(handlers::import::form))
        .layer(axum::middleware::from_fn_with_state(
            viewer_cfg,
            viewer::middleware,
        ));

    // The API router. Gated by Bearer tokens from the `api_keys` table —
    // see `api_auth::middleware`. Errors are JSON, not HTML. Every endpoint
    // (route + its discovery-doc row) lives in the `api_routes` registry.
    let api = api_routes::register(Router::<handlers::AppState>::new())
        .layer(axum::middleware::from_fn_with_state(
            pool.clone(),
            api_auth::middleware,
        ));

    let app = Router::new()
        .merge(ui)
        .merge(api)
        .fallback(not_found)
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .layer(RequestBodyLimitLayer::new(1024 * 1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    // Trim a single trailing slash before routing so directory-style paths work:
    // `/api/v1/` == `/api/v1`, `/api/v1/subjects/` == `/api/v1/subjects`, etc.
    // (PEMR-8). This MUST wrap the whole app — a Router-level `.layer()` runs
    // after route matching, too late to affect which route is chosen. `/` is
    // preserved (trim_trailing_slash never reduces the root to empty).
    let app = NormalizePathLayer::trim_trailing_slash().layer(app);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!("listening on http://{}", cfg.bind_addr);
    axum::serve(
        listener,
        ServiceExt::<axum::extract::Request>::into_make_service(app),
    )
    .await?;
    Ok(())
}
