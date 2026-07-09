use std::sync::OnceLock;

use maud::{DOCTYPE, Markup, html};
use uuid::Uuid;

use crate::models::Subject;
use crate::viewer::ViewerContext;

/// First 12 hex of a vendored asset's sha256 — a content version for cache
/// busting. Read once from the same `static/` dir `ServeDir` serves (relative to
/// cwd, which is the repo root in dev and `/app` in the container). Appending
/// `?v=<hash>` makes the browser fetch a fresh copy whenever the asset's content
/// changes — no manual refresh, no stale CSS after a deploy.
fn asset_ver(path: &str) -> String {
    std::fs::read(path)
        .map(|b| crate::api_auth::sha256_hex(&b)[..12].to_string())
        .unwrap_or_default()
}
fn css_ver() -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| asset_ver("static/vendor/app.css"))
}
fn htmx_ver() -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| asset_ver("static/vendor/htmx.min.js"))
}

/// Top-bar sections: (href, label), in display order.
const NAV_ITEMS: &[(&str, &str)] = &[
    ("/incidents", "Events"),
    ("/records", "Records"),
    ("/sources", "Sources"),
    ("/providers", "Providers"),
    ("/insurance", "Insurance"),
    ("/subjects", "Subjects"),
    ("/settings/api-keys", "API keys"),
    ("/settings/import", "Import"),
];

pub struct Nav<'a> {
    pub title: &'a str,
    pub current_path: &'a str,
    pub subjects: &'a [Subject],
    pub current_subject: Option<Uuid>,
    pub viewer: &'a ViewerContext,
}

pub fn shell(nav: &Nav<'_>, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (nav.title) " — personal EMR" }
                link rel="stylesheet" href={ "/static/vendor/app.css?v=" (css_ver()) };
                script src={ "/static/vendor/htmx.min.js?v=" (htmx_ver()) } defer {}
            }
            body class="min-h-screen text-ink antialiased" {
                (top_bar(nav))
                main class="mx-auto max-w-5xl px-4 py-6" { (body) }
                footer class="mx-auto max-w-5xl px-4 py-6 text-xs text-muted print:hidden" {
                    "personal EMR · "
                    a href="/healthz" class="hover:text-brand" { "healthz" }
                }
            }
        }
    }
}

fn top_bar(nav: &Nav<'_>) -> Markup {
    html! {
        header class="sticky top-0 z-10 bg-surface/90 backdrop-blur border-b border-line print:hidden" {
            div class="mx-auto max-w-5xl px-4 py-3 flex items-center justify-between gap-4" {
                a href="/" class="flex items-center gap-2 text-ink hover:text-brand hover:no-underline" {
                    span class="font-semibold tracking-tight" { "personal-emr" }
                }
                nav class="flex items-center gap-1 text-sm" {
                    (subject_switcher(nav))
                    @for (href, label) in NAV_ITEMS {
                        (nav_link(href, label, nav.current_path))
                    }
                }
            }
            @if let Some(email) = &nav.viewer.email {
                div class="mx-auto max-w-5xl px-4 pb-1 text-right text-xs text-muted" { (email) }
            }
        }
    }
}

fn nav_link(href: &str, label: &str, current_path: &str) -> Markup {
    let active = current_path == href;
    let cls = if active {
        "px-2.5 py-1 rounded-md text-brand-ink bg-brand-soft font-medium"
    } else {
        "px-2.5 py-1 rounded-md text-ink hover:bg-slate-100 hover:no-underline"
    };
    html! { a href=(href) class=(cls) { (label) } }
}

fn subject_switcher(nav: &Nav<'_>) -> Markup {
    let make_url = |sid: Option<Uuid>| -> String { subject_scoped_url(nav.current_path, sid) };
    let label = current_subject_label(nav);
    html! {
        details class="relative" {
            summary
                class="list-none cursor-pointer px-2.5 py-1 rounded-md text-sm font-medium text-ink hover:bg-slate-100 inline-flex items-center gap-1.5" {
                span class="inline-block size-2 rounded-full bg-subject-fg/70" {}
                span { (label.unwrap_or_else(|| "All subjects".to_string())) }
                span class="text-muted text-xs" { "▾" }
            }
            div class="absolute right-0 mt-1 min-w-48 rounded-md border border-line bg-surface shadow-lg p-1 z-20" {
                (switcher_link(&make_url(None), "All subjects", nav.current_subject.is_none()))
                @for s in nav.subjects {
                    (switcher_link(
                        &make_url(Some(s.id)),
                        &format!("{} {}", s.given_name, s.family_name),
                        nav.current_subject == Some(s.id),
                    ))
                }
            }
        }
    }
}

fn switcher_link(href: &str, label: &str, active: bool) -> Markup {
    let cls = if active {
        "block px-2.5 py-1.5 rounded text-sm bg-brand-soft text-brand-ink"
    } else {
        "block px-2.5 py-1.5 rounded text-sm text-ink hover:bg-slate-100 hover:no-underline"
    };
    html! { a href=(href) class=(cls) { (label) } }
}

fn current_subject_label(nav: &Nav<'_>) -> Option<String> {
    let id = nav.current_subject?;
    nav.subjects
        .iter()
        .find(|s| s.id == id)
        .map(|s| format!("{} {}", s.given_name, s.family_name))
}

/// Build a URL for `section` ("/" or a `subject_pages::scoped_sections` entry)
/// scoped to a subject. `None` means the all-subjects view (e.g. `/records`);
/// `Some(id)` returns the path-style version (e.g. `/subjects/<id>/records`).
/// Used by the subject switcher and by every "View all"/"New …" link in the
/// per-section views. Which sections scope comes from the same registry that
/// wires their routes, so this can't drift from `main`.
pub fn subject_scoped_url(section: &str, subject: Option<Uuid>) -> String {
    let rel = match section {
        "/" | "/dashboard" | "" => "",
        s if crate::subject_pages::is_scoped_section(s) => s,
        // Sections we don't subject-scope — return as-is.
        _ => return section.to_string(),
    };
    match subject {
        None => {
            if rel.is_empty() {
                "/".to_string()
            } else {
                rel.to_string()
            }
        }
        Some(id) => format!("/subjects/{id}{rel}"),
    }
}

/// Format a date with the chosen precision; `None` returns "—".
pub fn render_date(d: Option<time::Date>, precision: &str) -> String {
    match d {
        None => "—".into(),
        Some(d) => match precision {
            "year" => d.year().to_string(),
            "month" => format!("{}-{:02}", d.year(), u8::from(d.month())),
            _ => format!("{}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day()),
        },
    }
}

/// Format an event's date span: a single date when there's no end (or the end
/// equals the start), otherwise `start – end`. Used for events (incidents) that
/// may run multiple days, e.g. a hospital stay.
pub fn render_date_range(
    start: Option<time::Date>,
    start_precision: &str,
    end: Option<time::Date>,
    end_precision: &str,
) -> String {
    match end {
        Some(e) if Some(e) != start => {
            format!("{} – {}", render_date(start, start_precision), render_date(Some(e), end_precision))
        }
        _ => render_date(start, start_precision),
    }
}

/// Standalone 404 page. Rendered from the app-level fallback, which runs
/// outside the viewer middleware — so no `Nav`/`ViewerContext` here, just a
/// styled page with a way home.
pub fn not_found_page(path: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Not found — personal EMR" }
                link rel="stylesheet" href={ "/static/vendor/app.css?v=" (css_ver()) };
            }
            body class="min-h-screen text-ink antialiased" {
                main class="mx-auto max-w-5xl px-4 py-16" {
                    h1 class="text-2xl font-semibold tracking-tight text-ink mb-2" { "Page not found" }
                    p class="text-sm text-muted mb-4" {
                        code { (path) } " doesn't exist."
                    }
                    a href="/" class="text-brand hover:underline" { "← Back to the dashboard" }
                }
            }
        }
    }
}

pub fn subject_badge(subjects: &[Subject], id: Uuid) -> Markup {
    let name = subjects
        .iter()
        .find(|s| s.id == id)
        .map(|s| format!("{} {}", s.given_name, s.family_name))
        .unwrap_or_else(|| "?".to_string());
    crate::views::components::badge_subject(name)
}
