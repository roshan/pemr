use maud::{Markup, PreEscaped, html};
use time::Date;
use uuid::Uuid;

use crate::models::{Incident, Record, Subject, is_image_kind, record_kind_label};
use crate::views::components as c;
use crate::views::layout::{Nav, render_date, shell, subject_badge};

/// One dated thing on the timeline (incident, record, condition, …).
pub struct TimelineEvent {
    pub date: Date,
    pub kind: String,
    pub title: String,
    pub href: Option<String>,
    pub subject_id: Uuid,
}

/// All events on one calendar day, positioned at `pct` along the axis.
pub struct TimelineBucket {
    pub pct: f64,
    pub date: Date,
    pub kind: String, // dominant kind → dot colour
    pub events: Vec<TimelineEvent>,
}

/// Everything the timeline view needs, prepared by the handler.
pub struct TimelineData {
    pub range: String,             // active preset for tab highlight ("" if custom window)
    pub start: String,             // current window (ISO) — date boxes + zoom anchor
    pub end: String,
    pub min: String, // data bounds (ISO) — clamp zoom
    pub max: String,
    pub ticks: Vec<(f64, String)>, // month/year axis labels at their pct
    pub buckets: Vec<TimelineBucket>,
    pub subject: Option<Uuid>,
}

pub struct DashboardData<'a> {
    pub subjects: &'a [Subject],
    pub timeline_incidents: &'a [Incident],
    pub timeline_total: i64,
    pub recent_incidents: &'a [Incident],
    pub recent_records: &'a [Record],
}

const DASHBOARD_TIMELINE_LIMIT: usize = 12;

pub fn render(nav: &Nav<'_>, data: &DashboardData<'_>) -> Markup {
    shell(nav, body(nav, data))
}

/// Inner body markup (search bar, timeline, recent lanes, action shortcuts).
/// Reusable so the per-subject dashboard at `/subjects/{id}` can wrap it
/// with a bio header.
pub fn body(nav: &Nav<'_>, data: &DashboardData<'_>) -> Markup {
    html! {
        section class="mb-6" {
            form action="/" method="get" class="space-y-2" {
                (c::input_search(
                    "q",
                    "Search incidents and records…",
                    "/search",
                    "#results",
                    "[name=subject]",
                ))
                @if let Some(id) = nav.current_subject {
                    input type="hidden" name="subject" value=(id);
                }
            }
            div #results class="mt-3" {}
        }

        (incidents_timeline(nav.current_subject, data.timeline_incidents, data.timeline_total))

        (c::lane(
            html! {
                (c::section_heading("Recent incidents"))
                a href=(list_url("/incidents", nav.current_subject))
                  class="text-xs text-muted hover:text-brand" { "View all →" }
            },
            html! {
                @if data.recent_incidents.is_empty() {
                    (c::empty_state(html! {
                        "No incidents yet. "
                        a href=(new_incident_url(nav)) class="text-brand" { "Add one" } "."
                    }))
                } @else {
                    (c::card_grid(html! {
                        @for inc in data.recent_incidents {
                            (c::card(html! {
                                (c::card_title(format!("/incidents/{}", inc.id), &inc.title))
                                (c::meta_row(html! {
                                    (subject_badge(data.subjects, inc.subject_id))
                                    (c::badge_neutral(render_date(inc.occurred_at, &inc.occurred_precision)))
                                }))
                                @if !inc.narrative.is_empty() {
                                    p class="mt-2 text-sm text-ink/80 line-clamp-3" { (inc.narrative) }
                                }
                            }))
                        }
                    }))
                }
            },
        ))

        (c::lane(
            html! {
                (c::section_heading("Recent records"))
                a href=(list_url("/records", nav.current_subject))
                  class="text-xs text-muted hover:text-brand" { "View all →" }
            },
            html! {
                @if data.recent_records.is_empty() {
                    (c::empty_state(html! {
                        "No records yet. "
                        a href=(new_record_url(nav)) class="text-brand" { "Add one" } "."
                    }))
                } @else {
                    (c::card_grid(html! {
                        @for rec in data.recent_records {
                            (record_card(rec, data.subjects))
                        }
                    }))
                }
            },
        ))

        section class="flex flex-wrap gap-2 pt-2" {
            (c::button_link_primary(new_incident_url(nav), "New incident"))
            (c::button_link_secondary(new_record_url(nav), "New record"))
            (c::button_link_secondary("/sources", "Manage sources"))
        }
    }
}

fn list_url(base: &str, subject: Option<uuid::Uuid>) -> String {
    use crate::views::layout::subject_scoped_url;
    subject_scoped_url(base, subject)
}
fn new_incident_url(nav: &Nav<'_>) -> String {
    match nav.current_subject {
        Some(id) => format!("/incidents/new?subject={id}"),
        None => "/incidents/new".to_string(),
    }
}
fn new_record_url(nav: &Nav<'_>) -> String {
    match nav.current_subject {
        Some(id) => format!("/records/new?subject={id}"),
        None => "/records/new".to_string(),
    }
}

/// A "Recent records" card. For image-kind records that have a stored
/// thumbnail we lead with the picture (full-width banner across the top of
/// the card); other records just show title + meta.
fn record_card(rec: &Record, subjects: &[Subject]) -> Markup {
    let detail_url = format!("/records/{}", rec.id);
    let thumb_url = if rec.thumbnail_path.is_some() && is_image_kind(&rec.kind) {
        Some(format!("/records/{}/thumbnail", rec.id))
    } else {
        None
    };
    html! {
        article class="rounded-lg border border-line bg-surface shadow-xs overflow-hidden flex flex-col" {
            @if let Some(url) = thumb_url {
                a href=(detail_url) class="block hover:no-underline" {
                    img src=(url) alt=(rec.title) loading="lazy"
                        class="block w-full h-40 object-cover bg-slate-900";
                }
            }
            div class="p-4" {
                (c::card_title(detail_url, &rec.title))
                (c::meta_row(html! {
                    (subject_badge(subjects, rec.subject_id))
                    (c::badge_kind(record_kind_label(&rec.kind)))
                    (c::badge_neutral(render_date(rec.occurred_at, &rec.occurred_precision)))
                }))
            }
        }
    }
}

/// Compact horizontal timeline — dots on a line with dates underneath.
/// Shows the most recent `DASHBOARD_TIMELINE_LIMIT` incidents; if there are
/// more, an "Expand timeline →" link takes the user to the dedicated page.
fn incidents_timeline(
    current_subject: Option<uuid::Uuid>,
    incidents: &[Incident],
    total: i64,
) -> Markup {
    if incidents.is_empty() {
        return html! {};
    }
    // Oldest → newest reads left-to-right.
    let mut sorted: Vec<&Incident> = incidents.iter().collect();
    sorted.sort_by(|a, b| a.occurred_at.cmp(&b.occurred_at).then(a.created_at.cmp(&b.created_at)));

    let expand_url = crate::views::layout::subject_scoped_url("/timeline", current_subject);
    let has_more = total as usize > sorted.len();

    html! {
        section class="mb-6" {
            div class="flex items-baseline justify-between mb-2 gap-3" {
                (c::section_heading("Timeline"))
                a href=(expand_url) class="text-xs text-muted hover:text-brand" {
                    @if has_more { "Expand timeline (" (total) ") →" }
                    @else        { "Expand timeline →" }
                }
            }
            div class="relative overflow-x-auto rounded-lg border border-line bg-surface px-3 py-3" {
                div class="absolute timeline-line-top left-3 right-3 h-px bg-line" {}
                ol class="relative flex items-start gap-5 min-w-max" {
                    @for inc in &sorted {
                        li {
                            a href={ "/incidents/" (inc.id) }
                              title=(inc.title)
                              class="group block text-center" {
                                span class="block mx-auto w-3 h-3 rounded-full bg-brand group-hover:bg-indigo-800 ring-2 ring-surface transition-colors" {}
                                time class="block text-xs text-muted mt-2 whitespace-nowrap group-hover:text-ink" {
                                    (render_date(inc.occurred_at, &inc.occurred_precision))
                                }
                                span class="block text-xs text-ink mt-0.5 max-w-32 truncate group-hover:text-brand" {
                                    (inc.title)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

const TIMELINE_KINDS: [&str; 6] =
    ["incident", "record", "condition", "immunization", "observation", "appointment"];

/// The reusable timeline body (legend + axis), shared by the full `/timeline`
/// page and the subject chart. `tabs` shows the duration selector.
pub fn timeline_widget(data: &TimelineData, tabs: bool) -> Markup {
    let base = match data.subject {
        Some(id) => format!("/timeline?subject={id}"),
        None => "/timeline".to_string(),
    };
    let rurl = |r: &str| format!("{base}{}range={r}", if base.contains('?') { "&" } else { "?" });
    html! {
        @if tabs {
            div class="flex flex-wrap items-center gap-2 mb-3" {
                (c::timeline_tab(&rurl("3m"), "3M", data.range == "3m"))
                (c::timeline_tab(&rurl("1y"), "1Y", data.range == "1y"))
                (c::timeline_tab(&rurl("5y"), "5Y", data.range == "5y"))
                (c::timeline_tab(&rurl("all"), "All", data.range == "all"))
                span class="text-muted px-1" { "·" }
                form hx-get="/timeline" hx-target="#tl-inner" hx-swap="outerHTML"
                     hx-trigger="change" class="flex items-center gap-1" {
                    @if let Some(id) = data.subject { input type="hidden" name="subject" value=(id); }
                    (c::timeline_date_input("from", &data.start))
                    span class="text-muted" { "–" }
                    (c::timeline_date_input("to", &data.end))
                }
            }
        }
        div class="flex flex-wrap gap-3 mb-2" {
            @for k in TIMELINE_KINDS { (c::timeline_legend_item(k)) }
        }
        @if data.buckets.is_empty() {
            (c::empty_state("No events in this window — zoom out for more history."))
        } @else {
            (c::timeline_band(html! {
                @for (pct, label) in &data.ticks { (c::timeline_tick(*pct, label)) }
                @for b in &data.buckets {
                    @let d = b.date.to_string();
                    @let lo = b.events.first().map(|e| e.date.to_string()).unwrap_or_else(|| d.clone());
                    @let hi = b.events.last().map(|e| e.date.to_string()).unwrap_or_else(|| d.clone());
                    @let n = b.events.len();
                    @let aria = format!("{d} · {n} event{}", if n == 1 { "" } else { "s" });
                    (c::timeline_marker(b.pct, &d, &b.kind, n, &day_detail_url(data.subject, &lo, &hi), &aria))
                }
            }))
        }
        // Persistent panel: a marker click hx-swaps its events in here, so the
        // list stays open to click through. Lives inside #tl-inner, so it resets
        // when the window changes (zoom / tab / date box).
        div id="tl-detail" class="mt-4" { (c::timeline_detail_hint()) }
    }
}

/// htmx URL a marker click fetches into `#tl-detail`. `from`/`to` are the
/// bucket's own date span, so the detail handler re-queries exactly its events.
fn day_detail_url(subject: Option<uuid::Uuid>, from: &str, to: &str) -> String {
    match subject {
        Some(id) => format!("/timeline/day?subject={id}&from={from}&to={to}"),
        None => format!("/timeline/day?from={from}&to={to}"),
    }
}

/// Contents of the persistent detail panel for a clicked timeline point —
/// rendered into `#tl-detail` by an htmx GET to `/timeline/day`. `heading` is
/// the pre-formatted date (or date range) of the bucket.
pub fn timeline_day_detail(
    events: &[TimelineEvent],
    subject: Option<Uuid>,
    subjects: &[Subject],
    heading: &str,
) -> Markup {
    c::timeline_detail(heading, events.len(), html! {
        @if events.is_empty() {
            p class="text-sm text-muted" { "No events." }
        } @else {
            @for e in events {
                @let trailing = if subject.is_none() {
                    subject_badge(subjects, e.subject_id)
                } else {
                    html! {}
                };
                (c::timeline_event_row(&e.kind, &e.title, e.href.as_deref(), trailing))
            }
        }
    })
}

/// Scroll-wheel zoom, centred on the cursor: the date under the pointer stays
/// put while the window shrinks (wheel up) or grows (wheel down) by ~25% a tick.
/// The only first-party JS in the app — wheel direction + cursor position can't
/// be read in CSS/htmx. Reads the current window off `#tl-inner` + data bounds
/// off `#tl-zoom` and `htmx.ajax`-swaps `#tl-inner` to an explicit `from`/`to`
/// window. The cursored date keeps its fraction of the window so it stays under
/// the pointer (no edge re-shifting). A zoom-in that would land on a blank gap
/// is refused (it checks the visible markers' `data-d` dates), so you stop at
/// the tightest populated view rather than an empty one; at the fully-zoomed-out
/// end it lets the page scroll, and the empty-state still accepts a wheel so you
/// can always zoom back out. Initialization is deferred to `DOMContentLoaded`
/// because htmx loads with `defer` and isn't ready while this inline script
/// first runs.
const WHEEL_ZOOM_JS: &str = r#"
(function(){
  // htmx.min.js loads with `defer`, so it isn't ready while this inline script
  // runs during parse. Wait for DOMContentLoaded (fires after deferred scripts)
  // before wiring up, or window.htmx would be undefined and we'd bail silently.
  function init(){
    var z=document.getElementById('tl-zoom'); if(!z||!window.htmx) return;
    var DAY=86400000, base=z.dataset.base, busy=false;
    function ms(s){ return Date.parse(s); }
    function iso(m){ return new Date(m).toISOString().slice(0,10); }
    z.addEventListener('htmx:afterSettle', function(){ busy=false; });
    z.addEventListener('wheel', function(e){
      var inner=document.getElementById('tl-inner'); if(!inner) return;
      // The empty-state has no [data-tl-band]; fall back to #tl-inner so the
      // wheel still works there and the user can always zoom back out.
      var band=z.querySelector('[data-tl-band]')||inner;
      var br=band.getBoundingClientRect();
      if(e.clientY < br.top-28 || e.clientY > br.bottom+28) return;   // not over the strip: page scrolls
      var s=ms(inner.dataset.start), en=ms(inner.dataset.end), mn=ms(z.dataset.min), mx=ms(z.dataset.max);
      if(isNaN(s)||isNaN(en)) return;
      var span=en-s, full=Math.max(DAY, mx-mn);
      if(e.deltaY>0 && span>=full) return;                           // fully out: page scrolls
      e.preventDefault();
      if(busy) return;
      var tf=Math.min(1, Math.max(0, ((e.clientX-br.left)/Math.max(1,br.width)-0.04)/0.92));
      var focal=s+tf*span;
      var ns=e.deltaY<0 ? span*0.8 : span/0.8;                       // gentler than 0.6: ~25%/tick
      ns=Math.min(full, Math.max(21*DAY, ns));
      var a,b;
      // Pure cursor-centred: the date under the pointer keeps the same fraction
      // of the window, so it stays put. We deliberately DON'T shift the window
      // back inside [mn,mx] near the edges (that drifts the cursored date) — the
      // span is already capped at the data range, and the zoom-in gap-check
      // below stops us drifting into empty space.
      if(ns>=full){ a=mn; b=mx; }
      else { a=Math.round(focal-tf*ns); b=a+ns; }
      // Zoom-in: refuse a step that would land on a blank gap. Every visible
      // marker carries its date in data-d, and a zoom-in window is always a
      // subset of the current one, so if no marker falls inside [a,b] the new
      // window is empty — stay put at the tightest populated view.
      if(e.deltaY<0){
        var hit=false, dots=inner.querySelectorAll('[data-d]');
        for(var i=0;i<dots.length;i++){ var dm=ms(dots[i].dataset.d); if(dm>=a&&dm<=b){hit=true;break;} }
        if(!hit) return;
      }
      busy=true; setTimeout(function(){busy=false;},400);
      var url=base+(base.indexOf('?')>=0?'&':'?')+'from='+iso(a)+'&to='+iso(b);
      window.htmx.ajax('GET', url, {target:'#tl-inner', swap:'outerHTML'});
    }, {passive:false});
  }
  if(document.readyState==='loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
"#;

/// The swappable inner band (htmx target `#tl-inner`). Carries the current
/// window so the zoom script can read it after each in-place swap.
pub fn timeline_inner(data: &TimelineData) -> Markup {
    html! {
        div id="tl-inner" data-start=(data.start) data-end=(data.end) {
            (timeline_widget(data, true))
        }
    }
}

pub fn visual_timeline(nav: &Nav<'_>, data: &TimelineData) -> Markup {
    let base = match data.subject {
        Some(id) => format!("/timeline?subject={id}"),
        None => "/timeline".to_string(),
    };
    let body = html! {
        (c::page_title("Timeline"))
        div id="tl-zoom" data-base=(base) data-min=(data.min) data-max=(data.max) {
            (timeline_inner(data))
        }
        script { (PreEscaped(WHEEL_ZOOM_JS)) }
    };
    shell(nav, body)
}

pub fn dashboard_timeline_limit() -> usize { DASHBOARD_TIMELINE_LIMIT }
