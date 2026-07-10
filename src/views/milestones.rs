//! Developmental-milestone tracker UI (PEMR-40 / 41 / 42 / 44). Pure rendering —
//! the handlers load the data. The checklist is an inline, feature-gated section
//! on the subject chart; marking a milestone or stepping between checkpoints
//! HTMX-swaps `#milestone-checklist` in place. The "Act Early" guidance is passive
//! (a closed disclosure), never an automatic alert. The required
//! tracking-vs-screening disclaimer (`milestones::DISCLAIMER`) is shown in the
//! module and on both the progress + printable-summary pages.

use std::collections::HashMap;

use maud::{Markup, html};
use uuid::Uuid;

use crate::feature_registry::FeatureDef;
use crate::milestone_age::{self, TrackerAge};
use crate::milestones::{self, Milestone};
use crate::models::{MilestoneResponse, Subject};
use crate::peds;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn checklist_url(subject_id: Uuid, checkpoint: i32) -> String {
    format!("/subjects/{subject_id}/milestones/checklist?checkpoint={checkpoint}")
}

fn by_key<'a>(rows: &'a [MilestoneResponse]) -> HashMap<&'a str, &'a MilestoneResponse> {
    rows.iter().map(|r| (r.milestone_key.as_str(), r)).collect()
}

/// The swappable checklist for one checkpoint: a checkpoint stepper + the four
/// domains' milestones, each mark-able yes / not-yet / no. `computed` is the
/// subject's current-age checkpoint (marked "current age"); may be `None` if the
/// subject has no DOB.
pub fn checklist(
    subject_id: Uuid,
    checkpoint: i32,
    computed: Option<i32>,
    responses: &[MilestoneResponse],
) -> Markup {
    let map = by_key(responses);
    let cps = milestones::CHECKPOINTS;
    let idx = cps.iter().position(|&c| c == checkpoint);
    let prev = idx.and_then(|i| i.checked_sub(1)).map(|i| cps[i]);
    let next = idx.and_then(|i| cps.get(i + 1)).copied();

    html! {
        div id="milestone-checklist" {
            div class="flex items-center justify-between gap-2 mb-3" {
                (c::hx_nav_button(
                    "← Younger",
                    &prev.map(|p| checklist_url(subject_id, p)).unwrap_or_default(),
                    "#milestone-checklist",
                    prev.is_some(),
                ))
                div class="text-center" {
                    div class="text-sm font-semibold text-ink" { "Milestones by " (checkpoint) " months" }
                    div class="text-xs text-muted" {
                        (milestone_age::fmt_months(checkpoint))
                        @if computed == Some(checkpoint) { " · current age" }
                    }
                }
                (c::hx_nav_button(
                    "Older →",
                    &next.map(|n| checklist_url(subject_id, n)).unwrap_or_default(),
                    "#milestone-checklist",
                    next.is_some(),
                ))
            }

            @for (_key, label, items) in milestones::by_checkpoint_grouped(checkpoint) {
                @let met = items.iter().filter(|m| map.get(m.key).map(|r| r.response.as_str()) == Some("yes")).count();
                div class="mb-4" {
                    div class="flex items-baseline justify-between gap-3 mb-1" {
                        (c::subheading(label))
                        (c::progress_meter(met, items.len()))
                    }
                    ul {
                        @for m in &items {
                            (c::milestone_row(row_controls(subject_id, checkpoint, m, map.get(m.key).copied())))
                        }
                    }
                }
            }
        }
    }
}

/// One milestone's response controls: three buttons (the answer is encoded in
/// each button's POST URL — see `components::milestone_mark_button`), plus (once
/// "yes") an observed-on date that saves on change. No `<form>` — htmx's
/// `new FormData(form)` drops submit-button values, so a shared form would never
/// send the response.
fn row_controls(
    subject_id: Uuid,
    checkpoint: i32,
    m: &Milestone,
    resp: Option<&MilestoneResponse>,
) -> Markup {
    let cur = resp.map(|r| r.response.as_str());
    let mark = |r: &str| {
        format!("/subjects/{subject_id}/milestones/mark/{}/{r}?checkpoint={checkpoint}", m.key)
    };
    let observed_url =
        format!("/subjects/{subject_id}/milestones/observed/{}?checkpoint={checkpoint}", m.key);
    html! {
        div class="flex items-start justify-between gap-3" {
            span class="text-sm text-ink" { (m.text) }
            div class="flex gap-1 shrink-0" {
                (c::milestone_mark_button("Yes", &mark("yes"), cur == Some("yes")))
                (c::milestone_mark_button("Not yet", &mark("not_yet"), cur == Some("not_yet")))
                (c::milestone_mark_button("No", &mark("no"), cur == Some("no")))
            }
        }
        @if cur == Some("yes") {
            div class="mt-2 flex items-center gap-2" {
                span class="text-xs text-muted" { "First observed" }
                (c::milestone_observed_input(
                    &resp.and_then(|r| r.observed_on).map(|d| d.to_string()).unwrap_or_default(),
                    &observed_url,
                ))
            }
        }
    }
}

fn act_early_body() -> Markup {
    html! {
        p class="text-sm text-muted" {
            "Reference guidance from the CDC. This is informational only — nothing here is \
             triggered automatically by your answers."
        }
        @for para in milestones::ACT_EARLY_GUIDANCE {
            p class="text-sm text-ink mt-2" { (para) }
        }
    }
}

/// The compact milestone surface on the subject chart (feature area): current
/// checkpoint + per-checkpoint completion, the required disclaimer in brief, a
/// link to the full detail page, and a Remove control (disable the feature; data
/// is preserved). The interactive checklist lives on the detail page, not here.
pub fn summary_card(
    subject: &Subject,
    tracker: Option<TrackerAge>,
    met: usize,
    total: usize,
) -> Markup {
    let sid = subject.id;
    c::card(html! {
        div class="flex flex-wrap items-baseline justify-between gap-2 mb-1" {
            div class="flex flex-wrap items-baseline gap-2" {
                (c::section_heading("Developmental milestones"))
                @if let Some(t) = tracker { (c::badge_neutral(t.basis.label())) }
            }
            (c::hx_action_button(
                "Remove",
                &format!("/subjects/{sid}/features/milestones/remove"),
                "#subject-features",
                true,
            ))
        }
        @match tracker {
            Some(t) => {
                div class="flex flex-wrap items-center justify-between gap-2" {
                    span class="text-sm text-muted" {
                        "Checklist by " span class="text-ink" { (milestone_age::fmt_months(t.checkpoint)) }
                    }
                    (c::progress_meter(met, total))
                }
            }
            None => div class="mb-1" {
                (c::alert_info("Set this child\u{2019}s date of birth to use the milestone tracker."))
            }
        }
        p class="text-xs text-muted mt-2" { (milestones::DISCLAIMER) }
        div class="mt-3 flex flex-wrap gap-2" {
            (c::button_link_secondary(format!("/subjects/{sid}/milestones"), "Open milestones \u{2192}"))
        }
    })
}

/// The dedicated milestone detail page (`/subjects/{id}/milestones`): the full
/// interactive checklist, the disclaimer, the passive Act Early disclosure, and
/// links to the progress + printable-summary pages. `inner` is the checklist (or
/// a "set DOB" notice).
pub fn detail_page(
    nav: &Nav<'_>,
    subject: &Subject,
    tracker: Option<TrackerAge>,
    inner: Markup,
) -> Markup {
    let sid = subject.id;
    let body = html! {
        (c::page_title(format!("{} \u{2014} developmental milestones", subject.full_name)))
        div class="flex flex-wrap items-center gap-2 mb-3" {
            (c::button_link_secondary(format!("/subjects/{sid}"), "\u{2190} Back to chart"))
            (c::muted("CDC \u{201c}Learn the Signs. Act Early.\u{201d}"))
            @if let Some(t) = tracker { (c::badge_neutral(t.basis.label())) }
        }
        div class="mb-2" { (c::alert_info(milestones::DISCLAIMER)) }
        div class="mb-2" {
            (c::collapse_section(milestones::ACT_EARLY_HEADING, act_early_body(), false))
        }
        div class="flex flex-wrap gap-2 mb-4" {
            (c::button_link_secondary(format!("/subjects/{sid}/milestones/progress"), "Progress view"))
            (c::button_link_secondary(format!("/subjects/{sid}/milestones/summary"), "Printable summary"))
        }
        (inner)
    };
    shell(nav, body)
}

/// Shown on the detail page when the subject has no DOB — the checklist needs an
/// age to compute the checkpoint. No nag, just a pointer to set it.
pub fn needs_dob(subject: &Subject) -> Markup {
    html! {
        (c::alert_info("Set this child's date of birth to show the milestone checklist for their age."))
        div class="mt-2" {
            (c::button_link_secondary(format!("/subjects/{}/edit", subject.id), "Edit profile"))
        }
    }
}

/// The `#subject-features` container: each enabled feature's surface, then the
/// "Add feature" picker for the rest. Swapped whole on add/remove.
pub fn feature_area(subject_id: Uuid, surfaces: Vec<Markup>, add_options: &[&FeatureDef]) -> Markup {
    html! {
        div id="subject-features" {
            @for s in &surfaces {
                div class="mb-4" { (s) }
            }
            @if !add_options.is_empty() {
                div class="mt-1" {
                    p class="text-xs text-muted mb-1" { "Add a feature module to this chart:" }
                    div class="flex flex-wrap gap-2" {
                        @for f in add_options {
                            (c::hx_action_button(
                                format!("+ {}", f.label),
                                &format!("/subjects/{subject_id}/features/{}", f.key),
                                "#subject-features",
                                false,
                            ))
                        }
                    }
                }
            }
        }
    }
}

// ── Progress view (PEMR-44) ──────────────────────────────────────────────────

/// Milestones met per (checkpoint, domain).
fn met_total(checkpoint: i32, domain: &str, map: &HashMap<&str, &MilestoneResponse>) -> (usize, usize) {
    let items: Vec<Milestone> = milestones::by_checkpoint(checkpoint)
        .into_iter()
        .filter(|m| m.domain == domain)
        .collect();
    let met = items
        .iter()
        .filter(|m| map.get(m.key).map(|r| r.response.as_str()) == Some("yes"))
        .count();
    (met, items.len())
}

pub fn progress_page(
    nav: &Nav<'_>,
    subject: &Subject,
    tracker: Option<TrackerAge>,
    responses: &[MilestoneResponse],
) -> Markup {
    let map = by_key(responses);
    let current_cp = tracker.map(|t| t.checkpoint);
    let any_achieved = responses.iter().any(|r| r.response == "yes");
    // Achieved ("yes") milestones per domain, sorted by observed date then age —
    // computed here so the template stays declarative.
    let achieved_by_domain: Vec<(&'static str, Vec<&MilestoneResponse>)> = milestones::DOMAINS
        .iter()
        .map(|(dkey, dlabel)| {
            let mut rows: Vec<&MilestoneResponse> = responses
                .iter()
                .filter(|r| r.response == "yes" && r.domain == *dkey)
                .collect();
            rows.sort_by(|a, b| {
                a.observed_on
                    .cmp(&b.observed_on)
                    .then(a.expected_age_months.cmp(&b.expected_age_months))
            });
            (*dlabel, rows)
        })
        .collect();
    let body = html! {
        (c::page_title(format!("{} — milestone progress", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{}", subject.id), "← Back to chart"))
        div class="my-3" { (c::alert_info(milestones::DISCLAIMER)) }
        @if let Some(t) = tracker {
            p class="text-sm text-muted mb-3" {
                "Tracking age: " span class="text-ink" { (milestone_age::fmt_months(t.computed_months)) }
                " (" (t.basis.label()) ")."
            }
        }

        section class="mb-6" {
            (c::section_heading("Completion by checkpoint"))
            p class="text-sm text-muted mb-2" {
                "Each cell shows how many milestones you\u{2019}ve marked \u{201c}yes\u{201d} out of the total \
                 for that age and domain \u{2014} the arc filling in over time."
            }
            (c::data_table(
                html! { tr {
                    (c::th("Checkpoint"))
                    @for (_k, label) in milestones::DOMAINS { (c::th(*label)) }
                }},
                html! {
                    @for &cp in milestones::CHECKPOINTS {
                        tr {
                            (c::td(html! {
                                span class="font-medium" { (milestone_age::fmt_months(cp)) }
                                @if current_cp == Some(cp) { " " (c::badge_neutral("current")) }
                            }))
                            @for (dkey, _label) in milestones::DOMAINS {
                                @let (met, total) = met_total(cp, dkey, &map);
                                (c::td(c::progress_meter(met, total)))
                            }
                        }
                    }
                },
            ))
        }

        section class="mb-6" {
            (c::section_heading("Milestones achieved"))
            @if !any_achieved {
                (c::empty_state("No milestones marked \u{201c}yes\u{201d} yet. Mark milestones on the chart to build the record."))
            } @else {
                (c::card_grid(html! {
                    @for (dlabel, achieved) in &achieved_by_domain {
                        (c::summary_panel(*dlabel, if achieved.is_empty() {
                            c::empty_state("None yet")
                        } else {
                            c::panel_list(html! {
                                @for r in achieved {
                                    (c::panel_list_item(
                                        html! { (milestones::by_key(&r.milestone_key).map(|m| m.text).unwrap_or("(unknown)")) },
                                        html! {
                                            @if let Some(d) = r.observed_on { (d) }
                                            @else { (c::muted("date not set")) }
                                        },
                                    ))
                                }
                            })
                        }))
                    }
                }))
            }
        }
    };
    shell(nav, body)
}

// ── Printable summary (PEMR-42) ──────────────────────────────────────────────

pub fn summary_page(
    nav: &Nav<'_>,
    subject: &Subject,
    tracker: Option<TrackerAge>,
    checkpoint: i32,
    responses: &[MilestoneResponse],
) -> Markup {
    let map = by_key(responses);
    let dob = subject.dob.map(|d| d.to_string()).unwrap_or_else(|| "—".into());
    let body = html! {
        (c::page_title(format!("{} — developmental milestones", subject.full_name)))
        (c::meta_row(html! {
            span { "DOB " (dob) }
            @if let Some(t) = tracker {
                span class="mx-2 text-muted/60" { "·" }
                span { "Tracking age " (milestone_age::fmt_months(t.computed_months)) " (" (t.basis.label()) ")" }
            }
            span class="mx-2 text-muted/60" { "·" }
            span { "Checklist by " (milestone_age::fmt_months(checkpoint)) }
            span class="mx-2 text-muted/60" { "·" }
            span { "generated " (peds::today()) }
        }))
        div class="my-3" { (c::alert_info(milestones::DISCLAIMER)) }
        div class="my-3 print:hidden" {
            (c::alert_info("Use your browser\u{2019}s Print \u{2192} Save as PDF to export or attach this summary."))
        }

        @for (_dkey, dlabel, items) in milestones::by_checkpoint_grouped(checkpoint) {
            @let met = items.iter().filter(|m| map.get(m.key).map(|r| r.response.as_str()) == Some("yes")).count();
            section class="mb-4" {
                div class="flex items-baseline justify-between gap-3 mb-2" {
                    (c::section_heading(dlabel))
                    (c::muted(format!("{met}/{} met", items.len())))
                }
                (c::panel_list(html! {
                    @for m in &items {
                        @let r = map.get(m.key).copied();
                        (c::panel_list_item(
                            html! { (m.text) },
                            html! {
                                (milestones::response_label(r.map(|x| x.response.as_str()).unwrap_or("—")))
                                @if let Some(d) = r.and_then(|x| x.observed_on) { " · observed " (d) }
                            },
                        ))
                    }
                }))
            }
        }
    };
    shell(nav, body)
}
