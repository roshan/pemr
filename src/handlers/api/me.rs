use axum::Json;
use serde_json::{Value, json};

use crate::api_auth::ApiKeyContext;

/// `GET /api/v1/me` — return information about the API key authenticating
/// this request. Useful sanity check for the assistant agent.
pub async fn me(ctx: ApiKeyContext) -> Json<Value> {
    Json(json!({
        "key_id": ctx.key_id,
        "name": ctx.name,
        "owner_subject_id": ctx.owner_subject_id,
    }))
}
