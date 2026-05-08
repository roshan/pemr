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
                (c::th("Incidents")) (c::th("Records")) (c::th(""))
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

/// `/subjects/{id}` — combined bio header + dashboard view.
pub fn dashboard_page(nav: &Nav<'_>, subject: &Subject, data: &DashboardData<'_>) -> Markup {
    let inner = dashboard::body(nav, data);
    let body = html! {
        (bio_header(subject))
        (inner)
    };
    shell(nav, body)
}

fn bio_header(subject: &Subject) -> Markup {
    html! {
        section class="mb-5 flex flex-wrap items-baseline justify-between gap-3" {
            div {
                h1 class="text-2xl font-semibold tracking-tight text-ink" { (subject.full_name) }
                div class="text-xs text-muted mt-0.5 flex flex-wrap gap-x-3 gap-y-1" {
                    @if let Some(d) = subject.dob { span { "DOB " (d) } }
                    @if let Some(s) = &subject.sex_at_birth { span { (s) } }
                    @if let Some(b) = &subject.blood_type { span { "Blood " (b) } }
                    @if let Some(e) = &subject.cf_access_email {
                        span { (c::code(e)) }
                    }
                }
                @if !subject.notes.is_empty() {
                    p class="text-sm text-ink/80 mt-1 max-w-2xl" { (subject.notes) }
                }
            }
            (c::button_link_secondary(format!("/subjects/{}/edit", subject.id), "Edit profile"))
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
