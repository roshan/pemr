//! Immunization record + forecast (PEMR-25). Shows what's been given and, from
//! the subject's DOB + the simplified routine schedule in [`crate::peds`], what
//! is due / overdue / upcoming.

use maud::{Markup, html};

use crate::models::{Immunization, Subject};
use crate::peds::{DueItem, WellVisit};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn status_badge(status: &str) -> Markup {
    match status {
        "overdue" => c::badge_danger("overdue"),
        "due" => c::badge_warn("due now"),
        _ => c::badge_neutral("upcoming"),
    }
}

pub fn page(
    nav: &Nav<'_>,
    subject: &Subject,
    received: &[Immunization],
    due: &[DueItem],
    well_visits: &[WellVisit],
    has_dob: bool,
) -> Markup {
    let body = html! {
        (c::page_title(format!("{} — immunizations", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{}", subject.id), "← Back to chart"))

        div class="mt-4 space-y-4" {
            // Forecast
            (c::summary_panel("Due / forecast", if !has_dob {
                c::alert_info("Set this subject's date of birth to forecast which vaccines are due.")
            } else if due.is_empty() {
                c::empty_state("Up to date on the routine schedule (nothing due).")
            } else {
                c::panel_list(html! {
                    @for d in due {
                        (c::panel_list_item(
                            html! { (d.family) " " (c::muted(format!("dose {} of the series", d.dose_number))) " " (status_badge(d.status)) },
                            html! { "rec. " (d.due_on) },
                        ))
                    }
                })
            }))

            // Well-child visit cadence (age-based)
            @if has_dob {
                (c::summary_panel("Well-child visits (by age)", if well_visits.is_empty() {
                    c::empty_state("No routine well-child visits recommended in the next year.")
                } else {
                    c::panel_list(html! {
                        @for w in well_visits {
                            (c::panel_list_item(
                                html! {
                                    (w.label) " visit"
                                    @if w.past { " " (c::badge_neutral("recommended date passed")) }
                                },
                                html! { "rec. " (w.recommended_on) },
                            ))
                        }
                    })
                }))
            }

            // Recorded
            (c::summary_panel("Recorded immunizations", if received.is_empty() {
                c::empty_state("None recorded yet.")
            } else {
                c::panel_list(html! {
                    @for im in received {
                        (c::panel_list_item(
                            html! {
                                (im.vaccine)
                                @if let Some(n) = im.dose_number { " " (c::muted(format!("dose {n}"))) }
                            },
                            html! { @if let Some(d) = im.occurred_at { (d) } @else { "date unknown" } },
                        ))
                    }
                })
            }))

            (c::alert_info("Forecast uses a simplified routine ACIP-based schedule — recommended ages \
                only, no catch-up or contraindication logic. Not a substitute for your pediatrician."))
        }
    };
    shell(nav, body)
}
