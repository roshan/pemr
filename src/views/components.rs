//! Reusable UI primitives. **Tailwind class strings only live in this file.**
//! Views and handlers compose pages from these functions instead of
//! hand-crafting class strings in-line.
//!
//! When adding a new visual idiom, write a function here first; if you find
//! yourself reaching for an inline `class="..."` in `views/<page>.rs`, that's
//! a sign a primitive is missing.

#![allow(dead_code)] // primitives may not all be used yet — that's fine for a component library.

use maud::{Markup, PreEscaped, Render, html};

// ---------------------------------------------------------------------------
// Page chrome
// ---------------------------------------------------------------------------

/// Top-level page title (h1). Use once per page.
pub fn page_title(text: impl Render) -> Markup {
    html! { h1 class="text-2xl font-semibold tracking-tight text-ink mb-3" { (text) } }
}

/// Section heading inside a page (h2).
pub fn section_heading(text: impl Render) -> Markup {
    html! { h2 class="text-base font-semibold tracking-tight text-ink mb-2" { (text) } }
}

/// Sub-section heading inside cards or detail panels (h3).
pub fn subheading(text: impl Render) -> Markup {
    html! { h3 class="text-sm font-semibold tracking-tight text-ink mt-3 mb-1" { (text) } }
}

/// A vertically-spaced section with an optional inline action on the right of the heading.
pub fn lane(heading: Markup, body: Markup) -> Markup {
    html! {
        section class="mb-6" {
            div class="flex items-baseline justify-between mb-2 gap-3" { (heading) }
            (body)
        }
    }
}

/// A row of metadata badges/text rendered with consistent gap.
pub fn meta_row(items: Markup) -> Markup {
    html! { div class="flex flex-wrap items-center gap-2 text-xs text-muted" { (items) } }
}

// ---------------------------------------------------------------------------
// Cards / grid
// ---------------------------------------------------------------------------

/// Auto-fitting responsive grid of cards.
pub fn card_grid(children: Markup) -> Markup {
    html! { div class="card-grid" { (children) } }
}

/// A single card. Pass a `Markup` body — usually combine `card_title` + content + footer.
pub fn card(body: Markup) -> Markup {
    html! {
        article class="rounded-lg border border-line bg-surface p-4 shadow-xs" { (body) }
    }
}

pub fn card_title(href: impl Render, label: impl Render) -> Markup {
    html! {
        h3 class="text-sm font-semibold mb-1" {
            a href=(href) class="text-ink hover:text-brand hover:no-underline" { (label) }
        }
    }
}

/// A titled summary panel for the subject clinical chart. Pass the panel body
/// (usually a `panel_list` or an `empty_state`).
pub fn summary_panel(title: impl Render, body: Markup) -> Markup {
    html! {
        section class="rounded-lg border border-line bg-surface p-4 shadow-xs" {
            h3 class="text-sm font-semibold tracking-tight text-ink mb-2" { (title) }
            (body)
        }
    }
}

/// A compact list inside a summary panel.
pub fn panel_list(children: Markup) -> Markup {
    html! { ul class="space-y-1.5 text-sm text-ink" { (children) } }
}

/// One row in a `panel_list`: primary content left, optional muted detail right.
pub fn panel_list_item(primary: Markup, detail: Markup) -> Markup {
    html! {
        li class="flex items-baseline justify-between gap-3" {
            span { (primary) }
            span class="text-xs text-muted whitespace-nowrap" { (detail) }
        }
    }
}

/// Small muted inline text.
pub fn muted(text: impl Render) -> Markup {
    html! { span class="text-xs text-muted" { (text) } }
}

/// A warning-toned badge (e.g. an abnormal lab flag, a due item).
pub fn badge_warn(text: impl Render) -> Markup {
    html! { span class={(BADGE_BASE) " bg-amber-100 text-amber-800"} { (text) } }
}

/// A danger-toned badge (e.g. an overdue item).
pub fn badge_danger(text: impl Render) -> Markup {
    html! { span class={(BADGE_BASE) " bg-rose-100 text-rose-700"} { (text) } }
}

// ---------------------------------------------------------------------------
// Badges
// ---------------------------------------------------------------------------

const BADGE_BASE: &str = "inline-flex items-center rounded px-2 py-0.5 text-xs font-medium whitespace-nowrap";

pub fn badge_subject(text: impl Render) -> Markup {
    html! { span class={(BADGE_BASE) " bg-subject-bg text-subject-fg"} { (text) } }
}
pub fn badge_kind(text: impl Render) -> Markup {
    html! { span class={(BADGE_BASE) " bg-kind-bg text-kind-fg"} { (text) } }
}
pub fn badge_source(text: impl Render) -> Markup {
    html! { span class={(BADGE_BASE) " bg-source-bg text-source-fg"} { (text) } }
}
pub fn badge_neutral(text: impl Render) -> Markup {
    html! { span class={(BADGE_BASE) " bg-slate-100 text-slate-700"} { (text) } }
}

// ---------------------------------------------------------------------------
// Buttons + button-styled links
// ---------------------------------------------------------------------------

const BTN_BASE: &str = "inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed";

pub fn button_primary(label: impl Render) -> Markup {
    html! {
        button type="submit" class={(BTN_BASE) " bg-brand text-white hover:bg-indigo-700"} { (label) }
    }
}

/// Anchor styled as a primary button.
pub fn button_link_primary(href: impl Render, label: impl Render) -> Markup {
    html! {
        a href=(href)
          class={(BTN_BASE) " bg-brand text-white hover:bg-indigo-700 hover:no-underline"} {
            (label)
        }
    }
}

/// Anchor styled as a secondary button.
pub fn button_link_secondary(href: impl Render, label: impl Render) -> Markup {
    html! {
        a href=(href)
          class={(BTN_BASE) " border border-line bg-surface text-ink hover:bg-slate-50 hover:no-underline"} {
            (label)
        }
    }
}

/// Subtle text-button (used for things like "unlink" via HTMX delete).
pub fn button_subtle_danger(label: impl Render, htmx_attrs: HtmxDelete) -> Markup {
    html! {
        button type="button"
            hx-delete=(htmx_attrs.url)
            hx-target=(htmx_attrs.target)
            hx-swap=(htmx_attrs.swap)
            hx-confirm=[htmx_attrs.confirm]
            class={(BTN_BASE) " text-danger hover:bg-rose-50"} { (label) }
    }
}

pub struct HtmxDelete {
    pub url: String,
    pub target: &'static str,
    pub swap: &'static str,
    pub confirm: Option<String>,
}

/// A small inline form+button for a plain (non-HTMX) POST action, e.g. "remove".
pub fn post_button(action: impl Render, label: impl Render) -> Markup {
    html! {
        form action=(action) method="post" class="inline" {
            button type="submit"
                class="rounded px-2 py-0.5 text-xs font-medium text-danger hover:bg-rose-50" {
                (label)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------------

/// Wraps form children in a vertically-stacked layout.
pub fn form(action: impl Render, method: &str, body: Markup) -> Markup {
    html! {
        form action=(action) method=(method) class="space-y-3 max-w-xl" { (body) }
    }
}

pub fn form_multipart(action: impl Render, body: Markup) -> Markup {
    html! {
        form action=(action) method="post" enctype="multipart/form-data" class="space-y-3 max-w-xl" { (body) }
    }
}

const FIELD_INPUT: &str =
    "w-full rounded-md border border-line bg-surface px-3 py-1.5 text-sm text-ink \
     focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand/40 \
     placeholder:text-slate-400";

pub fn field<S: AsRef<str>>(label_text: S, control: Markup) -> Markup {
    html! {
        label class="block space-y-1" {
            span class="block text-sm font-medium text-ink" { (label_text.as_ref()) }
            (control)
        }
    }
}

pub fn field_with_hint<S: AsRef<str>>(label_text: S, hint: S, control: Markup) -> Markup {
    html! {
        label class="block space-y-1" {
            span class="block text-sm font-medium text-ink" { (label_text.as_ref()) }
            (control)
            span class="block text-xs text-muted" { (hint.as_ref()) }
        }
    }
}

pub fn input_text(name: &str, value: &str, required: bool, max_length: Option<u32>) -> Markup {
    html! {
        input
            type="text"
            name=(name)
            value=(value)
            required[required]
            maxlength=[max_length.map(|m| m.to_string())]
            class=(FIELD_INPUT);
    }
}

pub fn input_url(name: &str, value: &str) -> Markup {
    html! { input type="url" name=(name) value=(value) class=(FIELD_INPUT); }
}

pub fn input_email(name: &str, value: &str, placeholder: Option<&str>) -> Markup {
    html! {
        input type="email" name=(name) value=(value) placeholder=[placeholder] class=(FIELD_INPUT);
    }
}

pub fn input_date(name: &str, value: &str) -> Markup {
    html! { input type="date" name=(name) value=(value) class=(FIELD_INPUT); }
}

pub fn input_datetime(name: &str, value: &str, required: bool) -> Markup {
    html! {
        input type="datetime-local" name=(name) value=(value) required[required] class=(FIELD_INPUT);
    }
}

pub fn checkbox(name: &str, label: &str, checked: bool) -> Markup {
    html! {
        label class="inline-flex items-center gap-2 text-sm text-ink" {
            input type="checkbox" name=(name) value="on" checked[checked] class="rounded border-line";
            span { (label) }
        }
    }
}

pub fn input_file(name: &str) -> Markup {
    html! {
        input type="file" name=(name)
            class="block w-full text-sm text-ink \
                   file:mr-3 file:rounded-md file:border-0 file:bg-brand-soft file:text-brand-ink \
                   file:px-3 file:py-1.5 file:text-sm file:font-medium hover:file:bg-indigo-200";
    }
}

pub fn input_search(
    name: &str,
    placeholder: &str,
    hx_get: &str,
    hx_target: &str,
    hx_include: &str,
) -> Markup {
    html! {
        input
            type="search"
            name=(name)
            placeholder=(placeholder)
            autofocus
            hx-get=(hx_get)
            hx-trigger="keyup changed delay:300ms, search"
            hx-target=(hx_target)
            hx-include=(hx_include)
            hx-push-url="false"
            class={(FIELD_INPUT) " text-base"};
    }
}

pub fn select_field<F>(name: &str, required: bool, options: F) -> Markup
where
    F: FnOnce() -> Markup,
{
    html! {
        select name=(name) required[required] class=(FIELD_INPUT) {
            (options())
        }
    }
}

pub fn select_option(value: impl std::fmt::Display, label: impl Render, selected: bool) -> Markup {
    let v = value.to_string();
    html! { option value=(v) selected[selected] { (label) } }
}

pub fn textarea_field(name: &str, value: &str, rows: u32) -> Markup {
    html! {
        textarea
            name=(name)
            rows=(rows)
            class={(FIELD_INPUT) " font-mono"} { (value) }
    }
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

pub fn data_table(thead: Markup, tbody: Markup) -> Markup {
    html! {
        div class="overflow-x-auto rounded-lg border border-line bg-surface" {
            table class="min-w-full text-sm" {
                thead class="bg-slate-50 text-left text-xs uppercase tracking-wide text-muted" {
                    (thead)
                }
                tbody class="divide-y divide-line" { (tbody) }
            }
        }
    }
}

pub fn th(label: impl Render) -> Markup {
    html! { th class="px-3 py-2 font-medium" { (label) } }
}
pub fn td(content: Markup) -> Markup {
    html! { td class="px-3 py-2 align-top" { (content) } }
}

// ---------------------------------------------------------------------------
// Boxes (errors, info, etc.)
// ---------------------------------------------------------------------------

pub fn alert_danger(text: impl Render) -> Markup {
    html! {
        div class="rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-900" {
            (text)
        }
    }
}

pub fn alert_info(text: impl Render) -> Markup {
    html! {
        div class="rounded-md border border-line bg-slate-50 px-3 py-2 text-sm text-muted" {
            (text)
        }
    }
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

pub fn empty_state(text: impl Render) -> Markup {
    html! { p class="text-sm text-muted italic" { (text) } }
}

pub fn collapse_section<S: AsRef<str>>(summary: S, body: Markup, open: bool) -> Markup {
    html! {
        details open[open] class="rounded-lg border border-line bg-surface" {
            summary class="cursor-pointer select-none px-4 py-2 text-sm font-medium text-ink" {
                (summary.as_ref())
            }
            div class="border-t border-line px-4 py-3" { (body) }
        }
    }
}

/// Renders user-entered prose (markdown for v1 = newlines → <br>) safely.
/// We deliberately escape via maud's default and only allow newline-as-break.
pub fn prose(text: &str) -> Markup {
    html! {
        div class="text-sm text-ink whitespace-pre-wrap leading-relaxed" {
            (text)
        }
    }
}

/// Inline code-styled span.
pub fn code(text: impl Render) -> Markup {
    html! { code class="rounded bg-slate-100 px-1.5 py-0.5 text-xs text-ink" { (text) } }
}

/// "↗" external link arrow — renders nothing if href is empty.
pub fn external_link(href: Option<&str>) -> Markup {
    match href {
        Some(h) if !h.is_empty() => html! {
            a href=(h) target="_blank" rel="noreferrer"
              class="text-muted hover:text-brand" { (PreEscaped("↗")) }
        },
        _ => html! {},
    }
}

/// A small inline text link (e.g. "View full timeline →").
pub fn link_subtle(href: impl Render, label: impl Render) -> Markup {
    html! { a href=(href) class="text-sm text-brand hover:underline" { (label) } }
}

// ---------------------------------------------------------------------------
// Horizontal event timeline (/timeline). Positioning (left%, width) is passed as
// inline `style` since it's dynamic; everything visual stays in classes here.
// ---------------------------------------------------------------------------

/// Per-event-kind dot colour (standard palette literals so the scanner sees them).
fn timeline_kind_color(kind: &str) -> &'static str {
    match kind {
        "incident" => "bg-rose-500",
        "record" => "bg-indigo-500",
        "condition" => "bg-amber-500",
        "immunization" => "bg-emerald-500",
        "appointment" => "bg-sky-500",
        _ => "bg-slate-400", // observation / other
    }
}

pub fn timeline_kind_label(kind: &str) -> &'static str {
    match kind {
        "incident" => "Incident",
        "record" => "Record",
        "condition" => "Condition",
        "immunization" => "Immunization",
        "appointment" => "Appointment",
        "observation" => "Observation",
        _ => "Event",
    }
}

/// Duration-selector tab (a plain link; the page re-renders for the window).
pub fn timeline_tab(href: impl Render, label: impl Render, active: bool) -> Markup {
    let cls = if active {
        "rounded-md bg-brand px-3 py-1 text-sm font-medium text-white"
    } else {
        "rounded-md border border-line px-3 py-1 text-sm text-muted hover:bg-brand-soft hover:text-brand-ink"
    };
    html! { a href=(href) class=(cls) { (label) } }
}

/// Legend entry: dot + kind name.
pub fn timeline_legend_item(kind: &str) -> Markup {
    html! {
        span class="inline-flex items-center gap-1.5 text-xs text-muted" {
            span class={ "inline-block w-2.5 h-2.5 rounded-full " (timeline_kind_color(kind)) } {}
            (timeline_kind_label(kind))
        }
    }
}

/// A frameless, full-width band with a faint time axis. Events are positioned
/// by percentage and fit the width — no box, no dead scroll space. Markers'
/// popovers overflow the band on hover (it doesn't clip).
pub fn timeline_band(inner: Markup) -> Markup {
    html! {
        div class="relative h-16 w-full" {
            div class="absolute left-0 right-0 top-6 border-t border-line" {}
            (inner)
        }
    }
}

/// A month/year axis label, placed below the axis at its time position.
pub fn timeline_tick(left_pct: f64, label: impl Render) -> Markup {
    html! {
        span class="absolute top-9 -translate-x-1/2 text-xs text-muted whitespace-nowrap"
            style={ "left:" (format!("{left_pct:.3}")) "%" } { (label) }
    }
}

/// A positioned event marker: a coloured dot (sized by count) sitting on the
/// axis, with a popover listing that day's events. Focusable + revealed on hover
/// or focus, so it works for keyboard/touch, not just mouse. The exact date is
/// the popover header (the dots themselves stay unlabelled to avoid collisions).
pub fn timeline_marker(left_pct: f64, kind: &str, count: usize, popover: Markup) -> Markup {
    let size = if count >= 8 {
        "w-5 h-5"
    } else if count > 1 {
        "w-4 h-4"
    } else {
        "w-3 h-3"
    };
    html! {
        div class="group absolute top-6 -translate-x-1/2 -translate-y-1/2 z-10 focus:outline-none"
            tabindex="0"
            style={ "left:" (format!("{left_pct:.3}")) "%" } {
            span class={ "flex items-center justify-center rounded-full ring-2 ring-white group-hover:ring-brand group-focus:ring-brand cursor-pointer " (timeline_kind_color(kind)) " " (size) } {
                @if count > 1 {
                    span class="text-xs font-bold leading-none text-white" { (count) }
                }
            }
            div class="hidden group-hover:block group-focus-within:block absolute top-full left-1/2 -translate-x-1/2 mt-2 w-64 max-h-40 overflow-y-auto rounded-lg border border-line bg-surface p-3 shadow-lg text-left z-20" {
                (popover)
            }
        }
    }
}

/// Popover body: a date header + the event rows.
pub fn timeline_popover(date_label: impl Render, rows: Markup) -> Markup {
    html! {
        div class="text-xs font-semibold text-ink mb-1.5" { (date_label) }
        (rows)
    }
}

/// One event row inside a popover. `trailing` is an optional subject badge.
pub fn timeline_event_row(kind: &str, title: impl Render, href: Option<&str>, trailing: Markup) -> Markup {
    let dot = html! { span class={ "inline-block w-2 h-2 rounded-full mr-1.5 shrink-0 " (timeline_kind_color(kind)) } {} };
    html! {
        div class="flex items-baseline gap-1 text-sm py-0.5" {
            @match href {
                Some(h) => a href=(h) class="text-ink hover:text-brand" { (dot) (title) },
                None => span class="text-ink" { (dot) (title) },
            }
            (trailing)
        }
    }
}
