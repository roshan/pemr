use maud::{Markup, html};

use crate::models::{Incident, Record, Subject, record_kind_label};
use crate::views::components as c;
use crate::views::layout::{render_date, subject_badge};

pub struct SearchResults<'a> {
    pub query: &'a str,
    pub incidents: &'a [Incident],
    pub records: &'a [Record],
    pub subjects: &'a [Subject],
}

/// HTMX partial — replaces the contents of `#results` on the dashboard.
pub fn results_partial(r: &SearchResults<'_>) -> Markup {
    let q = r.query.trim();
    if q.is_empty() {
        return html! {};
    }
    html! {
        @if r.incidents.is_empty() && r.records.is_empty() {
            (c::empty_state(html! { "No matches for " (c::code(q)) "." }))
        }
        @if !r.incidents.is_empty() {
            (c::subheading("Events"))
            ul class="space-y-1.5" {
                @for inc in r.incidents {
                    li class="flex items-center gap-2 text-sm" {
                        a href={ "/incidents/" (inc.id) } class="font-medium" { (inc.title) }
                        (subject_badge(r.subjects, inc.subject_id))
                        span class="text-xs text-muted" {
                            (render_date(inc.occurred_at, &inc.occurred_precision))
                        }
                    }
                }
            }
        }
        @if !r.records.is_empty() {
            (c::subheading("Records"))
            ul class="space-y-1.5" {
                @for rec in r.records {
                    li class="flex items-center gap-2 text-sm" {
                        a href={ "/records/" (rec.id) } class="font-medium" { (rec.title) }
                        (subject_badge(r.subjects, rec.subject_id))
                        (c::badge_kind(record_kind_label(&rec.kind)))
                        span class="text-xs text-muted" {
                            (render_date(rec.occurred_at, &rec.occurred_precision))
                        }
                    }
                }
            }
        }
    }
}
