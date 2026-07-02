//! Insurance directory UI. An insurance card/policy is shared reference data (a
//! family shares one card, like providers/sources), so it lives at a top-level
//! `/insurance` directory; covered people are linked per-plan on the detail page.

use maud::{Markup, html};
use uuid::Uuid;

use crate::models::{
    INSURANCE_PLAN_KINDS, INSURANCE_PLAN_TYPES, INSURANCE_RELATIONSHIPS, InsurancePlan, Source,
    Subject, SubjectInsurance,
};
use crate::views::components as c;
use crate::views::layout::{Nav, render_date, shell};

fn plan_type_label(t: &str) -> String {
    match t {
        "medicare" => "Medicare".into(),
        "medicaid" => "Medicaid".into(),
        "tricare" => "Tricare".into(),
        "other" => "Other".into(),
        other => other.to_uppercase(),
    }
}

fn plan_kind_label(k: &str) -> &'static str {
    match k {
        "medical" => "Medical",
        "dental" => "Dental",
        "vision" => "Vision",
        "pharmacy" => "Pharmacy",
        _ => "Other",
    }
}

fn plan_type_select(name: &str, selected: Option<&str>) -> Markup {
    c::select_field(name, false, || {
        html! {
            (c::select_option("", "— unspecified —", selected.is_none()))
            @for t in INSURANCE_PLAN_TYPES {
                (c::select_option(t, plan_type_label(t), selected == Some(*t)))
            }
        }
    })
}

fn plan_kind_select(name: &str, selected: &str) -> Markup {
    c::select_field(name, true, || {
        html! {
            @for k in INSURANCE_PLAN_KINDS {
                (c::select_option(k, plan_kind_label(k), selected == *k))
            }
        }
    })
}

fn source_select(name: &str, sources: &[Source], selected: Option<Uuid>) -> Markup {
    c::select_field(name, false, || {
        html! {
            (c::select_option("", "— none —", selected.is_none()))
            @for s in sources {
                (c::select_option(s.id, &s.name, selected == Some(s.id)))
            }
        }
    })
}

/// The add/edit field set, shared by the create form and the edit form.
fn plan_fields(p: Option<&InsurancePlan>, sources: &[Source]) -> Markup {
    let v = |f: fn(&InsurancePlan) -> Option<String>| -> String {
        p.and_then(|p| f(p)).unwrap_or_default()
    };
    html! {
        (c::field("Payer / carrier", c::input_text("payer_name",
            &p.map(|p| p.payer_name.clone()).unwrap_or_default(), true, Some(120))))
        (c::field("Plan name", c::input_text("plan_name", &v(|p| p.plan_name.clone()), false, Some(120))))
        (c::field("Coverage kind", plan_kind_select("plan_kind",
            p.map(|p| p.plan_kind.as_str()).unwrap_or("medical"))))
        (c::field("Plan type", plan_type_select("plan_type", p.and_then(|p| p.plan_type.as_deref()))))
        (c::field("Member / subscriber ID", c::input_text("member_id", &v(|p| p.member_id.clone()), false, Some(60))))
        (c::field("Group number", c::input_text("group_number", &v(|p| p.group_number.clone()), false, Some(60))))
        (c::field("Subscriber (policyholder) name", c::input_text("subscriber_name",
            &v(|p| p.subscriber_name.clone()), false, Some(120))))
        (c::field("Rx BIN", c::input_text("rx_bin", &v(|p| p.rx_bin.clone()), false, Some(20))))
        (c::field("Rx PCN", c::input_text("rx_pcn", &v(|p| p.rx_pcn.clone()), false, Some(20))))
        (c::field("Rx group", c::input_text("rx_group", &v(|p| p.rx_group.clone()), false, Some(20))))
        (c::field("Member-services phone", c::input_text("payer_phone", &v(|p| p.payer_phone.clone()), false, Some(40))))
        (c::field("Effective date", c::input_date("effective_date",
            &p.and_then(|p| p.effective_date).map(|d| d.to_string()).unwrap_or_default())))
        (c::field("Expiration date", c::input_date("expiration_date",
            &p.and_then(|p| p.expiration_date).map(|d| d.to_string()).unwrap_or_default())))
        (c::field_with_hint("Source", "Optional — the portal/payer system this was synced from.",
            source_select("source_id", sources, p.and_then(|p| p.source_id))))
        (c::field("Notes", c::textarea_field("notes", p.map(|p| p.notes.as_str()).unwrap_or(""), 3)))
    }
}

pub fn list_page(
    nav: &Nav<'_>,
    plans: &[InsurancePlan],
    coverage_counts: &[(Uuid, i64)],
    sources: &[Source],
) -> Markup {
    let count_for = |id: Uuid| coverage_counts.iter().find(|(pid, _)| *pid == id).map(|(_, n)| *n).unwrap_or(0);
    let body = html! {
        (c::page_title("Insurance"))
        p class="text-sm text-muted mb-4" {
            "Insurance cards / policies (shared — one card can cover the whole family). Open a plan to add the people it covers."
        }

        @if plans.is_empty() {
            (c::empty_state("No insurance plans yet."))
        } @else {
            (c::data_table(
                html! { tr {
                    (c::th("Payer")) (c::th("Plan")) (c::th("Kind")) (c::th("Type"))
                    (c::th("Member ID")) (c::th("Covers"))
                } },
                html! {
                    @for p in plans {
                        tr class="hover:bg-slate-50" {
                            (c::td(html! { a href={ "/insurance/" (p.id) } class="font-medium" { (p.payer_name) } }))
                            (c::td(html! { (p.plan_name.clone().unwrap_or_else(|| "—".into())) }))
                            (c::td(html! { (c::badge_neutral(plan_kind_label(&p.plan_kind))) }))
                            (c::td(html! { @if let Some(t) = &p.plan_type { (plan_type_label(t)) } @else { "—" } }))
                            (c::td(html! { @if let Some(m) = &p.member_id { span class="font-mono" { (m) } } @else { "—" } }))
                            (c::td(html! { (count_for(p.id)) }))
                        }
                    }
                },
            ))
        }

        div class="mt-6" {
            (c::collapse_section("Add an insurance plan",
                c::form("/insurance", "post", html! {
                    (plan_fields(None, sources))
                    (c::button_primary("Add plan"))
                }),
            plans.is_empty()))
        }
    };
    shell(nav, body)
}

fn subj_name(subjects: &[Subject], id: Uuid) -> String {
    subjects
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.full_name.clone())
        .unwrap_or_else(|| "?".into())
}

pub fn detail_page(
    nav: &Nav<'_>,
    plan: &InsurancePlan,
    covered: &[SubjectInsurance],
    subjects: &[Subject],
) -> Markup {
    let pid = plan.id;
    let body = html! {
        (c::button_link_secondary("/insurance", "← Back to insurance"))
        (c::card(html! {
            div class="flex items-baseline justify-between gap-3" {
                h1 class="text-2xl font-semibold tracking-tight text-ink mb-2" { (plan.payer_name) }
                a href={ "/insurance/" (pid) "/edit" } class="text-sm" { "edit" }
            }
            (c::meta_row(html! {
                (c::badge_neutral(plan_kind_label(&plan.plan_kind)))
                @if let Some(t) = &plan.plan_type { (c::badge_source(plan_type_label(t))) }
                @if let Some(n) = &plan.plan_name { span { (n) } }
            }))
            div class="mt-3 grid grid-cols-2 gap-x-6 gap-y-1 text-sm" {
                @if let Some(m) = &plan.member_id { div { (c::muted("Member ID")) " " span class="font-mono" { (m) } } }
                @if let Some(g) = &plan.group_number { div { (c::muted("Group")) " " span class="font-mono" { (g) } } }
                @if let Some(s) = &plan.subscriber_name { div { (c::muted("Subscriber")) " " (s) } }
                @if let Some(p) = &plan.payer_phone { div { (c::muted("Phone")) " ☎ " (p) } }
                @if let Some(b) = &plan.rx_bin { div { (c::muted("Rx BIN")) " " span class="font-mono" { (b) } } }
                @if let Some(p) = &plan.rx_pcn { div { (c::muted("Rx PCN")) " " span class="font-mono" { (p) } } }
                @if let Some(g) = &plan.rx_group { div { (c::muted("Rx group")) " " span class="font-mono" { (g) } } }
                @if plan.effective_date.is_some() || plan.expiration_date.is_some() {
                    div {
                        (c::muted("Valid")) " "
                        (render_date(plan.effective_date, "day")) " – " (render_date(plan.expiration_date, "day"))
                    }
                }
            }
            @if !plan.notes.is_empty() { div class="mt-2" { (c::prose(&plan.notes)) } }
        }))

        (c::lane(html! { (c::section_heading("Covered people")) }, html! {
            @if covered.is_empty() { (c::empty_state("No one linked to this plan yet.")) }
            @else {
                ul class="space-y-1.5" {
                    @for si in covered {
                        li class="flex items-baseline justify-between gap-3 text-sm" {
                            div class="flex items-baseline gap-2" {
                                span class="font-medium" { (subj_name(subjects, si.subject_id)) }
                                (c::badge_neutral(&si.relationship))
                                @if si.is_primary { (c::badge_source("primary")) }
                                @if let Some(m) = &si.member_id { span class="font-mono text-muted" { (m) } }
                            }
                            (c::post_button(format!("/insurance/{pid}/subjects/{}/remove", si.subject_id), "remove"))
                        }
                    }
                }
            }
        }))
        (c::collapse_section("Cover a subject", {
            let uncovered: Vec<&Subject> = subjects
                .iter()
                .filter(|s| !covered.iter().any(|si| si.subject_id == s.id))
                .collect();
            if uncovered.is_empty() {
                c::alert_info("Every subject is already covered by this plan.")
            } else {
                c::form(format!("/insurance/{pid}/subjects"), "post", html! {
                    (c::field("Subject", c::select_field("subject_id", true, || html! {
                        @for s in &uncovered { (c::select_option(s.id, &s.full_name, false)) }
                    })))
                    (c::field("Relationship to policyholder", c::select_field("relationship", true, || html! {
                        @for r in INSURANCE_RELATIONSHIPS { (c::select_option(r, *r, *r == "self")) }
                    })))
                    (c::field_with_hint("Member ID", "Optional — only if this person's ID differs from the card's.",
                        c::input_text("member_id", "", false, Some(60))))
                    (c::checkbox("is_primary", "Primary coverage", true))
                    (c::button_primary("Add coverage"))
                })
            }
        }, covered.is_empty()))
    };
    shell(nav, body)
}

pub fn edit_form(nav: &Nav<'_>, plan: &InsurancePlan, sources: &[Source], error: Option<&str>) -> Markup {
    let body = html! {
        (c::page_title("Edit insurance plan"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::form(format!("/insurance/{}/edit", plan.id), "post", html! {
            (plan_fields(Some(plan), sources))
            div class="flex gap-2" {
                (c::button_primary("Save"))
                (c::button_link_secondary(format!("/insurance/{}", plan.id), "Cancel"))
            }
        }))
    };
    shell(nav, body)
}
