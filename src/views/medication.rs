//! Medication detail page (PEMR chart). Shows the full record for one
//! medication — the card's "done/active" rows link here for the complete
//! picture (dose, route, frequency, dates, reason, notes, source).

use maud::{Markup, html};

use crate::models::{Medication, Source, Subject};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub fn detail_page(
    nav: &Nav<'_>,
    subject: &Subject,
    med: &Medication,
    source: Option<&Source>,
) -> Markup {
    let status_cap = med.status.replace('_', " ");
    let dates = match (med.started_on, med.ended_on) {
        (Some(s), Some(e)) => format!("{s} \u{2192} {e}"),
        (Some(s), None) => format!("since {s}"),
        (None, Some(e)) => format!("ended {e}"),
        (None, None) => "dates unknown".to_string(),
    };
    let body = html! {
        (c::card(html! {
            div class="flex items-start justify-between gap-4 mb-2" {
                h1 class="text-2xl font-semibold tracking-tight text-ink" { (med.name) }
                (c::button_link_secondary(format!("/subjects/{}", subject.id), "Back to chart"))
            }
            (c::meta_row(html! {
                (c::badge_neutral(status_cap))
                (c::badge_neutral(dates))
                @if let Some(src) = source {
                    (c::badge_source(html! { "from " (src.name) }))
                    (c::external_link(med.external_url.as_deref()))
                }
            }))

            @if med.dose.is_some() || med.route.is_some() || med.frequency.is_some() {
                dl class="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-3 mt-6 text-sm" {
                    @if let Some(d) = &med.dose {
                        (kv("Dose", d))
                    }
                    @if let Some(r) = &med.route {
                        (kv("Route", r))
                    }
                    @if let Some(f) = &med.frequency {
                        (kv("Frequency", f))
                    }
                    @if let Some(r) = &med.reason {
                        (kv("Reason", r))
                    }
                }
            }

            @if !med.notes.is_empty() {
                (c::subheading("Notes"))
                (c::prose(&med.notes))
            }

            @if source.is_some() {
                (c::subheading("Source"))
                p class="text-sm text-muted" {
                    (c::external_link(med.external_url.as_deref()))
                    @if let Some(eid) = &med.external_id {
                        span class="text-xs text-muted" { "external id " (c::code(eid)) }
                    }
                }
            }
        }))
    };
    shell(nav, body)
}

fn kv(label: &str, value: &str) -> Markup {
    html! {
        div {
            dt class="text-xs text-muted uppercase tracking-wide" { (label) }
            dd class="mt-0.5 text-ink" { (value) }
        }
    }
}
