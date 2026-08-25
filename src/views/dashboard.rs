use maud::{Markup, PreEscaped, html};
use time::Date;
use uuid::Uuid;

use crate::models::{Incident, Record, Subject, is_image_kind, record_kind_label};
use crate::views::components as c;
use crate::views::layout::{Nav, render_date, render_date_range, shell, subject_badge};

/// One dated thing on the timeline (incident, record, condition, …).
pub struct TimelineEvent {
    pub date: Date,
    pub kind: String,
    pub title: String,
    pub href: Option<String>,
    pub subject_id: Uuid,
    /// End of a multi-day event (an event/incident with `ended_at`); `None` for
    /// point-in-time events. When set and after `date`, it renders as a span bar.
    pub end_date: Option<Date>,
}

/// A multi-day event rendered as a bar spanning `start_pct`..`end_pct` of the
/// band, instead of a dot.
pub struct TimelineSpan {
    pub start_pct: f64,
    pub end_pct: f64,
    pub kind: String,
    pub title: String,
    pub href: Option<String>,
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
    pub spans: Vec<TimelineSpan>, // multi-day events drawn as bars
    pub subject: Option<Uuid>,
}

pub struct DashboardData<'a> {
    pub subjects: &'a [Subject],
    pub timeline_incidents: &'a [Incident],
    pub timeline_total: i64,
    pub recent_incidents: &'a [Incident],
    pub recent_records: &'a [Record],
    /// Subject-scoped clinical at-a-glance, shown only on the home dashboard
    /// (`/`) when a single subject is in scope and has clinical data. `None` on
    /// the all-subjects view and on the per-subject chart (which renders the
    /// full clinical summary itself).
    pub clinical: Option<ClinicalSnapshot>,
    /// Current insurance card tiles for the one-tap dashboard lane.
    pub insurance_tiles: &'a [crate::handlers::insurance::InsuranceCardTile],
}

/// The home-dashboard clinical snapshot: the subject in scope plus the counts
/// straight from `subject_modules::snapshot_counts` (the same filters the
/// chart's modules use, so the two never disagree on what counts as active /
/// due). No field-by-field copying — the counts struct rides along whole.
pub struct ClinicalSnapshot {
    pub subject_id: Uuid,
    pub counts: crate::subject_modules::SnapshotCounts,
}

/// Timeline window presets: (query key, tab label, window in days —
/// `None` = the full data range). One entry drives both the tab row and the
/// handler's preset lookup, in display order.
pub const TIMELINE_RANGES: &[(&str, &str, Option<i64>)] = &[
    ("3m", "3M", Some(91)),
    ("1y", "1Y", Some(365)),
    ("5y", "5Y", Some(1826)),
    ("all", "All", None),
];
pub const DEFAULT_RANGE: &str = "1y";

const DASHBOARD_TIMELINE_LIMIT: usize = 12;

pub fn render(nav: &Nav<'_>, data: &DashboardData<'_>) -> Markup {
    shell(nav, body(nav, data, true))
}

/// Inner body markup (search bar, timeline, recent lanes, action shortcuts).
/// Reusable so the per-subject dashboard at `/subjects/{id}` can wrap it with a
/// bio header. `show_timeline` is false there because the subject page already
/// renders the richer `timeline_widget` above — avoids two timelines on one page.
pub fn body(nav: &Nav<'_>, data: &DashboardData<'_>, show_timeline: bool) -> Markup {
    html! {
        section class="mb-6" {
            form action="/" method="get" class="space-y-2" {
                (c::input_search(
                    "q",
                    "Search events and records…",
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

        @if let Some(snap) = &data.clinical {
            (clinical_snapshot(snap))
        }

        @if show_timeline && !data.insurance_tiles.is_empty() {
            (insurance_cards_lane(data.insurance_tiles))
        }

        @if show_timeline {
            (incidents_timeline(nav.current_subject, data.timeline_incidents, data.timeline_total))
        }

        (c::lane(
            html! {
                (c::section_heading("Recent events"))
                a href=(list_url("/incidents", nav.current_subject))
                  class="text-xs text-muted hover:text-brand" { "View all →" }
            },
            html! {
                @if data.recent_incidents.is_empty() {
                    (c::empty_state(html! {
                        "No events yet. "
                        a href=(new_incident_url(nav)) class="text-brand" { "Add one" } "."
                    }))
                } @else {
                    (c::card_grid(html! {
                        @for inc in data.recent_incidents {
                            (c::card(html! {
                                (c::card_title(format!("/incidents/{}", inc.id), &inc.title))
                                (c::meta_row(html! {
                                    (subject_badge(data.subjects, inc.subject_id))
                                    (c::badge_neutral(render_date_range(inc.occurred_at, &inc.occurred_precision, inc.ended_at, &inc.ended_precision)))
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
            (c::button_link_primary(new_incident_url(nav), "New event"))
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

/// The one-tap "Insurance cards" lane (PEMR-55): the current card image for
/// each plan, linked to the full plan/card detail. A plan with no card on file
/// renders a clean absence tile linking to the plan (where uploads happen),
/// matching the API's 404-as-absence contract. Home dashboard only.
fn insurance_cards_lane(
    tiles: &[crate::handlers::insurance::InsuranceCardTile],
) -> Markup {
    let cards = html! {
        @for t in tiles {
            @let plan_url = format!("/insurance/{}", t.plan.id);
            @match (&t.current_front) {
                Some(card) => {
                    @let img_url = format!("/insurance/cards/{}/file", card.id);
                    a href=(plan_url) class="block rounded-lg border border-line bg-surface shadow-xs overflow-hidden hover:border-brand hover:no-underline" {
                        img src=(img_url)
                            alt={ (t.plan.payer_name) " insurance card, front" }
                            loading="lazy"
                            class="block w-full h-44 object-cover bg-slate-900";
                        div class="p-3" {
                            div class="text-sm font-semibold text-ink" { (t.plan.payer_name) }
                            @if let Some(pn) = &t.plan.plan_name {
                                p class="text-xs text-muted" { (pn) }
                            }
                        }
                    }
                }
                None => {
                    a href=(plan_url) class="block rounded-lg border border-dashed border-line bg-surface p-3 hover:border-brand hover:no-underline text-center" {
                        div class="text-sm font-medium text-ink" { (t.plan.payer_name) }
                        p class="text-xs text-muted mt-1" { "No card on file" }
                    }
                }
            }
        }
    };
    html! {
        section class="mb-6" {
            div class="flex items-baseline justify-between mb-2" {
                (c::section_heading("Insurance cards"))
                (c::link_subtle("/insurance", "Manage →"))
            }
            div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3" { (cards) }
        }
    }
}

/// A subject-scoped clinical at-a-glance for the home dashboard: a row of count
/// tiles (problems / meds / allergies / immunizations / vitals / appointments)
/// so a subject whose data is purely clinical — e.g. an immunizations-only
/// import — doesn't land on an empty-looking page of "No events / No records".
/// Each tile links to the best detail view; the full chart is one click away.
fn clinical_snapshot(s: &ClinicalSnapshot) -> Markup {
    let sid = s.subject_id;
    let n = &s.counts;
    let chart = format!("/subjects/{sid}");
    let imm = format!("/subjects/{sid}/immunizations");
    let appts = format!("/subjects/{sid}/appointments");
    html! {
        section class="mb-6" {
            div class="flex items-baseline justify-between mb-2" {
                (c::section_heading("Clinical snapshot"))
                (c::link_subtle(&chart, "Open chart →"))
            }
            (c::stat_grid(html! {
                (c::stat_tile(&chart, n.problems, "Problems", n.problems > 0, html! {}))
                (c::stat_tile(&chart, n.medications, "Medications", n.medications > 0, html! {}))
                (c::stat_tile(&chart, n.allergies, "Allergies", n.allergies > 0, html! {}))
                (c::stat_tile(&imm, n.immunizations, "Immunizations", n.immunizations > 0, html! {
                    @if n.vaccines_due > 0 { (c::badge_warn(format!("{} due", n.vaccines_due))) }
                }))
                (c::stat_tile(&chart, n.vitals, "Vitals & labs", n.vitals > 0, html! {}))
                (c::stat_tile(&appts, n.appointments, "Appointments", n.appointments > 0, html! {}))
            }))
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

/// The reusable timeline body (legend + axis), shared by the full `/timeline`
/// page and the subject chart. `tabs` shows the duration selector. Tabs come
/// from `TIMELINE_RANGES`; the legend from the `timeline_kinds` registry.
pub fn timeline_widget(data: &TimelineData, tabs: bool) -> Markup {
    let base = match data.subject {
        Some(id) => format!("/timeline?subject={id}"),
        None => "/timeline".to_string(),
    };
    let rurl = |r: &str| format!("{base}{}range={r}", if base.contains('?') { "&" } else { "?" });
    html! {
        @if tabs {
            div class="flex flex-wrap items-center gap-2 mb-3" {
                @for &(key, label, _) in TIMELINE_RANGES {
                    (c::timeline_tab(&rurl(key), label, data.range == key))
                }
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
            @for k in crate::timeline_kinds::KINDS { (c::timeline_legend_item(k.key)) }
        }
        @if data.start.is_empty() {
            // No events at all for this subject — there's no time axis to draw.
            (c::empty_state("No events yet."))
        } @else {
            // Always render the band (even when this window has no events) so the
            // wheel handler's geometry stays stable and pan/zoom keep working —
            // an empty window just shows the axis plus a faint note, never swaps
            // the band away.
            (c::timeline_band(html! {
                @for (pct, label) in &data.ticks { (c::timeline_tick(*pct, label)) }
                @for s in &data.spans {
                    (c::timeline_span_bar(s.start_pct, s.end_pct - s.start_pct, &s.kind, &s.title, s.href.as_deref()))
                }
                @for b in &data.buckets {
                    @let d = b.date.to_string();
                    @let lo = b.events.first().map(|e| e.date.to_string()).unwrap_or_else(|| d.clone());
                    @let hi = b.events.last().map(|e| e.date.to_string()).unwrap_or_else(|| d.clone());
                    @let n = b.events.len();
                    @let aria = format!("{d} · {n} event{}", if n == 1 { "" } else { "s" });
                    (c::timeline_marker(b.pct, &d, &b.kind, n, &day_detail_url(data.subject, &lo, &hi), &aria))
                }
                @if data.buckets.is_empty() && data.spans.is_empty() { (c::timeline_band_empty_note()) }
            }))
        }
        // NOTE: the `#tl-detail` panel is intentionally NOT here — it lives
        // OUTSIDE this widget (and outside the swappable `#tl-inner`), so a
        // marker click's event list persists across zoom/tab/date changes and
        // sits as its own block below the timeline. See `visual_timeline` (full
        // page) and the subject embed.
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

/// Trackpad/wheel navigation for the timeline, matching macOS expectations
/// (the only first-party JS in the app — wheel axis + cursor position can't be
/// read in CSS/htmx):
///   • Horizontal swipe → PAN. Scroll right moves the window to later dates,
///     1:1 with the finger. Always `preventDefault`ed so a sideways swipe can't
///     trigger browser back/forward navigation.
///   • Vertical up → ZOOM IN, down → ZOOM OUT, both about the day under the
///     cursor (it keeps its fraction of the window, so it stays put).
/// It keeps the intended window in JS (`cur`), accumulating sub-day deltas, and
/// `htmx.ajax`-swaps `#tl-inner` to that `from`/`to` (coalesced: one request in
/// flight, the latest window sent on settle). A zoom-in that would land on a
/// blank gap is refused (checks visible markers' `data-d`), so you stop at the
/// tightest populated view; pans clamp to the data range; fully zoomed out, a
/// downward scroll lets the page scroll. `cur` re-syncs from the rendered window
/// on settle, so the tab/date-box swaps stay in step. Init is deferred to
/// `DOMContentLoaded` because htmx loads with `defer`.
const WHEEL_ZOOM_JS: &str = r#"
(function(){
  function init(){
    var z=document.getElementById('tl-zoom'); if(!z||!window.htmx) return;
    var DAY=86400000, base=z.dataset.base;
    var cur=null, mn, mx, busy=false, pending=false, last='';
    function ms(s){ return Date.parse(s); }
    function iso(m){ return new Date(m).toISOString().slice(0,10); }
    function key(){ return cur ? iso(cur.a)+'|'+iso(cur.b) : ''; }
    function sync(){
      var inner=document.getElementById('tl-inner'); if(!inner) return;
      var a=ms(inner.dataset.start), b=ms(inner.dataset.end);
      if(!isNaN(a)&&!isNaN(b)){ cur={a:a,b:b}; last=key(); }
      mn=ms(z.dataset.min); mx=ms(z.dataset.max);
    }
    function send(){
      if(!cur) return;
      var k=key(); if(k===last) return;            // no day-level change yet
      if(busy){ pending=true; return; }            // coalesce: one request in flight
      busy=true; last=k;
      var url=base+(base.indexOf('?')>=0?'&':'?')+'from='+iso(cur.a)+'&to='+iso(cur.b);
      window.htmx.ajax('GET', url, {target:'#tl-inner', swap:'outerHTML'});
    }
    sync();
    z.addEventListener('htmx:afterSettle', function(){
      busy=false;
      if(pending){ pending=false; send(); }        // flush the latest gesture window
      else sync();                                 // adopt the rendered window (tabs/date too)
    });
    function px(e,d){ return e.deltaMode===1 ? d*16 : e.deltaMode===2 ? d*400 : d; }
    z.addEventListener('wheel', function(e){
      var inner=document.getElementById('tl-inner'); if(!inner) return;
      // The empty-state has no [data-tl-band]; fall back to #tl-inner so the
      // wheel still works there and you can always zoom back out.
      var band=z.querySelector('[data-tl-band]')||inner, br=band.getBoundingClientRect();
      if(e.clientY < br.top-28 || e.clientY > br.bottom+28) return;   // off the strip: page scrolls
      if(!cur) sync(); if(!cur) return;
      var full=Math.max(DAY, mx-mn), span=cur.b-cur.a;
      var dx=px(e,e.deltaX), dy=px(e,e.deltaY);
      if(Math.abs(dx) > Math.abs(dy)){
        // Horizontal → pan. Scroll right (dx>0) shifts the window to later dates.
        e.preventDefault();
        var shift=dx/Math.max(1,br.width)*span;
        cur.a+=shift; cur.b+=shift;
        if(cur.a<mn){ cur.a=mn; cur.b=mn+span; }
        if(cur.b>mx){ cur.b=mx; cur.a=mx-span; }
      } else {
        // Vertical → zoom about the cursor. Up (dy<0) in, down (dy>0) out.
        if(dy>0 && span>=full) return;                                // fully out: page scrolls
        e.preventDefault();
        var tf=Math.min(1, Math.max(0, ((e.clientX-br.left)/Math.max(1,br.width)-0.04)/0.92));
        var focal=cur.a+tf*span;
        var ns=Math.min(full, Math.max(14*DAY, span*Math.pow(2, dy/400)));
        if(dy<0 && ns>=span) return;                                  // already at max zoom-in
        if(ns>=full){ cur.a=mn; cur.b=mx; }
        else {
          var a=focal-tf*ns, b=a+ns;
          if(ns<span){                                                // zoom-in: skip blank gaps
            var hit=false, dots=inner.querySelectorAll('[data-d]');
            for(var i=0;i<dots.length;i++){ var dm=ms(dots[i].dataset.d); if(dm>=a&&dm<=b){hit=true;break;} }
            if(!hit) return;
          }
          cur.a=a; cur.b=b;
        }
      }
      send();
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
        // The detail panel sits OUTSIDE #tl-zoom: a marker click hx-swaps its
        // events in here, and because it isn't part of the zoomable #tl-inner it
        // persists across zoom/tab/date changes as its own block below.
        div id="tl-detail" class="mt-6" { (c::timeline_detail_hint()) }
        script { (PreEscaped(WHEEL_ZOOM_JS)) }
    };
    shell(nav, body)
}

pub fn dashboard_timeline_limit() -> usize { DASHBOARD_TIMELINE_LIMIT }
