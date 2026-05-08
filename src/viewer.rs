//! Viewer middleware: reads `Cf-Access-Authenticated-User-Email` (or the
//! `DEV_VIEWER_EMAIL` env var as a local-dev fallback), looks up the matching
//! `subjects.cf_access_email`, and stores the result in the request extensions
//! as `ViewerContext`.
//!
//! This drives **UI defaults only** — it never gates access. See CLAUDE.md.

use axum::extract::{FromRequestParts, Request};
use axum::http::{HeaderName, request::Parts};
use axum::middleware::Next;
use axum::response::Response;
use sqlx::PgPool;
use uuid::Uuid;

const CF_EMAIL_HEADER: HeaderName = HeaderName::from_static("cf-access-authenticated-user-email");

#[derive(Clone, Debug, Default)]
pub struct ViewerContext {
    pub email: Option<String>,
    pub default_subject_id: Option<Uuid>,
}

#[derive(Clone)]
pub struct ViewerConfig {
    pub pool: PgPool,
    pub dev_viewer_email: Option<String>,
}

pub async fn middleware(
    axum::extract::State(cfg): axum::extract::State<ViewerConfig>,
    mut req: Request,
    next: Next,
) -> Response {
    let header_email = req
        .headers()
        .get(&CF_EMAIL_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let email = header_email.or_else(|| cfg.dev_viewer_email.clone());

    let default_subject_id = match &email {
        Some(e) => sqlx::query_scalar::<_, Uuid>(
            "select id from subjects where cf_access_email = $1",
        )
        .bind(e)
        .fetch_optional(&cfg.pool)
        .await
        .unwrap_or(None),
        None => None,
    };

    let ctx = ViewerContext {
        email,
        default_subject_id,
    };
    req.extensions_mut().insert(ctx);
    next.run(req).await
}

impl<S> FromRequestParts<S> for ViewerContext
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<ViewerContext>()
            .cloned()
            .unwrap_or_default())
    }
}
