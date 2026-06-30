use maud::{Markup, html};

use crate::models::{
    Allergy, Appointment, CareTeamMember, Condition, Immunization, Medication, Subject, VitalRow,
};
use crate::views::components as c;
use crate::views::dashboard::{self, DashboardData};
use crate::views::layout::{Nav, shell};

pub fn list_page(nav: &Nav<'_>, subjects: &[Subject], counts: &[(uuid::Uuid, i64, i64)]) -> Markup {
    let body = html! {
        (c::page_title("Subjects"))

        (c::data_table(
            html! { tr {
                (c::th("Name")) (c::th("DOB")) (c::th("Email (CF Access)"))
                (c::th("Events")) (c::th("Records")) (c::th(""))
            }},
            html! {
                @for s in subjects {
                    @let (inc, rec) = counts.iter()
                        .find(|(id, _, _)| *id == s.id)
                        .map(|(_, i, r)| (*i, *r))
                        .unwrap_or((0, 0));
                    tr class="hover:bg-slate-50" {
                        (c::td(html! { a href={ "/subjects/" (s.id) } class="font-medium" { (s.full_name) } }))
                        (c::td(html! { (s.dob.map(|d| d.to_string()).unwrap_or_else(|| "—".into())) }))
                        (c::td(html! { (s.cf_access_email.clone().unwrap_or_else(|| "—".into())) }))
                        (c::td(html! { (inc) }))
                        (c::td(html! { (rec) }))
                        (c::td(html! { a href={ "/subjects/" (s.id) "/edit" } { "edit" } }))
                    }
                }
            },
        ))

        div class="mt-6" {
            (c::collapse_section("Add a subject (e.g. parent)", html! {
                (c::form("/subjects", "post", html! {
                    (c::field("Given name", c::input_text("given_name", "", true, Some(60))))
                    (c::field("Family name", c::input_text("family_name", "", true, Some(60))))
                    (c::field("Date of birth", c::input_date("dob", "")))
                    (c::field("Sex at birth", c::input_text("sex_at_birth", "", false, Some(40))))
                    (c::field("Blood type", c::input_text("blood_type", "", false, Some(10))))
                    (c::field_with_hint(
                        "Cloudflare Access email",
                        "Optional — used as the default subject for that email when they sign in.",
                        c::input_email("cf_access_email", "", None),
                    ))
                    (c::field("Notes", c::textarea_field("notes", "", 3)))
                    (c::button_primary("Add subject"))
                }))
            }, false))
        }
    };
    shell(nav, body)
}

/// `/subjects/{id}` — the subject CHART: bio header, a clinical summary
/// (problems / meds / allergies / vitals / immunizations / appointments / care
/// team), then the incident + record timeline below.
pub fn dashboard_page(
    nav: &Nav<'_>,
    subject: &Subject,
    summary: &ClinicalSummary,
    data: &DashboardData<'_>,
    timeline: &dashboard::TimelineData,
) -> Markup {
    // Skip the compact mini-timeline in the embedded body — the rich
    // `timeline_widget` below already covers it (no two timelines on one page).
    let inner = dashboard::body(nav, data, false);
    let body = html! {
        (bio_header(subject))
        (clinical_summary(summary))
        section class="mb-6" {
            div class="flex items-baseline justify-between mb-2" {
                (c::section_heading("Timeline"))
                (c::link_subtle(format!("/subjects/{}/timeline", subject.id), "Open full timeline →"))
            }
            (dashboard::timeline_widget(timeline, false))
            // Detail panel sits below the embedded timeline (its own block), so
            // a marker click's event list persists and reads as separate.
            div id="tl-detail" class="mt-4" { (c::timeline_detail_hint()) }
        }
        (inner)
    };
    shell(nav, body)
}

/// Clinical data surfaced on the subject chart. Owned Vecs, built by the handler.
pub struct ClinicalSummary {
    pub subject_id: uuid::Uuid,
    pub no_known_allergies: bool,
    pub allergies: Vec<Allergy>,
    pub medications: Vec<Medication>,
    pub conditions: Vec<Condition>,
    pub immunizations: Vec<Immunization>,
    pub vitals: Vec<VitalRow>,
    pub upcoming_appts: Vec<Appointment>,
    pub care_team: Vec<CareTeamMember>,
    /// Vaccines due or overdue (from the forecast), for the panel badge.
    pub vaccines_due: usize,
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }
}

fn clinical_summary(cs: &ClinicalSummary) -> Markup {
    html! {
        section class="mb-6" {
            (c::section_heading("Clinical summary"))
            (c::card_grid(html! {
                (c::summary_panel("Problems", if cs.conditions.is_empty() {
                    c::empty_state("No active problems")
                } else {
                    truncated_list(cs.conditions.iter().map(|x| c::panel_list_item(
                        html! { (x.name) },
                        html! { @if let Some(d) = x.onset_date { "since " (d) } },
                    )), cs.conditions.len(), None)
                }))
                (c::summary_panel("Medications", if cs.medications.is_empty() {
                    c::empty_state("No active medications")
                } else {
                    truncated_list(cs.medications.iter().map(|m| c::panel_list_item(
                        html! { (m.name) },
                        html! {
                            @if let Some(dose) = &m.dose { (dose) }
                            @if let Some(freq) = &m.frequency { " · " (freq) }
                        },
                    )), cs.medications.len(), None)
                }))
                (c::summary_panel("Allergies", if cs.allergies.is_empty() {
                    if cs.no_known_allergies {
                        c::empty_state("No known allergies (asserted)")
                    } else {
                        c::empty_state("No allergies recorded")
                    }
                } else {
                    truncated_list(cs.allergies.iter().map(|a| c::panel_list_item(
                        html! { (a.substance) },
                        html! {
                            @if let Some(crit) = &a.criticality { (crit) }
                            @else if let Some(sev) = &a.severity { (sev) }
                        },
                    )), cs.allergies.len(), None)
                }))
                (c::summary_panel("Recent vitals & labs", if cs.vitals.is_empty() {
                    c::empty_state("No vitals or labs recorded")
                } else {
                    truncated_list(cs.vitals.iter().map(|v| {
                        let val = v.value_num.map(fmt_num)
                            .or_else(|| v.value_text.clone())
                            .unwrap_or_else(|| "—".into());
                        c::panel_list_item(
                            html! {
                                (v.display)
                                @if let Some(f) = &v.abnormal_flag {
                                    @if f != "normal" { " " (c::badge_warn(f)) }
                                }
                            },
                            html! {
                                (val) @if let Some(u) = &v.unit { " " (u) }
                                " · " (v.effective_on)
                            },
                        )
                    }), cs.vitals.len(), None)
                }))
                (c::summary_panel_linked(html! {
                    "Immunizations"
                    @if cs.vaccines_due > 0 { " " (c::badge_warn(format!("{} due", cs.vaccines_due))) }
                }, format!("/subjects/{}/immunizations", cs.subject_id),
                if cs.immunizations.is_empty() {
                    c::empty_state("No immunizations recorded")
                } else {
                    truncated_list(cs.immunizations.iter().map(|im| c::panel_list_item(
                        html! { (im.vaccine) },
                        html! { @if let Some(d) = im.occurred_at { (d) } },
                    )), cs.immunizations.len(), Some(format!("/subjects/{}/immunizations", cs.subject_id)))
                }))
                (c::summary_panel_linked("Upcoming appointments",
                format!("/subjects/{}/appointments", cs.subject_id),
                if cs.upcoming_appts.is_empty() {
                    c::empty_state("None scheduled")
                } else {
                    truncated_list(cs.upcoming_appts.iter().map(|ap| c::panel_list_item(
                        html! { (ap.title) },
                        html! { (ap.starts_at.date()) },
                    )), cs.upcoming_appts.len(), Some(format!("/subjects/{}/appointments", cs.subject_id)))
                }))
                (c::summary_panel_linked("Care team",
                format!("/subjects/{}/care-team", cs.subject_id),
                if cs.care_team.is_empty() {
                    c::empty_state("No care team recorded")
                } else {
                    truncated_list(cs.care_team.iter().map(|m| c::panel_list_item(
                        html! {
                            (m.full_name)
                            @if let Some(sp) = &m.specialty { " " (c::muted(sp)) }
                        },
                        html! { (m.role) },
                    )), cs.care_team.len(), Some(format!("/subjects/{}/care-team", cs.subject_id)))
                }))
            }))
        }
    }
}

const CARD_MAX: usize = 4;

fn truncated_list(
    items: impl Iterator<Item = Markup>,
    total: usize,
    more_href: Option<String>,
) -> Markup {
    let shown: Vec<Markup> = items.take(CARD_MAX).collect();
    let overflow = total.saturating_sub(shown.len());
    html! {
        (c::panel_list(html! {
            @for item in shown { (item) }
        }))
        @if overflow > 0 {
            @if let Some(href) = more_href {
                a href=(href) class="block mt-2 text-xs text-brand hover:underline" {
                    "and " (overflow) " more →"
                }
            } @else {
                p class="mt-2 text-xs text-muted" { "and " (overflow) " more" }
            }
        }
    }
}

fn bio_header(subject: &Subject) -> Markup {
    let bio_parts: Vec<Markup> = [
        subject.dob.map(|d| html! { span { "DOB " (d) } }),
        subject.sex_at_birth.as_ref().map(|s| html! { span { (s) } }),
        subject.blood_type.as_ref().map(|b| html! { span { "Blood " (b) } }),
        subject.cf_access_email.as_ref().map(|e| html! { span { (c::code(e)) } }),
    ]
    .into_iter()
    .flatten()
    .collect();
    html! {
        section class="mb-5 flex flex-wrap items-baseline justify-between gap-3" {
            div {
                h1 class="text-2xl font-semibold tracking-tight text-ink" { (subject.full_name) }
                @if !bio_parts.is_empty() {
                    div class="text-xs text-muted mt-1" {
                        @for (i, p) in bio_parts.iter().enumerate() {
                            @if i > 0 { span class="mx-2 text-muted/60" { "·" } }
                            (p)
                        }
                    }
                }
                @if !subject.notes.is_empty() {
                    p class="text-sm text-ink/80 mt-1 max-w-2xl" { (subject.notes) }
                }
            }
            div class="flex flex-wrap gap-2" {
                (c::button_link_primary(format!("/subjects/{}/clinical", subject.id), "+ Add data"))
                (c::button_link_secondary(format!("/subjects/{}/summary", subject.id), "Summary (print)"))
                (c::button_link_secondary(format!("/subjects/{}/appointments", subject.id), "Appointments"))
                (c::button_link_secondary(format!("/subjects/{}/immunizations", subject.id), "Immunizations"))
                (c::button_link_secondary(format!("/subjects/{}/care-team", subject.id), "Care team & IDs"))
                (c::button_link_secondary(format!("/subjects/{}/reminders", subject.id), "Reminders"))
                (c::button_link_secondary(format!("/subjects/{}/growth", subject.id), "Growth charts"))
                (c::button_link_secondary(format!("/subjects/{}/edit", subject.id), "Edit profile"))
            }
        }
    }
}

pub fn edit_form(nav: &Nav<'_>, subject: &Subject, error: Option<&str>) -> Markup {
    let body = html! {
        (c::page_title("Edit subject"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::form(format!("/subjects/{}/edit", subject.id), "post", html! {
            (c::field("Given name", c::input_text("given_name", &subject.given_name, true, Some(60))))
            (c::field("Family name", c::input_text("family_name", &subject.family_name, true, Some(60))))
            (c::field("Date of birth", c::input_date(
                "dob", &subject.dob.map(|d| d.to_string()).unwrap_or_default(),
            )))
            (c::field("Sex at birth", c::input_text(
                "sex_at_birth", &subject.sex_at_birth.clone().unwrap_or_default(), false, Some(40),
            )))
            (c::field("Blood type", c::input_text(
                "blood_type", &subject.blood_type.clone().unwrap_or_default(), false, Some(10),
            )))
            (c::field("Cloudflare Access email", c::input_email(
                "cf_access_email",
                &subject.cf_access_email.clone().unwrap_or_default(),
                None,
            )))
            (c::field("Notes", c::textarea_field("notes", &subject.notes, 5)))
            div class="flex gap-2" {
                (c::button_primary("Save"))
                (c::button_link_secondary(format!("/subjects/{}", subject.id), "Cancel"))
            }
        }))
    };
    shell(nav, body)
}
