//! Bearer-token middleware for `/api/v1/*`.
//!
//! The UI is gated by Cloudflare Access (see [`crate::viewer`]); the API is
//! a parallel auth surface gated by an app-issued bearer token whose sha256
//! lives in `api_keys`. The contract for the assistant agent is:
//!
//! - The agent still goes through Cloudflare Access — typically via a CF
//!   Access service token configured on the `emr.roshangeorge.dev` app —
//!   so the CF tunnel will let the request reach us.
//! - The agent additionally sends `Authorization: Bearer <token>`; we
//!   sha256 the token and look it up in `api_keys`, rejecting missing or
//!   revoked rows.
//!
//! On every authenticated request we update `last_used_at` so the UI can
//! show when a key was last seen. Updates are best-effort: a failed update
//! logs a warning but does not fail the request.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{StatusCode, header, request::Parts};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

/// Prefix on every issued token. Makes scanning/grepping for leaked tokens
/// easier and signals provenance.
pub const TOKEN_PREFIX: &str = "pemr_";

/// Length of the random portion of the token in hex characters (= 16 bytes
/// of randomness from two `Uuid::new_v4()` calls would be 32 hex chars; we
/// use 32 random bytes = 64 hex chars for plenty of margin).
const RANDOM_HEX_LEN: usize = 64;

/// The slice of `pemr_<random>` we store and display in the UI to identify
/// which key a row represents. Long enough to be unique in practice, short
/// enough to be obviously not the full token.
pub const PREFIX_DISPLAY_LEN: usize = TOKEN_PREFIX.len() + 8;

#[derive(Clone, Debug)]
pub struct ApiKeyContext {
    pub key_id: Uuid,
    pub name: String,
    pub owner_subject_id: Option<Uuid>,
}

/// A freshly-generated token + its sha256 hash + the prefix we'll show in
/// the UI. The raw token is shown to the user exactly once and then thrown
/// away; only the hash and the prefix persist in the database.
pub struct GeneratedToken {
    pub raw: String,
    pub hash_hex: String,
    pub prefix: String,
}

pub fn generate_token() -> GeneratedToken {
    // 32 bytes of randomness from two Uuid::new_v4() calls (each pulls
    // from the OS CSPRNG via `getrandom`).
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    let random_hex = hex::encode(bytes);
    debug_assert_eq!(random_hex.len(), RANDOM_HEX_LEN);
    let raw = format!("{TOKEN_PREFIX}{random_hex}");
    let hash_hex = sha256_hex(raw.as_bytes());
    let prefix = raw[..PREFIX_DISPLAY_LEN].to_string();
    GeneratedToken { raw, hash_hex, prefix }
}

pub fn sha256_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hex::encode(hasher.finalize())
}

pub async fn middleware(
    State(pool): State<PgPool>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = match extract_bearer(&req) {
        Some(t) => t,
        None => return unauthorized("missing or malformed Authorization header"),
    };
    if !token.starts_with(TOKEN_PREFIX) {
        return unauthorized("invalid token");
    }
    let hash = sha256_hex(token.as_bytes());
    let row = sqlx::query_as::<_, (Uuid, String, Option<Uuid>)>(
        "select id, name, owner_subject_id
           from api_keys
          where token_hash = $1 and revoked_at is null",
    )
    .bind(&hash)
    .fetch_optional(&pool)
    .await;
    let (id, name, owner_subject_id) = match row {
        Ok(Some(row)) => row,
        Ok(None) => return unauthorized("invalid token"),
        Err(e) => {
            tracing::error!(error = %e, "api_auth lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "internal error",
            }))).into_response();
        }
    };

    if let Err(e) = sqlx::query("update api_keys set last_used_at = now() where id = $1")
        .bind(id)
        .execute(&pool)
        .await
    {
        tracing::warn!(error = %e, key_id = %id, "failed to update api_keys.last_used_at");
    }

    req.extensions_mut().insert(ApiKeyContext {
        key_id: id,
        name,
        owner_subject_id,
    });
    next.run(req).await
}

fn extract_bearer(req: &Request) -> Option<String> {
    let h = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer "))?;
    Some(rest.trim().to_string())
}

fn unauthorized(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer realm=\"personal-emr\"")],
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

impl<S> FromRequestParts<S> for ApiKeyContext
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ApiKeyContext>()
            .cloned()
            .ok_or_else(|| unauthorized("missing api key context"))
    }
}
