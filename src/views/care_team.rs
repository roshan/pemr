//! Care team + subject identifiers management (PEMR-17). Per-subject page with
//! add/remove for subject_providers and subject_identifiers.

use maud::{Markup, html};
use uuid::Uuid;

use crate::models::{
    Provider, SUBJECT_IDENTIFIER_TYPES, SUBJECT_PROVIDER_ROLES, Source, SubjectIdentifier,
    SubjectProvider, Subject,
};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn provider_name(providers: &[Provider], id: Uuid) -> String {
    providers
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.full_name.clone())
        .unwrap_or_else(|| "?".into())
}

fn source_name(sources: &[Source], id: Uuid) -> String {
    sources
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "?".into())
}

pub fn page(
    nav: &Nav<'_>,
    subject: &Subject,
    care_team: &[SubjectProvider],
    providers: &[Provider],
    identifiers: &[SubjectIdentifier],
    sources: &[Source],
) -> Markup {
    let sid = subject.id;
    let body = html! {
        (c::page_title(format!("{} — care team & identifiers", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{sid}"), "← Back to chart"))

        div class="mt-4 space-y-6" {
            // Care team
            (c::lane(html! { (c::section_heading("Care team")) }, html! {
                @if care_team.is_empty() { (c::empty_state("No providers linked.")) }
                @else {
                    ul class="space-y-1.5" {
                        @for sp in care_team {
                            li class="flex items-baseline justify-between gap-3 text-sm" {
                                div class="flex items-baseline gap-2" {
                                    span class="font-medium" { (provider_name(providers, sp.provider_id)) }
                                    (c::badge_neutral(&sp.role))
                                    @if !sp.active { (c::muted("inactive")) }
                                }
                                (c::post_button(
                                    format!("/subjects/{sid}/care-team/{}/remove", sp.provider_id),
                                    "remove",
                                ))
                            }
                        }
                    }
                }
            }))
            (c::collapse_section("Link a provider", html! {
                @if providers.is_empty() {
                    (c::alert_info("No providers yet — add one in the Providers directory first."))
                } @else {
                    (c::form(format!("/subjects/{sid}/care-team"), "post", html! {
                        (c::field("Provider", c::select_field("provider_id", true, || html! {
                            @for p in providers { (c::select_option(p.id, &p.full_name, false)) }
                        })))
                        (c::field("Role", c::select_field("role", true, || html! {
                            @for r in SUBJECT_PROVIDER_ROLES { (c::select_option(r, *r, *r == "pcp")) }
                        })))
                        (c::field("Since", c::input_date("since", "")))
                        (c::button_primary("Link provider"))
                    }))
                }
            }, care_team.is_empty()))

            // Identifiers
            (c::lane(html! { (c::section_heading("Identifiers (MRN, member ID, …)")) }, html! {
                @if identifiers.is_empty() { (c::empty_state("None recorded.")) }
                @else {
                    ul class="space-y-1.5" {
                        @for idn in identifiers {
                            li class="flex items-baseline justify-between gap-3 text-sm" {
                                div class="flex items-baseline gap-2" {
                                    (c::badge_neutral(&idn.id_type))
                                    span class="font-mono" { (idn.value) }
                                    (c::muted(source_name(sources, idn.source_id)))
                                }
                                (c::post_button(
                                    format!("/subjects/{sid}/identifiers/{}/remove", idn.id),
                                    "remove",
                                ))
                            }
                        }
                    }
                }
            }))
            (c::collapse_section("Add an identifier", html! {
                @if sources.is_empty() {
                    (c::alert_info("No sources yet — add one under Sources first."))
                } @else {
                    (c::form(format!("/subjects/{sid}/identifiers"), "post", html! {
                        (c::field("Source", c::select_field("source_id", true, || html! {
                            @for s in sources { (c::select_option(s.id, &s.name, false)) }
                        })))
                        (c::field("Type", c::select_field("id_type", true, || html! {
                            @for t in SUBJECT_IDENTIFIER_TYPES { (c::select_option(t, *t, *t == "mrn")) }
                        })))
                        (c::field("Value", c::input_text("value", "", true, Some(120))))
                        (c::button_primary("Add identifier"))
                    }))
                }
            }, identifiers.is_empty()))
        }
    };
    shell(nav, body)
}
