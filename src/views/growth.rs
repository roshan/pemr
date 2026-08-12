//! Growth charts (PEMR-24). Plots the subject's weight / length /
//! head-circumference by AGE (months) with **CDC P5/P50/P95 percentile bands**
//! overlaid (from the vendored CDC LMS tables, see [`crate::growth_ref`]).
//! Bands require the subject's DOB (for age) and sex_at_birth; without sex we
//! draw the measured trend alone.

use maud::{Markup, html};
use uuid::Uuid;

use crate::growth_ref::RefPoint;
use crate::models::Subject;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub struct GrowthPoint {
    pub age_months: f64,
    pub value: f64,
    pub date: time::Date,
    /// Exact CDC percentile at this age (LMS method), when DOB + sex + the
    /// table's 0–36 mo range allow.
    pub percentile: Option<f64>,
}

pub struct GrowthSeries {
    pub label: &'static str,
    pub unit: &'static str,
    pub points: Vec<GrowthPoint>,
    /// CDC P5/P50/P95 curve for this measure + sex (empty if sex unknown).
    pub reference: Vec<RefPoint>,
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n:.1}") }
}

/// Value formatting for tooltips: up to 2 decimals, trailing zeros trimmed
/// (9.98 stays "9.98"; 7.00 becomes "7" — fmt_num would round 9.98 to "10.0").
fn fmt_val(n: f64) -> String {
    let s = format!("{n:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn fmt_pct(p: f64) -> String {
    if p < 1.0 {
        "<P1".into()
    } else if p > 99.0 {
        ">P99".into()
    } else {
        format!("P{p:.0}")
    }
}

fn line_chart(s: &GrowthSeries) -> Markup {
    if s.points.is_empty() && s.reference.is_empty() {
        return c::summary_panel(s.label, c::empty_state("No measurements recorded"));
    }
    const W: f64 = 640.0;
    const H: f64 = 240.0;
    const PL: f64 = 44.0;
    const PR: f64 = 26.0;
    const PT: f64 = 12.0;
    const PB: f64 = 26.0;
    let plot_w = W - PL - PR;
    let plot_h = H - PT - PB;

    // x domain: 0 .. a bit past the latest measurement, clamped to the table range.
    let max_age = s.points.iter().map(|p| p.age_months).fold(0.0_f64, f64::max);
    let xmax = (max_age + 3.0).clamp(6.0, 36.0);
    let refp: Vec<RefPoint> = s
        .reference
        .iter()
        .copied()
        .filter(|r| r.age_months <= xmax + 0.001)
        .collect();

    // y domain from in-range points + reference p5/p95.
    let mut ys: Vec<f64> =
        s.points.iter().filter(|p| p.age_months <= xmax).map(|p| p.value).collect();
    for r in &refp {
        ys.push(r.p5);
        ys.push(r.p95);
    }
    if ys.is_empty() {
        ys = s.points.iter().map(|p| p.value).collect();
    }
    let ymin = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let ymax = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let yspan = if ymax > ymin { ymax - ymin } else { 1.0 };
    let xspan = if xmax > 0.0 { xmax } else { 1.0 };

    let sx = |a: f64| PL + (a / xspan) * plot_w;
    let sy = |v: f64| PT + (1.0 - (v - ymin) / yspan) * plot_h;

    let refline = |sel: &dyn Fn(&RefPoint) -> f64| -> String {
        refp.iter()
            .map(|r| format!("{:.1},{:.1}", sx(r.age_months), sy(sel(r))))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let subj: String = s
        .points
        .iter()
        .filter(|p| p.age_months <= xmax)
        .map(|p| format!("{:.1},{:.1}", sx(p.age_months), sy(p.value)))
        .collect::<Vec<_>>()
        .join(" ");
    let axis_y = PT + plot_h;
    let has_ref = !refp.is_empty();

    let body = html! {
        svg viewBox="0 0 640 240" class="max-w-full h-auto" role="img"
            aria-label=(format!("{} for age", s.label)) {
            line x1=(PL) y1=(axis_y) x2=(PL + plot_w) y2=(axis_y) stroke="#cbd5e1" stroke-width="1" {}
            line x1=(PL) y1=(PT) x2=(PL) y2=(axis_y) stroke="#cbd5e1" stroke-width="1" {}
            @if has_ref {
                polyline points=(refline(&|r| r.p5)) fill="none" stroke="#cbd5e1" stroke-width="1" {}
                polyline points=(refline(&|r| r.p95)) fill="none" stroke="#cbd5e1" stroke-width="1" {}
                polyline points=(refline(&|r| r.p50)) fill="none" stroke="#94a3b8" stroke-width="1" stroke-dasharray="4 3" {}
                @let last = refp.last().unwrap();
                text x=(sx(last.age_months) + 3.0) y=(sy(last.p95) + 3.0) font-size="9" fill="#94a3b8" { "95" }
                text x=(sx(last.age_months) + 3.0) y=(sy(last.p50) + 3.0) font-size="9" fill="#94a3b8" { "50" }
                text x=(sx(last.age_months) + 3.0) y=(sy(last.p5) + 3.0) font-size="9" fill="#94a3b8" { "5" }
            }
            @if !subj.is_empty() {
                polyline points=(subj) fill="none" stroke="#4f46e5" stroke-width="2" {}
                // Each point is a hover group: visible dot + a larger invisible
                // hit target + a CSS-revealed tooltip (value · percentile / date
                // · age). Pure Tailwind group-hover — no JS, per HTMX rules.
                @for p in s.points.iter().filter(|p| p.age_months <= xmax) {
                    @let px = sx(p.age_months);
                    @let py = sy(p.value);
                    @let line1 = match p.percentile {
                        Some(pc) => format!("{} {} · {}", fmt_val(p.value), s.unit, fmt_pct(pc)),
                        None => format!("{} {}", fmt_val(p.value), s.unit),
                    };
                    @let line2 = format!("{} · {:.1} mo", p.date, p.age_months);
                    // ~5.4px/char at font-size 10; SVG text can't be measured server-side.
                    @let tw = 12.0 + 5.4 * line1.len().max(line2.len()) as f64;
                    @let tx = (px - tw / 2.0).clamp(PL + 2.0, PL + plot_w - tw + 20.0);
                    @let ty = if py - 42.0 >= PT { py - 42.0 } else { py + 10.0 };
                    g class="group" {
                        circle cx=(format!("{px:.1}")) cy=(format!("{py:.1}")) r="2.5" fill="#4f46e5" {}
                        circle cx=(format!("{px:.1}")) cy=(format!("{py:.1}")) r="9"
                            fill="none" pointer-events="all" {}
                        g class="hidden group-hover:block" pointer-events="none" {
                            circle cx=(format!("{px:.1}")) cy=(format!("{py:.1}")) r="4"
                                fill="none" stroke="#4f46e5" stroke-width="1.5" {}
                            rect x=(format!("{tx:.1}")) y=(format!("{ty:.1}")) width=(format!("{tw:.1}"))
                                height="32" rx="4" fill="#ffffff" fill-opacity="0.95"
                                stroke="#cbd5e1" stroke-width="1" {}
                            text x=(format!("{:.1}", tx + 6.0)) y=(format!("{:.1}", ty + 13.0))
                                font-size="10" font-weight="600" fill="#0f172a" { (line1) }
                            text x=(format!("{:.1}", tx + 6.0)) y=(format!("{:.1}", ty + 26.0))
                                font-size="10" fill="#64748b" { (line2) }
                        }
                    }
                }
            }
            text x=(PL - 6.0) y=(PT + 9.0) text-anchor="end" font-size="11" fill="#64748b" { (fmt_num(ymax)) }
            text x=(PL - 6.0) y=(axis_y) text-anchor="end" font-size="11" fill="#64748b" { (fmt_num(ymin)) }
            text x=(PL) y=(H - 8.0) text-anchor="start" font-size="11" fill="#64748b" { "0 mo" }
            text x=(PL + plot_w) y=(H - 8.0) text-anchor="end" font-size="11" fill="#64748b" { (format!("{:.0} mo", xmax)) }
        }
    };
    c::summary_panel(html! { (s.label) " " (c::muted(s.unit)) }, body)
}

/// Compact growth card for the subject chart: the weight-for-age curve with CDC
/// bands, the latest value of each measure, linked to the full charts.
pub fn mini_card(subject_id: Uuid, series: &[GrowthSeries]) -> Markup {
    let weight = series.iter().find(|s| s.label == "Weight");
    let latest = |label: &str| -> Option<String> {
        series.iter().find(|s| s.label == label).and_then(|s| {
            s.points.last().map(|p| match p.percentile {
                Some(pc) => format!("{} {} ({})", fmt_val(p.value), s.unit, fmt_pct(pc)),
                None => format!("{} {}", fmt_val(p.value), s.unit),
            })
        })
    };
    let chart = match weight {
        Some(w) if !w.points.is_empty() => mini_chart(w),
        _ => c::empty_state("No growth measurements"),
    };
    c::summary_panel_linked(
        html! { "Growth " (c::muted("weight for age")) },
        format!("/subjects/{subject_id}/growth"),
        html! {
            (chart)
            div class="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted" {
                @if let Some(w) = latest("Weight") { span { "Wt " span class="text-ink" { (w) } } }
                @if let Some(l) = latest("Length / height") { span { "Len " span class="text-ink" { (l) } } }
                @if let Some(h) = latest("Head circumference") { span { "HC " span class="text-ink" { (h) } } }
            }
        },
    )
}

/// A small SVG (no axis labels) of one measure vs. its CDC bands — for `mini_card`.
fn mini_chart(s: &GrowthSeries) -> Markup {
    const W: f64 = 300.0;
    const H: f64 = 120.0;
    const PL: f64 = 6.0;
    const PR: f64 = 16.0;
    const PT: f64 = 8.0;
    const PB: f64 = 8.0;
    let plot_w = W - PL - PR;
    let plot_h = H - PT - PB;
    let max_age = s.points.iter().map(|p| p.age_months).fold(0.0_f64, f64::max);
    let xmax = (max_age + 2.0).clamp(6.0, 36.0);
    let refp: Vec<RefPoint> =
        s.reference.iter().copied().filter(|r| r.age_months <= xmax + 0.001).collect();
    let mut ys: Vec<f64> =
        s.points.iter().filter(|p| p.age_months <= xmax).map(|p| p.value).collect();
    for r in &refp {
        ys.push(r.p5);
        ys.push(r.p95);
    }
    if ys.is_empty() {
        ys = s.points.iter().map(|p| p.value).collect();
    }
    let ymin = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let ymax = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let yspan = if ymax > ymin { ymax - ymin } else { 1.0 };
    let xspan = if xmax > 0.0 { xmax } else { 1.0 };
    let sx = |a: f64| PL + (a / xspan) * plot_w;
    let sy = |v: f64| PT + (1.0 - (v - ymin) / yspan) * plot_h;
    let refline = |sel: &dyn Fn(&RefPoint) -> f64| -> String {
        refp.iter()
            .map(|r| format!("{:.1},{:.1}", sx(r.age_months), sy(sel(r))))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let subj: String = s
        .points
        .iter()
        .filter(|p| p.age_months <= xmax)
        .map(|p| format!("{:.1},{:.1}", sx(p.age_months), sy(p.value)))
        .collect::<Vec<_>>()
        .join(" ");
    let has_ref = !refp.is_empty();
    html! {
        svg viewBox="0 0 300 120" class="w-full h-auto" role="img"
            aria-label=(format!("{} for age", s.label)) {
            @if has_ref {
                polyline points=(refline(&|r| r.p5)) fill="none" stroke="#e2e8f0" stroke-width="1" {}
                polyline points=(refline(&|r| r.p95)) fill="none" stroke="#e2e8f0" stroke-width="1" {}
                polyline points=(refline(&|r| r.p50)) fill="none" stroke="#cbd5e1" stroke-width="1" stroke-dasharray="3 2" {}
            }
            @if !subj.is_empty() {
                polyline points=(subj) fill="none" stroke="#4f46e5" stroke-width="1.5" {}
                @for p in s.points.iter().filter(|p| p.age_months <= xmax) {
                    circle cx=(format!("{:.1}", sx(p.age_months))) cy=(format!("{:.1}", sy(p.value))) r="2" fill="#4f46e5" {}
                }
            }
        }
    }
}

pub fn page(
    nav: &Nav<'_>,
    subject: &Subject,
    series: &[GrowthSeries],
    has_dob: bool,
    has_sex: bool,
) -> Markup {
    let body = html! {
        (c::page_title(format!("{} — growth", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{}", subject.id), "← Back to chart"))
        div class="mt-4 space-y-4" {
            @if !has_dob {
                (c::alert_info("Set this subject's date of birth to plot growth by age and overlay \
                    CDC percentile bands."))
            } @else {
                @for s in series { (line_chart(s)) }
                @if has_sex {
                    (c::alert_info("Bands are CDC P5 / P50 / P95 for age (0–36 mo infant charts; \
                        dashed = median). Measured points plotted by age in months. CDC data is public \
                        domain; US standard-of-care is WHO for 0–24 mo."))
                } @else {
                    (c::alert_info("Showing measured trend by age. Set this subject's sex at birth to \
                        overlay CDC percentile bands."))
                }
            }
        }
    };
    shell(nav, body)
}
