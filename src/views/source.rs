use maud::{Markup, html};

use crate::models::{
    Incident, Record, SOURCE_KINDS, Source, Subject, record_kind_label, source_kind_label,
};
use crate::views::components as c;
use crate::views::layout::{Nav, render_date, shell, subject_badge};

pub fn list_page(nav: &Nav<'_>, sources: &[Source]) -> Markup {
    let body = html! {
        (c::page_title("Sources"))
        p class="text-sm text-muted mb-4" {
            "Where the data came from. MyChart instances, Quest portal, individual clinics, etc."
        }

        @if sources.is_empty() {
            (c::empty_state("No sources yet."))
        } @else {
            (c::data_table(
                html! { tr { (c::th("Name")) (c::th("Kind")) (c::th("URL")) } },
                html! {
                    @for s in sources {
                        tr class="hover:bg-slate-50" {
                            (c::td(html! { a href={ "/sources/" (s.id) } class="font-medium" { (s.name) } }))
                            (c::td(html! { (c::badge_source(source_kind_label(&s.kind))) }))
                            (c::td(html! {
                                @if let Some(u) = &s.base_url {
                                    a href=(u) target="_blank" rel="noreferrer" { (u) }
                                } @else { "—" }
                            }))
                        }
                    }
                },
            ))
        }

        div class="mt-6" {
            (c::collapse_section("Add a source", html! {
                (c::form("/sources", "post", html! {
                    (c::field("Name", c::input_text("name", "", true, Some(120))))
                    (c::field("Kind", c::select_field("kind", true, || html! {
                        @for k in SOURCE_KINDS {
                            (c::select_option(k, source_kind_label(k), false))
                        }
                    })))
                    (c::field_with_hint(
                        "Base URL",
                        "Optional — the public URL of this portal/clinic.",
                        c::input_url("base_url", ""),
                    ))
                    (c::field("Notes", c::textarea_field("notes", "", 3)))
                    (c::button_primary("Add source"))
                }))
            }, sources.is_empty()))
        }
    };
    shell(nav, body)
}

pub fn detail_page(
    nav: &Nav<'_>,
    source: &Source,
    incidents: &[Incident],
    records: &[Record],
    subjects: &[Subject],
) -> Markup {
    let body = html! {
        (c::card(html! {
            h1 class="text-2xl font-semibold tracking-tight text-ink mb-2" { (source.name) }
            (c::meta_row(html! {
                (c::badge_source(source_kind_label(&source.kind)))
                @if let Some(u) = &source.base_url {
                    a href=(u) target="_blank" rel="noreferrer" class="text-xs" { (u) }
                }
            }))
            @if !source.notes.is_empty() {
                div class="mt-2" { (c::prose(&source.notes)) }
            }
        }))

        (c::lane(
            html! { (c::section_heading("Incidents from this source")) },
            html! {
                @if incidents.is_empty() { (c::empty_state("None.")) }
                @else {
                    ul class="space-y-1.5" {
                        @for inc in incidents {
                            li class="flex items-center gap-2 text-sm" {
                                span class="text-xs text-muted w-24" { (render_date(inc.occurred_at, &inc.occurred_precision)) }
                                (subject_badge(subjects, inc.subject_id))
                                a href={ "/incidents/" (inc.id) } { (inc.title) }
                            }
                        }
                    }
                }
            },
        ))
        (c::lane(
            html! { (c::section_heading("Records from this source")) },
            html! {
                @if records.is_empty() { (c::empty_state("None.")) }
                @else {
                    ul class="space-y-1.5" {
                        @for rec in records {
                            li class="flex items-center gap-2 text-sm" {
                                span class="text-xs text-muted w-24" { (render_date(rec.occurred_at, &rec.occurred_precision)) }
                                (subject_badge(subjects, rec.subject_id))
                                (c::badge_kind(record_kind_label(&rec.kind)))
                                a href={ "/records/" (rec.id) } { (rec.title) }
                            }
                        }
                    }
                }
            },
        ))
    };
    shell(nav, body)
}
