use axum::Json;
use serde_json::{Value, json};

use crate::api_auth::ApiKeyContext;

/// `GET /api/v1` — discovery doc for the assistant agent.
pub async fn index(_ctx: ApiKeyContext) -> Json<Value> {
    Json(json!({
        "name": "personal-emr",
        "version": "v1",
        "description": "Read-only JSON API for the personal EMR. Authenticate with `Authorization: Bearer <token>` obtained from /settings/api-keys in the UI.",
        "endpoints": [
            {"method": "GET", "path": "/api/v1",                              "summary": "this discovery document"},
            {"method": "GET", "path": "/api/v1/me",                           "summary": "which API key this request is authenticated as"},
            {"method": "GET", "path": "/api/v1/subjects",                     "summary": "list subjects (people)"},
            {"method": "GET", "path": "/api/v1/subjects/{id}",                "summary": "one subject"},
            {"method": "GET", "path": "/api/v1/incidents",                    "summary": "list incidents (?subject=<uuid> to filter)"},
            {"method": "GET", "path": "/api/v1/incidents/{id}",               "summary": "one incident + linked records"},
            {"method": "GET", "path": "/api/v1/records",                      "summary": "list records (?subject=<uuid> &kind=<kind>)"},
            {"method": "GET", "path": "/api/v1/records/{id}",                 "summary": "one record (metadata)"},
            {"method": "GET", "path": "/api/v1/records/{id}/file",            "summary": "stream the original file bytes"},
            {"method": "GET", "path": "/api/v1/records/{id}/preview",         "summary": "stream the preview (DICOM → PNG)"},
            {"method": "GET", "path": "/api/v1/records/{id}/thumbnail",       "summary": "stream the thumbnail (webp)"},
            {"method": "GET", "path": "/api/v1/sources",                      "summary": "list sources (EMR portals, clinics)"},
            {"method": "GET", "path": "/api/v1/sources/{id}",                 "summary": "one source"},
            {"method": "GET", "path": "/api/v1/search?q=...&subject=...",     "summary": "full-text search across incidents + records"},
        ],
        "record_kinds": crate::models::RECORD_KINDS,
        "source_kinds": crate::models::SOURCE_KINDS,
    }))
}
