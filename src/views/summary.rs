//! Printable one-page health summary (PEMR-27). Reuses the chart's
//! `ClinicalSummary` and renders it as stacked sections. App chrome is
//! `print:hidden` (see layout), so Browser → Print → Save as PDF yields a clean
//! one-pager to hand a new provider or the ER. The recorded-immunizations
//! section doubles as a school/camp immunization printout.

use maud::{Markup, html};

use crate::models::Subject;
use crate::peds;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};
use crate::views::subject::ClinicalSummary;

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n:.1}") }
}

fn section(heading: &str, body: Markup) -> Markup {
    html! {
        section class="mb-4" {
            (c::section_heading(heading))
            (body)
        }
    }
}

pub fn page(nav: &Nav<'_>, subject: &Subject, cs: &ClinicalSummary) -> Markup {
    let dob = subject.dob.map(|d| d.to_string()).unwrap_or_else(|| "—".into());
    let body = html! {
        (c::page_title(format!("{} — health summary", subject.full_name)))
        (c::meta_row(html! {
            span { "DOB " (dob) }
            @if let Some(sex) = &subject.sex_at_birth { span class="mx-2 text-muted/60" { "·" } span { (sex) } }
            @if let Some(bt) = &subject.blood_type { span class="mx-2 text-muted/60" { "·" } span { "Blood " (bt) } }
            span class="mx-2 text-muted/60" { "·" }
            span { "generated " (peds::today()) }
        }))
        div class="my-3 print:hidden" {
            (c::alert_info("Use your browser's Print → Save as PDF to export this page. \
                Generated from personal-emr; not a complete medical record."))
        }

        (section("Active problems", if cs.conditions.is_empty() {
            c::empty_state("None recorded")
        } else {
            c::panel_list(html! {
                @for x in &cs.conditions {
                    (c::panel_list_item(html! { (x.name) },
                        html! { @if let Some(d) = x.onset_date { "since " (d) } }))
                }
            })
        }))

        (section("Allergies", if cs.allergies.is_empty() {
            if cs.no_known_allergies { c::empty_state("No known allergies (asserted)") }
            else { c::empty_state("None recorded") }
        } else {
            c::panel_list(html! {
                @for a in &cs.allergies {
                    (c::panel_list_item(html! { (a.substance) },
                        html! {
                            @if let Some(crit) = &a.criticality { (crit) }
                            @else if let Some(sev) = &a.severity { (sev) }
                            @if let Some(r) = &a.reaction { " — " (r) }
                        }))
                }
            })
        }))

        (section("Medications", if cs.medications.is_empty() {
            c::empty_state("None recorded")
        } else {
            c::panel_list(html! {
                @for m in &cs.medications {
                    (c::panel_list_item(html! { (m.name) },
                        html! {
                            @if let Some(dose) = &m.dose { (dose) }
                            @if let Some(freq) = &m.frequency { " · " (freq) }
                        }))
                }
            })
        }))

        (section("Immunizations", if cs.immunizations.is_empty() {
            c::empty_state("None recorded")
        } else {
            c::panel_list(html! {
                @for im in &cs.immunizations {
                    (c::panel_list_item(
                        html! { (im.vaccine) @if let Some(n) = im.dose_number { " " (c::muted(format!("dose {n}"))) } },
                        html! { @if let Some(d) = im.occurred_at { (d) } @else { "date unknown" } }))
                }
            })
        }))

        (section("Recent vitals & growth", if cs.vitals.is_empty() {
            c::empty_state("None recorded")
        } else {
            c::panel_list(html! {
                @for v in &cs.vitals {
                    @let val = v.value_num.map(fmt_num)
                        .or_else(|| v.value_text.clone())
                        .unwrap_or_else(|| "—".into());
                    (c::panel_list_item(html! { (v.display) },
                        html! { (val) @if let Some(u) = &v.unit { " " (u) } " · " (v.effective_on) }))
                }
            })
        }))

        (section("Care team", if cs.care_team.is_empty() {
            c::empty_state("None recorded")
        } else {
            c::panel_list(html! {
                @for m in &cs.care_team {
                    (c::panel_list_item(
                        html! { (m.full_name) @if let Some(sp) = &m.specialty { " " (c::muted(sp)) } },
                        html! { (m.role) }))
                }
            })
        }))

        (section("Upcoming appointments", if cs.upcoming_appts.is_empty() {
            c::empty_state("None scheduled")
        } else {
            c::panel_list(html! {
                @for ap in &cs.upcoming_appts {
                    (c::panel_list_item(html! { (ap.title) }, html! { (ap.starts_at.date()) }))
                }
            })
        }))
    };
    shell(nav, body)
}
