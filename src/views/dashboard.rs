use maud::{Markup, html};

use crate::models::{Incident, Record, Subject, is_image_kind, record_kind_label};
use crate::views::components as c;
use crate::views::layout::{Nav, render_date, shell, subject_badge};

pub struct DashboardData<'a> {
    pub subjects: &'a [Subject],
    pub timeline_incidents: &'a [Incident],
    pub timeline_total: i64,
    pub recent_incidents: &'a [Incident],
    pub recent_records: &'a [Record],
}

const DASHBOARD_TIMELINE_LIMIT: usize = 12;

pub fn render(nav: &Nav<'_>, data: &DashboardData<'_>) -> Markup {
    shell(nav, body(nav, data))
}

/// Inner body markup (search bar, timeline, recent lanes, action shortcuts).
/// Reusable so the per-subject dashboard at `/subjects/{id}` can wrap it
/// with a bio header.
pub fn body(nav: &Nav<'_>, data: &DashboardData<'_>) -> Markup {
    html! {
        section class="mb-6" {
            form action="/" method="get" class="space-y-2" {
                (c::input_search(
                    "q",
                    "Search incidents and records…",
                    "/search",
                    "#results",
                    "[name=subject]",
                ))
                @if let Some(id) = nav.current_subject {
                    input type="hidden" name="subject" value=(id);
                }
            }
            div #results class="mt-3" {}
        }

        (incidents_timeline(nav.current_subject, data.timeline_incidents, data.timeline_total))

        (c::lane(
            html! {
                (c::section_heading("Recent incidents"))
                a href=(list_url("/incidents", nav.current_subject))
                  class="text-xs text-muted hover:text-brand" { "View all →" }
            },
            html! {
                @if data.recent_incidents.is_empty() {
                    (c::empty_state(html! {
                        "No incidents yet. "
                        a href=(new_incident_url(nav)) class="text-brand" { "Add one" } "."
                    }))
                } @else {
                    (c::card_grid(html! {
                        @for inc in data.recent_incidents {
                            (c::card(html! {
                                (c::card_title(format!("/incidents/{}", inc.id), &inc.title))
                                (c::meta_row(html! {
                                    (subject_badge(data.subjects, inc.subject_id))
                                    (c::badge_neutral(render_date(inc.occurred_at, &inc.occurred_precision)))
                                }))
                                @if !inc.narrative.is_empty() {
                                    p class="mt-2 text-sm text-ink/80 line-clamp-3" { (inc.narrative) }
                                }
                            }))
                        }
                    }))
                }
            },
        ))

        (c::lane(
            html! {
                (c::section_heading("Recent records"))
                a href=(list_url("/records", nav.current_subject))
                  class="text-xs text-muted hover:text-brand" { "View all →" }
            },
            html! {
                @if data.recent_records.is_empty() {
                    (c::empty_state(html! {
                        "No records yet. "
                        a href=(new_record_url(nav)) class="text-brand" { "Add one" } "."
                    }))
                } @else {
                    (c::card_grid(html! {
                        @for rec in data.recent_records {
                            (record_card(rec, data.subjects))
                        }
                    }))
                }
            },
        ))

        section class="flex flex-wrap gap-2 pt-2" {
            (c::button_link_primary(new_incident_url(nav), "New incident"))
            (c::button_link_secondary(new_record_url(nav), "New record"))
            (c::button_link_secondary("/sources", "Manage sources"))
        }
    }
}

fn list_url(base: &str, subject: Option<uuid::Uuid>) -> String {
    use crate::views::layout::subject_scoped_url;
    subject_scoped_url(base, subject)
}
fn new_incident_url(nav: &Nav<'_>) -> String {
    match nav.current_subject {
        Some(id) => format!("/incidents/new?subject={id}"),
        None => "/incidents/new".to_string(),
    }
}
fn new_record_url(nav: &Nav<'_>) -> String {
    match nav.current_subject {
        Some(id) => format!("/records/new?subject={id}"),
        None => "/records/new".to_string(),
    }
}

/// A "Recent records" card. For image-kind records that have a stored
/// thumbnail we lead with the picture (full-width banner across the top of
/// the card); other records just show title + meta.
fn record_card(rec: &Record, subjects: &[Subject]) -> Markup {
    let detail_url = format!("/records/{}", rec.id);
    let thumb_url = if rec.thumbnail_path.is_some() && is_image_kind(&rec.kind) {
        Some(format!("/records/{}/thumbnail", rec.id))
    } else {
        None
    };
    html! {
        article class="rounded-lg border border-line bg-surface shadow-xs overflow-hidden flex flex-col" {
            @if let Some(url) = thumb_url {
                a href=(detail_url) class="block hover:no-underline" {
                    img src=(url) alt=(rec.title) loading="lazy"
                        class="block w-full h-40 object-cover bg-slate-900";
                }
            }
            div class="p-4" {
                (c::card_title(detail_url, &rec.title))
                (c::meta_row(html! {
                    (subject_badge(subjects, rec.subject_id))
                    (c::badge_kind(record_kind_label(&rec.kind)))
                    (c::badge_neutral(render_date(rec.occurred_at, &rec.occurred_precision)))
                }))
            }
        }
    }
}

/// Compact horizontal timeline — dots on a line with dates underneath.
/// Shows the most recent `DASHBOARD_TIMELINE_LIMIT` incidents; if there are
/// more, an "Expand timeline →" link takes the user to the dedicated page.
fn incidents_timeline(
    current_subject: Option<uuid::Uuid>,
    incidents: &[Incident],
    total: i64,
) -> Markup {
    if incidents.is_empty() {
        return html! {};
    }
    // Oldest → newest reads left-to-right.
    let mut sorted: Vec<&Incident> = incidents.iter().collect();
    sorted.sort_by(|a, b| a.occurred_at.cmp(&b.occurred_at).then(a.created_at.cmp(&b.created_at)));

    let expand_url = crate::views::layout::subject_scoped_url("/timeline", current_subject);
    let has_more = total as usize > sorted.len();

    html! {
        section class="mb-6" {
            div class="flex items-baseline justify-between mb-2 gap-3" {
                (c::section_heading("Timeline"))
                a href=(expand_url) class="text-xs text-muted hover:text-brand" {
                    @if has_more { "Expand timeline (" (total) ") →" }
                    @else        { "Expand timeline →" }
                }
            }
            div class="relative overflow-x-auto rounded-lg border border-line bg-surface px-3 py-3" {
                div class="absolute timeline-line-top left-3 right-3 h-px bg-line" {}
                ol class="relative flex items-start gap-5 min-w-max" {
                    @for inc in &sorted {
                        li {
                            a href={ "/incidents/" (inc.id) }
                              title=(inc.title)
                              class="group flex flex-col items-center" {
                                span class="block w-3 h-3 rounded-full bg-brand group-hover:bg-indigo-800 ring-2 ring-surface transition-colors" {}
                                time class="text-xs text-muted mt-2 whitespace-nowrap group-hover:text-ink" {
                                    (render_date(inc.occurred_at, &inc.occurred_precision))
                                }
                                span class="text-xs text-ink mt-0.5 max-w-32 truncate group-hover:text-brand" {
                                    (inc.title)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn full_timeline(nav: &Nav<'_>, incidents: &[Incident], subjects: &[Subject]) -> Markup {
    let body = html! {
        (c::page_title("Timeline"))
        @if incidents.is_empty() {
            (c::empty_state("No incidents yet."))
        } @else {
            ol class="relative space-y-4 ml-3 border-l border-line pl-6" {
                @for inc in incidents {
                    li class="relative" {
                        span class="absolute timeline-marker-offset top-1.5 w-3 h-3 rounded-full bg-brand ring-2 ring-canvas" {}
                        a href={ "/incidents/" (inc.id) } class="block group rounded -mx-2 px-2 py-1 hover:bg-brand-soft" {
                            div class="flex flex-wrap items-baseline gap-2" {
                                time class="text-xs font-mono text-muted" {
                                    (render_date(inc.occurred_at, &inc.occurred_precision))
                                }
                                span class="text-base font-semibold text-ink group-hover:text-brand" {
                                    (inc.title)
                                }
                                (subject_badge(subjects, inc.subject_id))
                            }
                            @if !inc.narrative.is_empty() {
                                p class="text-sm text-muted mt-1 line-clamp-2" { (inc.narrative) }
                            }
                        }
                    }
                }
            }
        }
    };
    shell(nav, body)
}

pub fn dashboard_timeline_limit() -> usize { DASHBOARD_TIMELINE_LIMIT }
