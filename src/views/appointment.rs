//! Per-subject appointments UI (PEMR-17): list (upcoming + past) + add/edit
//! with the status lifecycle. Plain form POST → redirect.

use maud::{Markup, html};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::models::{APPOINTMENT_STATUSES, Appointment, Provider, Subject};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn fmt_dt(t: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
    )
}

fn status_badge(status: &str) -> Markup {
    match status {
        "cancelled" => c::badge_danger("cancelled"),
        "no_show" => c::badge_warn("no-show"),
        "completed" => c::badge_neutral("completed"),
        _ => c::badge_source("scheduled"),
    }
}

fn provider_name(providers: &[Provider], id: Option<Uuid>) -> Option<String> {
    id.and_then(|pid| providers.iter().find(|p| p.id == pid))
        .map(|p| p.full_name.clone())
}

fn appt_row(a: &Appointment, providers: &[Provider]) -> Markup {
    html! {
        li class="flex items-baseline justify-between gap-3 text-sm" {
            div class="flex items-baseline gap-2" {
                span class="text-xs text-muted w-32" { (fmt_dt(a.starts_at)) }
                span class="font-medium" { (a.title) }
                (status_badge(&a.status))
                @if let Some(n) = provider_name(providers, a.provider_id) { (c::muted(n)) }
            }
            a href={ "/appointments/" (a.id) "/edit" } { "edit" }
        }
    }
}

fn provider_select(name: &str, providers: &[Provider], selected: Option<Uuid>) -> Markup {
    c::select_field(name, false, || {
        html! {
            (c::select_option("", "— none —", selected.is_none()))
            @for p in providers {
                (c::select_option(p.id, &p.full_name, selected == Some(p.id)))
            }
        }
    })
}

fn status_select(selected: &str) -> Markup {
    c::select_field("status", true, || {
        html! {
            @for s in APPOINTMENT_STATUSES {
                (c::select_option(s, *s, *s == selected))
            }
        }
    })
}

pub fn list_page(
    nav: &Nav<'_>,
    subject: &Subject,
    upcoming: &[Appointment],
    past: &[Appointment],
    providers: &[Provider],
) -> Markup {
    let body = html! {
        (c::page_title(format!("{} — appointments", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{}", subject.id), "← Back to chart"))

        div class="mt-4 space-y-5" {
            (c::lane(html! { (c::section_heading("Upcoming")) }, html! {
                @if upcoming.is_empty() { (c::empty_state("None scheduled.")) }
                @else { ul class="space-y-1.5" { @for a in upcoming { (appt_row(a, providers)) } } }
            }))
            (c::lane(html! { (c::section_heading("Past")) }, html! {
                @if past.is_empty() { (c::empty_state("None.")) }
                @else { ul class="space-y-1.5" { @for a in past { (appt_row(a, providers)) } } }
            }))

            (c::collapse_section("Add an appointment", html! {
                (c::form(format!("/subjects/{}/appointments", subject.id), "post", html! {
                    (c::field("Title", c::input_text("title", "", true, Some(120))))
                    (c::field("Starts at", c::input_datetime("starts_at", "", true)))
                    (c::field("Ends at", c::input_datetime("ends_at", "", false)))
                    (c::field("Status", status_select("scheduled")))
                    (c::field("Provider", provider_select("provider_id", providers, None)))
                    (c::field("Location", c::input_text("location", "", false, Some(160))))
                    (c::field("Notes", c::textarea_field("notes", "", 3)))
                    (c::button_primary("Add appointment"))
                }))
            }, upcoming.is_empty() && past.is_empty()))
        }
    };
    shell(nav, body)
}

pub fn edit_form(
    nav: &Nav<'_>,
    a: &Appointment,
    subject: &Subject,
    providers: &[Provider],
    error: Option<&str>,
) -> Markup {
    // datetime-local wants "YYYY-MM-DDTHH:MM"
    let dtv = |t: OffsetDateTime| {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}",
            t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute(),
        )
    };
    let body = html! {
        (c::page_title("Edit appointment"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::form(format!("/appointments/{}/edit", a.id), "post", html! {
            (c::field("Title", c::input_text("title", &a.title, true, Some(120))))
            (c::field("Starts at", c::input_datetime("starts_at", &dtv(a.starts_at), true)))
            (c::field("Ends at", c::input_datetime("ends_at", &a.ends_at.map(dtv).unwrap_or_default(), false)))
            (c::field("Status", status_select(&a.status)))
            (c::field("Provider", provider_select("provider_id", providers, a.provider_id)))
            (c::field("Location", c::input_text("location", &a.location.clone().unwrap_or_default(), false, Some(160))))
            (c::field("Notes", c::textarea_field("notes", &a.notes, 3)))
            div class="flex gap-2" {
                (c::button_primary("Save"))
                (c::button_link_secondary(format!("/subjects/{}/appointments", subject.id), "Cancel"))
            }
        }))
    };
    shell(nav, body)
}
