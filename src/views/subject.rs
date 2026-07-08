use maud::{Markup, html};

use crate::models::Subject;
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
    cards: &[Markup],
    data: &DashboardData<'_>,
    timeline: &dashboard::TimelineData,
) -> Markup {
    // Skip the compact mini-timeline in the embedded body — the rich
    // `timeline_widget` below already covers it (no two timelines on one page).
    let inner = dashboard::body(nav, data, false);
    let body = html! {
        (bio_header(subject))
        // The clinical summary is an ordered set of self-contained modules
        // (`subject_modules`); this page just lays out whatever cards they
        // produced — it has no per-feature knowledge.
        section class="mb-6" {
            (c::section_heading("Clinical summary"))
            (c::card_grid(html! { @for card in cards { (card) } }))
        }
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
