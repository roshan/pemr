//! Care reminders UI (PEMR-19): per-subject "what's due" list + add + mark
//! done/dismiss. `overdue` is derived (due_on < today), never stored.

use maud::{Markup, html};
use time::Date;

use crate::models::{CARE_REMINDER_KINDS, CareReminder, Subject};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn badge(r: &CareReminder, today: Date) -> Markup {
    match r.status.as_str() {
        "done" => c::badge_neutral("done"),
        "dismissed" => c::muted("dismissed"),
        _ => match r.due_on {
            Some(d) if d < today => c::badge_danger("overdue"),
            _ => c::badge_warn("due"),
        },
    }
}

pub fn page(nav: &Nav<'_>, subject: &Subject, reminders: &[CareReminder], today: Date) -> Markup {
    let sid = subject.id;
    let body = html! {
        (c::page_title(format!("{} — care reminders", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{sid}"), "← Back to chart"))

        div class="mt-4 space-y-5" {
            (c::lane(html! { (c::section_heading("Reminders")) }, html! {
                @if reminders.is_empty() { (c::empty_state("Nothing tracked.")) }
                @else {
                    ul class="space-y-1.5" {
                        @for r in reminders {
                            li class="flex items-baseline justify-between gap-3 text-sm" {
                                div class="flex items-baseline gap-2" {
                                    (badge(r, today))
                                    span class="font-medium" { (r.title) }
                                    (c::muted(&r.kind))
                                    @if let Some(d) = r.due_on { (c::muted(html! { "due " (d) })) }
                                }
                                @if r.status == "due" {
                                    div class="flex gap-1" {
                                        (c::post_button(format!("/subjects/{sid}/reminders/{}/done", r.id), "done"))
                                        (c::post_button(format!("/subjects/{sid}/reminders/{}/dismissed", r.id), "dismiss"))
                                    }
                                }
                            }
                        }
                    }
                }
            }))

            (c::collapse_section("Add a reminder", html! {
                (c::form(format!("/subjects/{sid}/reminders"), "post", html! {
                    (c::field("Title", c::input_text("title", "", true, Some(120))))
                    (c::field("Kind", c::select_field("kind", true, || html! {
                        @for k in CARE_REMINDER_KINDS { (c::select_option(k, *k, *k == "other")) }
                    })))
                    (c::field("Due on", c::input_date("due_on", "")))
                    (c::button_primary("Add reminder"))
                }))
            }, reminders.is_empty()))
        }
    };
    shell(nav, body)
}
