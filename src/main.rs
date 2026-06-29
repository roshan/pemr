mod api_auth;
mod config;
mod db;
mod dicom_import;
mod error;
mod files;
mod growth_ref;
mod handlers;
mod images;
mod import_cli;
mod importer;
mod models;
mod peds;
mod sync;
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
    let ui = Router::<handlers::AppState>::new()
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
        .route("/subjects/{id}/growth", get(handlers::subjects::growth))
        .route(
            "/subjects/{id}/immunizations",
            get(handlers::subjects::immunizations).post(handlers::clinical::add_immunization),
        )
        // clinical entry (PEMR-3 UI)
        .route("/subjects/{id}/clinical", get(handlers::clinical::page))
        .route("/subjects/{id}/allergies", post(handlers::clinical::add_allergy))
        .route("/subjects/{id}/medications", post(handlers::clinical::add_medication))
        .route("/subjects/{id}/conditions", post(handlers::clinical::add_condition))
        .route("/subjects/{id}/observations", post(handlers::clinical::add_observation))
        .route("/subjects/{id}/summary", get(handlers::subjects::summary))
        .route(
            "/subjects/{id}/appointments",
            get(handlers::appointments::list).post(handlers::appointments::create),
        )
        .route(
            "/appointments/{id}/edit",
            get(handlers::appointments::edit_form).post(handlers::appointments::edit),
        )
        // care team + identifiers (PEMR-17)
        .route(
            "/subjects/{id}/care-team",
            get(handlers::care_team::page).post(handlers::care_team::add_provider),
        )
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
        // care reminders (PEMR-19)
        .route(
            "/subjects/{id}/reminders",
            get(handlers::reminders::page).post(handlers::reminders::add),
        )
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
        .layer(axum::middleware::from_fn_with_state(
            viewer_cfg,
            viewer::middleware,
        ));

    // The API router. Gated by Bearer tokens from the `api_keys` table —
    // see `api_auth::middleware`. Errors are JSON, not HTML.
    let api = Router::<handlers::AppState>::new()
        .route("/api/v1", get(handlers::api::root::index))
        .route("/api/v1/me", get(handlers::api::me::me))
        .route("/api/v1/subjects", get(handlers::api::subjects::list))
        .route("/api/v1/subjects/{id}", get(handlers::api::subjects::detail))
        .route("/api/v1/incidents", get(handlers::api::incidents::list))
        .route("/api/v1/incidents/{id}", get(handlers::api::incidents::detail))
        .route("/api/v1/records", get(handlers::api::records::list))
        .route("/api/v1/records/{id}", get(handlers::api::records::detail))
        .route("/api/v1/records/{id}/file", get(handlers::api::records::file))
        .route("/api/v1/records/{id}/preview", get(handlers::api::records::preview))
        .route("/api/v1/records/{id}/thumbnail", get(handlers::api::records::thumbnail))
        .route(
            "/api/v1/sources",
            get(handlers::api::sources::list).post(handlers::api::sources::create),
        )
        .route("/api/v1/sources/{id}", get(handlers::api::sources::detail))
        // Clinical read/write surface. POST = idempotent upsert (see api::mod);
        // GET list accepts ?subject=&limit=&offset=. Reference data (providers)
        // is not subject-scoped. Join tables have no detail-by-id route.
        .route(
            "/api/v1/providers",
            get(handlers::api::providers::list).post(handlers::api::providers::create),
        )
        .route("/api/v1/providers/{id}", get(handlers::api::providers::detail))
        .route(
            "/api/v1/appointments",
            get(handlers::api::appointments::list).post(handlers::api::appointments::create),
        )
        .route(
            "/api/v1/appointments/{id}",
            get(handlers::api::appointments::detail),
        )
        .route(
            "/api/v1/allergies",
            get(handlers::api::allergies::list).post(handlers::api::allergies::create),
        )
        .route("/api/v1/allergies/{id}", get(handlers::api::allergies::detail))
        .route(
            "/api/v1/medications",
            get(handlers::api::medications::list).post(handlers::api::medications::create),
        )
        .route(
            "/api/v1/medications/{id}",
            get(handlers::api::medications::detail),
        )
        .route(
            "/api/v1/conditions",
            get(handlers::api::conditions::list).post(handlers::api::conditions::create),
        )
        .route(
            "/api/v1/conditions/{id}",
            get(handlers::api::conditions::detail),
        )
        .route(
            "/api/v1/immunizations",
            get(handlers::api::immunizations::list).post(handlers::api::immunizations::create),
        )
        .route(
            "/api/v1/immunizations/{id}",
            get(handlers::api::immunizations::detail),
        )
        .route(
            "/api/v1/observations",
            get(handlers::api::observations::list).post(handlers::api::observations::create),
        )
        .route(
            "/api/v1/observations/{id}",
            get(handlers::api::observations::detail),
        )
        .route(
            "/api/v1/care-reminders",
            get(handlers::api::care_reminders::list).post(handlers::api::care_reminders::create),
        )
        .route(
            "/api/v1/care-reminders/{id}",
            get(handlers::api::care_reminders::detail),
        )
        .route(
            "/api/v1/subject-identifiers",
            get(handlers::api::subject_identifiers::list)
                .post(handlers::api::subject_identifiers::create),
        )
        .route(
            "/api/v1/subject-identifiers/{id}",
            get(handlers::api::subject_identifiers::detail),
        )
        .route(
            "/api/v1/subject-providers",
            get(handlers::api::subject_providers::list)
                .post(handlers::api::subject_providers::create),
        )
        .route(
            "/api/v1/subject-relationships",
            get(handlers::api::subject_relationships::list)
                .post(handlers::api::subject_relationships::create),
        )
        .route("/api/v1/search", get(handlers::api::search::search))
        .route("/api/v1/import/fhir", post(handlers::api::import::fhir))
        .route("/api/v1/import/ccda", post(handlers::api::import::ccda))
        .layer(axum::middleware::from_fn_with_state(
            pool.clone(),
            api_auth::middleware,
        ));

    let app = Router::new()
        .merge(ui)
        .merge(api)
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
