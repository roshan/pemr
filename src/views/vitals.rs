//! `/subjects/{id}/vitals` — the full vitals & labs list (all observations), the
//! "View all" target from the chart's "Recent vitals & labs" card.

use maud::{Markup, html};

use crate::models::{ObservationRow, Subject};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n:.1}") }
}

/// Human reference range from the low/high bounds.
fn reference(lo: Option<f64>, hi: Option<f64>) -> String {
    match (lo, hi) {
        (Some(l), Some(h)) => format!("{}–{}", fmt_num(l), fmt_num(h)),
        (None, Some(h)) => format!("≤ {}", fmt_num(h)),
        (Some(l), None) => format!("≥ {}", fmt_num(l)),
        (None, None) => "—".into(),
    }
}
/// A lab battery grouped under one result `<organizer>` (its members share a
/// `panel_id`). Singletons (no panel) are their own group of one.
enum Group<'a> {
    Panel(&'a [ObservationRow]),
    Row(&'a ObservationRow),
}

/// Chunk the (panel-ordered) rows so a CBC's analytes group under their panel
/// instead of reading as five unrelated rows. Groups preserve row order.
fn groups<'a>(rows: &'a [ObservationRow]) -> Vec<Group<'a>> {
    let mut out: Vec<Group<'a>> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let pid = rows[i].panel_id;
        if let Some(pid) = pid {
            // All rows sharing this panel are adjacent (query orders by
            // panel_id within an effective_on). Take the run.
            let mut j = i + 1;
            while j < rows.len() && rows[j].panel_id == Some(pid) {
                j += 1;
            }
            if j - i > 1 {
                out.push(Group::Panel(&rows[i..j]));
            } else {
                out.push(Group::Row(&rows[i]));
            }
            i = j;
        } else {
            out.push(Group::Row(&rows[i]));
            i += 1;
        }
    }
    out
}

pub fn page(nav: &Nav<'_>, subject: &Subject, rows: &[ObservationRow]) -> Markup {
    let body = html! {
        (c::page_title(format!("{} — vitals & labs", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{}", subject.id), "← Back to chart"))
        div class="mt-4" {
            @if rows.is_empty() {
                (c::empty_state("No vitals or labs recorded"))
            } @else {
                (c::data_table(
                    html! { tr {
                        (c::th("Date")) (c::th("Test")) (c::th("Value")) (c::th("Reference")) (c::th("Flag"))
                    } },
                    html! {
                        @for g in groups(rows) {
                            @if let Group::Panel(members) = g {
                                tr class="bg-slate-100" {
                                    td colspan="5" class="px-3 py-1.5 text-xs font-medium text-slate-600" {
                                        "Result panel — " (members[0].effective_on) " (" (members.len()) " results)"
                                    }
                                }
                                @for r in members {
                                    @let val = r.value_num.map(fmt_num)
                                        .or_else(|| r.value_text.clone())
                                        .unwrap_or_else(|| "—".into());
                                    tr class="hover:bg-slate-50" {
                                        (c::td(html! { (r.effective_on) }))
                                        (c::td(html! { (r.display) " " (c::muted(r.category.as_str())) }))
                                        (c::td(html! { span class="font-medium" { (val) } @if let Some(u) = &r.unit { " " (c::muted(u)) } }))
                                        (c::td(html! { (c::muted(reference(r.ref_low, r.ref_high))) }))
                                        (c::td(html! {
                                            @if let Some(f) = &r.abnormal_flag { @if f != "normal" { (c::badge_warn(f)) } }
                                        }))
                                    }
                                }
                            }
                            @if let Group::Row(r) = g {
                                @let val = r.value_num.map(fmt_num)
                                    .or_else(|| r.value_text.clone())
                                    .unwrap_or_else(|| "—".into());
                                tr class="hover:bg-slate-50" {
                                    (c::td(html! { (r.effective_on) }))
                                    (c::td(html! { (r.display) " " (c::muted(r.category.as_str())) }))
                                    (c::td(html! { span class="font-medium" { (val) } @if let Some(u) = &r.unit { " " (c::muted(u)) } }))
                                    (c::td(html! { (c::muted(reference(r.ref_low, r.ref_high))) }))
                                    (c::td(html! {
                                        @if let Some(f) = &r.abnormal_flag { @if f != "normal" { (c::badge_warn(f)) } }
                                    }))
                                }
                            }
                        }
                    },
                ))
            }
        }
    };
    shell(nav, body)
}
