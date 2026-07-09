//! Registry of per-subject pages. One entry drives **both** the chart's actions
//! row (a button) **and** the route under `/subjects/{id}/{slug}` — so adding a
//! subject page is a single registry line, with no edits to the view or `main.rs`.
//! Auxiliary sub-actions (remove/edit-child/identifiers/…) stay explicit in
//! `main`. Same inversion-of-control as `subject_modules`.

use axum::Router;
use axum::routing::{MethodRouter, get};

use crate::handlers::{self, AppState};

pub struct SubjectPage {
    /// Path suffix under `/subjects/{id}/` — also what the button links to.
    pub slug: &'static str,
    /// The secondary-button label on the subject chart.
    pub label: &'static str,
    /// The page's method router — GET, plus POST where the page also submits.
    pub route: fn() -> MethodRouter<AppState>,
}

/// The ordered registry. Order == button order on the chart.
pub fn pages() -> Vec<SubjectPage> {
    vec![
        SubjectPage {
            slug: "summary",
            label: "Summary (print)",
            route: || get(handlers::subjects::summary),
        },
        SubjectPage {
            slug: "appointments",
            label: "Appointments",
            route: || get(handlers::appointments::list).post(handlers::appointments::create),
        },
        SubjectPage {
            slug: "immunizations",
            label: "Immunizations",
            route: || {
                get(handlers::subjects::immunizations).post(handlers::clinical::add_immunization)
            },
        },
        SubjectPage {
            slug: "vitals",
            label: "Vitals & labs",
            route: || get(handlers::subjects::vitals_labs),
        },
        SubjectPage {
            slug: "care-team",
            label: "Care team & IDs",
            route: || get(handlers::care_team::page).post(handlers::care_team::add_provider),
        },
        SubjectPage {
            slug: "reminders",
            label: "Reminders",
            route: || get(handlers::reminders::page).post(handlers::reminders::add),
        },
        SubjectPage {
            slug: "growth",
            label: "Growth charts",
            route: || get(handlers::subjects::growth),
        },
        SubjectPage {
            slug: "edit",
            label: "Edit profile",
            route: || get(handlers::subjects::edit_form).post(handlers::subjects::edit),
        },
    ]
}

/// A subject-scoped variant of a top-level section: `/records` has a
/// `/subjects/{id}/records`, etc. One entry drives **both** the route and
/// `layout::subject_scoped_url`'s knowledge of which sections scope (the
/// subject switcher + every "View all" link) — so the two can't drift.
pub struct ScopedSection {
    /// The top-level section path this scopes (e.g. "/records").
    pub section: &'static str,
    pub route: fn() -> MethodRouter<AppState>,
}

pub fn scoped_sections() -> Vec<ScopedSection> {
    vec![
        ScopedSection {
            section: "/records",
            route: || get(handlers::records::list_for_subject),
        },
        ScopedSection {
            section: "/incidents",
            route: || get(handlers::incidents::list_for_subject),
        },
        ScopedSection {
            section: "/timeline",
            route: || get(handlers::dashboard::timeline_for_subject),
        },
    ]
}

pub fn is_scoped_section(section: &str) -> bool {
    scoped_sections().iter().any(|s| s.section == section)
}

/// Register every page route (`/subjects/{id}/{slug}`) and scoped-section
/// route (`/subjects/{id}/records`, …) onto `router`.
pub fn register(mut router: Router<AppState>) -> Router<AppState> {
    for p in pages() {
        router = router.route(&format!("/subjects/{{id}}/{}", p.slug), (p.route)());
    }
    for s in scoped_sections() {
        router = router.route(&format!("/subjects/{{id}}{}", s.section), (s.route)());
    }
    router
}
