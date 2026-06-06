//! Providers directory UI (PEMR-17). Shared clinician reference data.

use maud::{Markup, html};
use uuid::Uuid;

use crate::models::{Provider, Source};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn facility_name(sources: &[Source], id: Option<Uuid>) -> String {
    match id {
        None => "—".into(),
        Some(fid) => sources
            .iter()
            .find(|s| s.id == fid)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "—".into()),
    }
}

fn facility_select(name: &str, sources: &[Source], selected: Option<Uuid>) -> Markup {
    c::select_field(name, false, || {
        html! {
            (c::select_option("", "— none —", selected.is_none()))
            @for s in sources {
                (c::select_option(s.id, &s.name, selected == Some(s.id)))
            }
        }
    })
}

pub fn list_page(nav: &Nav<'_>, providers: &[Provider], sources: &[Source]) -> Markup {
    let body = html! {
        (c::page_title("Providers"))
        p class="text-sm text-muted mb-4" {
            "Clinicians (shared reference data, not tied to one subject). Link them to a subject as care team from the subject's chart."
        }

        @if providers.is_empty() {
            (c::empty_state("No providers yet."))
        } @else {
            (c::data_table(
                html! { tr {
                    (c::th("Name")) (c::th("Specialty")) (c::th("NPI")) (c::th("Facility")) (c::th(""))
                } },
                html! {
                    @for p in providers {
                        tr class="hover:bg-slate-50" {
                            (c::td(html! { span class="font-medium" { (p.full_name) } }))
                            (c::td(html! { (p.specialty.clone().unwrap_or_else(|| "—".into())) }))
                            (c::td(html! { (p.npi.clone().unwrap_or_else(|| "—".into())) }))
                            (c::td(html! { (facility_name(sources, p.facility_id)) }))
                            (c::td(html! { a href={ "/providers/" (p.id) "/edit" } { "edit" } }))
                        }
                    }
                },
            ))
        }

        div class="mt-6" {
            (c::collapse_section("Add a provider", html! {
                (c::form("/providers", "post", html! {
                    (c::field("Full name", c::input_text("full_name", "", true, Some(120))))
                    (c::field("Specialty", c::input_text("specialty", "", false, Some(80))))
                    (c::field_with_hint("NPI", "National Provider Identifier (the global dedup key).",
                        c::input_text("npi", "", false, Some(20))))
                    (c::field("Facility", facility_select("facility_id", sources, None)))
                    (c::field("Phone", c::input_text("phone", "", false, Some(40))))
                    (c::field("Email", c::input_email("email", "", None)))
                    (c::field("Notes", c::textarea_field("notes", "", 3)))
                    (c::button_primary("Add provider"))
                }))
            }, providers.is_empty()))
        }
    };
    shell(nav, body)
}

pub fn edit_form(nav: &Nav<'_>, p: &Provider, sources: &[Source], error: Option<&str>) -> Markup {
    let body = html! {
        (c::page_title("Edit provider"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::form(format!("/providers/{}/edit", p.id), "post", html! {
            (c::field("Full name", c::input_text("full_name", &p.full_name, true, Some(120))))
            (c::field("Specialty", c::input_text("specialty", &p.specialty.clone().unwrap_or_default(), false, Some(80))))
            (c::field("NPI", c::input_text("npi", &p.npi.clone().unwrap_or_default(), false, Some(20))))
            (c::field("Facility", facility_select("facility_id", sources, p.facility_id)))
            (c::field("Phone", c::input_text("phone", &p.phone.clone().unwrap_or_default(), false, Some(40))))
            (c::field("Email", c::input_email("email", &p.email.clone().unwrap_or_default(), None)))
            (c::field("Notes", c::textarea_field("notes", &p.notes, 4)))
            div class="flex gap-2" {
                (c::button_primary("Save"))
                (c::button_link_secondary("/providers", "Cancel"))
            }
        }))
    };
    shell(nav, body)
}
