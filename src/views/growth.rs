//! Growth charts (PEMR-24). v1 plots the subject's raw growth measurements
//! (weight / height / head-circumference) over time as SVG trend lines.
//!
//! CDC/WHO **percentile bands** are intentionally NOT drawn yet: they require
//! the official LMS reference tables vendored into the repo (no runtime calls
//! per the supply-chain rules). Fabricating them would be medically wrong, so
//! that's a follow-up — the LMS→z→percentile math is trivial once the real
//! tables are in place.

use maud::{Markup, html};
use time::Date;

use crate::models::Subject;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub struct GrowthSeries {
    pub label: &'static str,
    pub unit: &'static str,
    pub points: Vec<(Date, f64)>,
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n:.1}") }
}

/// One SVG trend line for a measure. Pure SVG attributes (no Tailwind beyond
/// the responsive-size layout utilities on the <svg>).
fn line_chart(label: &str, unit: &str, points: &[(Date, f64)]) -> Markup {
    if points.is_empty() {
        return c::summary_panel(label, c::empty_state("No measurements recorded"));
    }
    const W: f64 = 640.0;
    const H: f64 = 240.0;
    const PL: f64 = 44.0;
    const PR: f64 = 12.0;
    const PT: f64 = 12.0;
    const PB: f64 = 26.0;
    let plot_w = W - PL - PR;
    let plot_h = H - PT - PB;

    let xs: Vec<f64> = points.iter().map(|(d, _)| d.to_julian_day() as f64).collect();
    let ys: Vec<f64> = points.iter().map(|(_, v)| *v).collect();
    let xmin = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let xmax = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ymin = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let ymax = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let xspan = if xmax > xmin { xmax - xmin } else { 1.0 };
    let yspan = if ymax > ymin { ymax - ymin } else { 1.0 };
    let sx = |x: f64| PL + (x - xmin) / xspan * plot_w;
    let sy = |y: f64| PT + (1.0 - (y - ymin) / yspan) * plot_h;

    let poly: String = points
        .iter()
        .map(|(d, v)| format!("{:.1},{:.1}", sx(d.to_julian_day() as f64), sy(*v)))
        .collect::<Vec<_>>()
        .join(" ");
    let axis_y = PT + plot_h;

    let body = html! {
        svg viewBox="0 0 640 240" class="max-w-full h-auto" role="img"
            aria-label=(format!("{label} trend")) {
            // axes
            line x1=(PL) y1=(axis_y) x2=(PL + plot_w) y2=(axis_y) stroke="#cbd5e1" stroke-width="1" {}
            line x1=(PL) y1=(PT) x2=(PL) y2=(axis_y) stroke="#cbd5e1" stroke-width="1" {}
            // trend
            polyline points=(poly) fill="none" stroke="#4f46e5" stroke-width="2" {}
            @for (d, v) in points {
                circle cx=(format!("{:.1}", sx(d.to_julian_day() as f64)))
                       cy=(format!("{:.1}", sy(*v))) r="2.5" fill="#4f46e5" {}
            }
            // y range labels
            text x=(PL - 6.0) y=(PT + 9.0) text-anchor="end" font-size="11" fill="#64748b" { (fmt_num(ymax)) }
            text x=(PL - 6.0) y=(axis_y) text-anchor="end" font-size="11" fill="#64748b" { (fmt_num(ymin)) }
            // x range labels
            text x=(PL) y=(H - 8.0) text-anchor="start" font-size="11" fill="#64748b" { (points.first().unwrap().0) }
            text x=(PL + plot_w) y=(H - 8.0) text-anchor="end" font-size="11" fill="#64748b" { (points.last().unwrap().0) }
        }
    };
    c::summary_panel(html! { (label) " " (c::muted(unit)) }, body)
}

pub fn page(nav: &Nav<'_>, subject: &Subject, series: &[GrowthSeries]) -> Markup {
    let any = series.iter().any(|s| !s.points.is_empty());
    let body = html! {
        (c::page_title(format!("{} — growth", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{}", subject.id), "← Back to chart"))
        div class="mt-4 space-y-4" {
            @if !any {
                (c::alert_info("No growth measurements yet. Add weight / height / head-circumference \
                    observations (canonical LOINC 29463-7 / 8302-2 / 9843-4) via the API or an import, \
                    and they'll plot here."))
            } @else {
                @for s in series { (line_chart(s.label, s.unit, &s.points)) }
                (c::alert_info("Trend lines show recorded measurements over time. CDC/WHO percentile \
                    bands are a follow-up (they need the official LMS reference tables vendored into \
                    the repo)."))
            }
        }
    };
    shell(nav, body)
}
