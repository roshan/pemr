//! Registry of the `/api/v1` surface. One entry per path drives **both** the
//! router wiring (`register`, called from `main.rs`) **and** the discovery
//! document at `GET /api/v1` (`handlers::api::root` iterates `routes()`) — so
//! an endpoint can't ship undocumented and the doc can't drift from the
//! routes. Same inversion of control as `subject_pages`. Registry order is
//! the discovery doc's display order.

use axum::Router;
use axum::routing::{MethodRouter, get, post};

use crate::handlers::AppState;
use crate::handlers::api;

pub struct ApiRoute {
    /// Route path (axum syntax, e.g. `/api/v1/records/{id}`).
    pub path: &'static str,
    /// Discovery-doc display path, when it should show query params the route
    /// path can't (`None` = same as `path`).
    pub doc_path: Option<&'static str>,
    /// The path's method router — GET, POST, or both.
    pub route: fn() -> MethodRouter<AppState>,
    /// `(method, summary)` rows for the discovery doc.
    pub docs: &'static [(&'static str, &'static str)],
}

/// The ordered registry. POST on the clinical resources is an idempotent
/// upsert (see `handlers::api::mod`); adding PUT/PATCH/DELETE or writes on
/// non-clinical resources is a fresh scope decision — see CLAUDE.md.
pub fn routes() -> Vec<ApiRoute> {
    vec![
        ApiRoute {
            path: "/api/v1",
            doc_path: None,
            route: || get(api::root::index),
            docs: &[("GET", "this discovery document")],
        },
        ApiRoute {
            path: "/api/v1/me",
            doc_path: None,
            route: || get(api::me::me),
            docs: &[("GET", "which API key this request is authenticated as")],
        },
        ApiRoute {
            path: "/api/v1/subjects",
            doc_path: None,
            route: || get(api::subjects::list),
            docs: &[("GET", "list subjects (people)")],
        },
        ApiRoute {
            path: "/api/v1/subjects/{id}",
            doc_path: None,
            route: || get(api::subjects::detail),
            docs: &[("GET", "one subject")],
        },
        ApiRoute {
            path: "/api/v1/incidents",
            doc_path: None,
            route: || get(api::incidents::list).post(api::incidents::create),
            docs: &[
                ("GET", "list incidents (?subject=)"),
                (
                    "POST",
                    "upsert an incident/event (req: subject_id, title); no provenance key — \
                     dedup is on content: (subject_id, lower(title), occurred_at)",
                ),
            ],
        },
        ApiRoute {
            path: "/api/v1/incidents/{id}",
            doc_path: None,
            route: || get(api::incidents::detail),
            docs: &[("GET", "one incident + linked records")],
        },
        ApiRoute {
            path: "/api/v1/records",
            doc_path: None,
            route: || get(api::records::list).post(api::records::create),
            docs: &[
                ("GET", "list records (?subject=&kind=)"),
                (
                    "POST",
                    "upsert a record; multipart/form-data (req: subject_id, kind, title; \
                     optional file part + link_incident); dedup on (source_id, external_id)",
                ),
            ],
        },
        ApiRoute {
            path: "/api/v1/records/{id}",
            doc_path: None,
            route: || get(api::records::detail),
            docs: &[("GET", "one record (metadata)")],
        },
        ApiRoute {
            path: "/api/v1/records/{id}/file",
            doc_path: None,
            route: || get(api::records::file),
            docs: &[("GET", "stream the original file bytes")],
        },
        ApiRoute {
            path: "/api/v1/records/{id}/preview",
            doc_path: None,
            route: || get(api::records::preview),
            docs: &[("GET", "stream the preview (DICOM → PNG)")],
        },
        ApiRoute {
            path: "/api/v1/records/{id}/thumbnail",
            doc_path: None,
            route: || get(api::records::thumbnail),
            docs: &[("GET", "stream the thumbnail (webp)")],
        },
        ApiRoute {
            path: "/api/v1/sources",
            doc_path: None,
            route: || get(api::sources::list).post(api::sources::create),
            docs: &[
                ("GET", "list sources (EMR portals, clinics)"),
                ("POST", "create/update a source (clinic, portal); dedup on name"),
            ],
        },
        ApiRoute {
            path: "/api/v1/sources/{id}",
            doc_path: None,
            route: || get(api::sources::detail),
            docs: &[("GET", "one source")],
        },
        ApiRoute {
            path: "/api/v1/providers",
            doc_path: None,
            route: || get(api::providers::list).post(api::providers::create),
            docs: &[
                ("GET", "list clinicians (reference data; not subject-scoped)"),
                ("POST", "upsert a clinician; dedup on npi, else (source_id, external_id)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/providers/{id}",
            doc_path: None,
            route: || get(api::providers::detail),
            docs: &[("GET", "one provider")],
        },
        ApiRoute {
            path: "/api/v1/appointments",
            doc_path: None,
            route: || get(api::appointments::list).post(api::appointments::create),
            docs: &[
                ("GET", "list appointments (?subject=)"),
                ("POST", "upsert an appointment (req: subject_id, title, starts_at)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/appointments/{id}",
            doc_path: None,
            route: || get(api::appointments::detail),
            docs: &[("GET", "one appointment")],
        },
        ApiRoute {
            path: "/api/v1/allergies",
            doc_path: None,
            route: || get(api::allergies::list).post(api::allergies::create),
            docs: &[
                ("GET", "list allergies (?subject=)"),
                ("POST", "upsert an allergy (req: subject_id, substance)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/allergies/{id}",
            doc_path: None,
            route: || get(api::allergies::detail),
            docs: &[("GET", "one allergy")],
        },
        ApiRoute {
            path: "/api/v1/medications",
            doc_path: None,
            route: || get(api::medications::list).post(api::medications::create),
            docs: &[
                ("GET", "list medications (?subject=)"),
                ("POST", "upsert a medication (req: subject_id, name)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/medications/{id}",
            doc_path: None,
            route: || get(api::medications::detail),
            docs: &[("GET", "one medication")],
        },
        ApiRoute {
            path: "/api/v1/conditions",
            doc_path: None,
            route: || get(api::conditions::list).post(api::conditions::create),
            docs: &[
                ("GET", "list conditions / problem list (?subject=)"),
                ("POST", "upsert a condition (req: subject_id, name)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/conditions/{id}",
            doc_path: None,
            route: || get(api::conditions::detail),
            docs: &[("GET", "one condition")],
        },
        ApiRoute {
            path: "/api/v1/immunizations",
            doc_path: None,
            route: || get(api::immunizations::list).post(api::immunizations::create),
            docs: &[
                ("GET", "list immunizations (?subject=)"),
                ("POST", "upsert an immunization (req: subject_id, vaccine)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/immunizations/{id}",
            doc_path: None,
            route: || get(api::immunizations::detail),
            docs: &[("GET", "one immunization")],
        },
        ApiRoute {
            path: "/api/v1/observations",
            doc_path: None,
            route: || get(api::observations::list).post(api::observations::create),
            docs: &[
                ("GET", "list vitals + labs (?subject=&code=<LOINC>)"),
                (
                    "POST",
                    "upsert an observation (req: subject_id, display, effective_on); \
                     BP = two rows; growth vitals use canonical LOINC",
                ),
            ],
        },
        ApiRoute {
            path: "/api/v1/observations/{id}",
            doc_path: None,
            route: || get(api::observations::detail),
            docs: &[("GET", "one observation")],
        },
        ApiRoute {
            path: "/api/v1/care-reminders",
            doc_path: None,
            route: || get(api::care_reminders::list).post(api::care_reminders::create),
            docs: &[
                ("GET", "list care reminders / what's due (?subject=)"),
                ("POST", "create a care reminder (req: subject_id, title)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/care-reminders/{id}",
            doc_path: None,
            route: || get(api::care_reminders::detail),
            docs: &[("GET", "one care reminder")],
        },
        ApiRoute {
            path: "/api/v1/subject-identifiers",
            doc_path: None,
            route: || get(api::subject_identifiers::list).post(api::subject_identifiers::create),
            docs: &[
                ("GET", "list cross-system identifiers / MRNs (?subject=)"),
                ("POST", "upsert an identifier (req: subject_id, source_id, value)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/subject-identifiers/{id}",
            doc_path: None,
            route: || get(api::subject_identifiers::detail),
            docs: &[("GET", "one identifier")],
        },
        ApiRoute {
            path: "/api/v1/subject-providers",
            doc_path: None,
            route: || get(api::subject_providers::list).post(api::subject_providers::create),
            docs: &[
                ("GET", "list care-team links (?subject=)"),
                ("POST", "upsert a care-team link (req: subject_id, provider_id)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/subject-relationships",
            doc_path: None,
            route: || get(api::subject_relationships::list).post(api::subject_relationships::create),
            docs: &[
                ("GET", "list family graph edges (?subject=)"),
                (
                    "POST",
                    "upsert a relationship (req: subject_id, related_subject_id, relationship)",
                ),
            ],
        },
        ApiRoute {
            path: "/api/v1/insurance-plans",
            doc_path: None,
            route: || get(api::insurance_plans::list).post(api::insurance_plans::create),
            docs: &[
                ("GET", "list insurance cards/policies (reference data; not subject-scoped)"),
                ("POST", "upsert an insurance plan (req: payer_name); dedup on (source_id, external_id)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/insurance-plans/{id}",
            doc_path: None,
            route: || get(api::insurance_plans::detail),
            docs: &[("GET", "one insurance plan")],
        },
        ApiRoute {
            path: "/api/v1/subject-insurance",
            doc_path: None,
            route: || get(api::subject_insurance::list).post(api::subject_insurance::create),
            docs: &[
                ("GET", "list coverage links / who's on which card (?subject=)"),
                ("POST", "upsert a coverage link (req: subject_id, plan_id)"),
            ],
        },
        ApiRoute {
            path: "/api/v1/search",
            doc_path: Some("/api/v1/search?q=...&subject=..."),
            route: || get(api::search::search),
            docs: &[("GET", "full-text search across incidents + records")],
        },
        ApiRoute {
            path: "/api/v1/import/fhir",
            doc_path: Some("/api/v1/import/fhir?subject=<uuid>&source=<name>"),
            route: || post(api::import::fhir),
            docs: &[(
                "POST",
                "import a FHIR R4 Bundle (Apple Health clinical records / Epic FHIR export); \
                 idempotent on resource id",
            )],
        },
        ApiRoute {
            path: "/api/v1/import/ccda",
            doc_path: Some("/api/v1/import/ccda?subject=<uuid>&source=<name>"),
            route: || post(api::import::ccda),
            docs: &[(
                "POST",
                "import a C-CDA XML document (MyChart 'Download My Record'); body is the raw XML",
            )],
        },
    ]
}

/// Register every endpoint's route onto `router`.
pub fn register(mut router: Router<AppState>) -> Router<AppState> {
    for r in routes() {
        router = router.route(r.path, (r.route)());
    }
    router
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_unique_and_documented() {
        let rs = routes();
        for (i, r) in rs.iter().enumerate() {
            assert!(
                rs.iter().skip(i + 1).all(|o| o.path != r.path),
                "duplicate api path: {}",
                r.path
            );
            assert!(!r.docs.is_empty(), "undocumented api path: {}", r.path);
        }
    }
}
