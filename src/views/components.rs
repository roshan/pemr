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

/// Like `summary_panel` but with a "View all →" link in the title bar.
pub fn summary_panel_linked(title: impl Render, href: impl Render, body: Markup) -> Markup {
    html! {
        section class="rounded-lg border border-line bg-surface p-4 shadow-xs" {
            div class="flex items-baseline justify-between mb-2" {
                h3 class="text-sm font-semibold tracking-tight text-ink" { (title) }
                a href=(href) class="text-xs text-brand hover:underline shrink-0" { "View all →" }
            }
            (body)
        }
    }
}

/// Dense grid wrapper for `stat_tile`s (the home-dashboard clinical snapshot).
pub fn stat_grid(children: Markup) -> Markup {
    html! { div class="stat-grid" { (children) } }
}

/// A compact count tile: a big number, a label underneath, and an optional
/// trailing badge (e.g. "2 due"), wrapped as a link. `emphasis` inks a non-zero
/// count and mutes a zero, so a wall of zeros reads as quiet. Used by the
/// home-dashboard clinical snapshot; class strings stay here per the rules.
pub fn stat_tile(
    href: impl Render,
    value: impl Render,
    label: impl Render,
    emphasis: bool,
    badge: Markup,
) -> Markup {
    let num_cls = if emphasis {
        "text-2xl font-semibold tracking-tight text-ink"
    } else {
        "text-2xl font-semibold tracking-tight text-muted"
    };
    html! {
        a href=(href)
          class="block rounded-lg border border-line bg-surface p-3 shadow-xs hover:border-brand hover:no-underline" {
            div class="flex items-baseline justify-between gap-1" {
                span class=(num_cls) { (value) }
                (badge)
            }
            span class="mt-0.5 block text-xs text-muted" { (label) }
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

/// Like [`panel_list_item`], but the primary truncates with an ellipsis instead
/// of growing — for rows whose primary can be a long phrase (e.g. "1 syringe
/// subcutaneous") that would otherwise shove the nowrap detail out of the card.
/// `min-w-0` lets the flex child shrink; `shrink-0` keeps the detail readable.
pub fn panel_list_item_truncated(primary: Markup, detail: Markup) -> Markup {
    html! {
        li class="flex items-baseline justify-between gap-3" {
            span class="min-w-0 truncate" { (primary) }
            span class="text-xs text-muted whitespace-nowrap shrink-0" { (detail) }
        }
    }
}

/// Small muted inline text.
pub fn muted(text: impl Render) -> Markup {
    html! { span class="text-xs text-muted" { (text) } }
}

/// A forecast row that is overdue — rose left-border + tinted background.
pub fn forecast_item_overdue(primary: Markup, detail: Markup) -> Markup {
    html! {
        li class="flex items-baseline justify-between gap-3 rounded-r px-2 py-1 border-l-2 border-rose-400 bg-rose-50" {
            span { (primary) }
            span class="text-xs text-muted whitespace-nowrap" { (detail) }
        }
    }
}

/// A forecast row that is due now — amber left-border + tinted background.
pub fn forecast_item_due(primary: Markup, detail: Markup) -> Markup {
    html! {
        li class="flex items-baseline justify-between gap-3 rounded-r px-2 py-1 border-l-2 border-amber-400 bg-amber-50" {
            span { (primary) }
            span class="text-xs text-muted whitespace-nowrap" { (detail) }
        }
    }
}

/// A forecast row that is upcoming — plain, no highlight.
pub fn forecast_item_upcoming(primary: Markup, detail: Markup) -> Markup {
    html! {
        li class="flex items-baseline justify-between gap-3" {
            span { (primary) }
            span class="text-xs text-muted whitespace-nowrap" { (detail) }
        }
    }
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

/// A submit button carrying a `name`/`value` — for forms with more than one
/// action (e.g. Preview vs Import). `primary` picks the filled vs outline look.
pub fn submit_action(name: &str, value: &str, label: impl Render, primary: bool) -> Markup {
    let variant = if primary {
        " bg-brand text-white hover:bg-indigo-700"
    } else {
        " border border-line bg-surface text-ink hover:bg-slate-50"
    };
    html! {
        button type="submit" name=(name) value=(value) class={(BTN_BASE) (variant)} { (label) }
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

/// A button that hx-GETs `url` into `target` (outerHTML swap) — for in-place
/// navigation like the milestone checkpoint prev/next stepper. When `enabled` is
/// false it renders as an inert, muted control (e.g. no earlier checkpoint).
pub fn hx_nav_button(label: impl Render, url: &str, target: &str, enabled: bool) -> Markup {
    if enabled {
        html! {
            button type="button" hx-get=(url) hx-target=(target) hx-swap="outerHTML"
                class={(BTN_BASE) " border border-line bg-surface text-ink hover:bg-slate-50"} {
                (label)
            }
        }
    } else {
        html! {
            span class={(BTN_BASE) " border border-line bg-slate-50 text-slate-400 cursor-not-allowed"} {
                (label)
            }
        }
    }
}

/// One milestone checklist row — a bordered list item. The view fills `body`
/// with the response controls.
pub fn milestone_row(body: Markup) -> Markup {
    html! {
        li class="py-2 border-b border-line last:border-0" { (body) }
    }
}

/// A milestone response button (Yes / Not yet / No). The response is encoded in
/// the POST **URL** (not a form field) — htmx serializes forms with
/// `new FormData(form)`, which drops submit-button values, so a shared-form +
/// submit-button design would never send the response. This posts a bodyless
/// request to a path that carries the answer, and swaps the checklist back in.
pub fn milestone_mark_button(label: impl Render, url: &str, active: bool) -> Markup {
    let variant = if active {
        " bg-brand text-white hover:bg-indigo-700"
    } else {
        " border border-line bg-surface text-ink hover:bg-slate-50"
    };
    html! {
        button type="button" hx-post=(url) hx-target="#milestone-checklist" hx-swap="outerHTML"
            class={(BTN_BASE) (variant)} { (label) }
    }
}

/// The observed-on date editor shown on a milestone marked "yes". It carries its
/// OWN value (htmx includes a triggering input's `name=value`), so changing the
/// date posts `observed_on` and saves immediately — no separate submit button.
pub fn milestone_observed_input(value: &str, url: &str) -> Markup {
    html! {
        input type="date" name="observed_on" value=(value)
            hx-post=(url) hx-trigger="change" hx-target="#milestone-checklist" hx-swap="outerHTML"
            class="rounded-md border border-line px-2 py-1 text-sm text-ink bg-surface";
    }
}

/// A button that hx-POSTs `url` and swaps the response into `target`
/// (outerHTML) — for the feature registry's add / remove controls (no page
/// reload; the surface "pops in"). `danger` picks the destructive look.
pub fn hx_action_button(label: impl Render, url: &str, target: &str, danger: bool) -> Markup {
    let variant = if danger {
        " text-danger hover:bg-rose-50"
    } else {
        " border border-line bg-surface text-ink hover:bg-slate-50"
    };
    html! {
        button type="button" hx-post=(url) hx-target=(target) hx-swap="outerHTML"
            class={(BTN_BASE) (variant)} { (label) }
    }
}

/// A foldable card — a `<details>`/`<summary>` disclosure styled as a card.
/// Unlike `collapse_section`, the summary accepts arbitrary `Markup` (so it can
/// carry a badge / inline stat), and it has the card's border + shadow. Put
/// interactive controls (buttons) in `body`, not `summary` — a click inside the
/// summary also toggles the fold.
pub fn foldable_card(summary: Markup, body: Markup, open: bool) -> Markup {
    html! {
        details open[open] class="rounded-lg border border-line bg-surface shadow-xs" {
            summary class="cursor-pointer select-none px-4 py-3 text-sm font-semibold text-ink" {
                (summary)
            }
            div class="border-t border-line px-4 py-3" { (body) }
        }
    }
}

/// A small progress meter (filled bar) for per-domain milestone completion.
/// `met`/`total` drive the fill; renders nothing fancy — a labelled bar.
pub fn progress_meter(met: usize, total: usize) -> Markup {
    let pct = if total == 0 { 0.0 } else { (met as f64 / total as f64) * 100.0 };
    html! {
        div class="flex items-center gap-2" {
            div class="h-2 w-24 rounded-full bg-slate-100 overflow-hidden" {
                div class="h-2 rounded-full bg-brand" style={ "width:" (format!("{pct:.0}")) "%" } {}
            }
            span class="text-xs text-muted whitespace-nowrap" { (met) "/" (total) }
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

/// A whole-number input with optional min/max bounds (e.g. gestational weeks).
pub fn input_number(name: &str, value: &str, min: Option<i64>, max: Option<i64>) -> Markup {
    html! {
        input type="number" inputmode="numeric" name=(name) value=(value)
            min=[min.map(|m| m.to_string())] max=[max.map(|m| m.to_string())]
            class=(FIELD_INPUT);
    }
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

/// A `<select>` that htmx-GETs `hx_get` on change (sending its own value as a
/// query param) and swaps the response into `hx_target`. Used by the sync page's
/// "sync source" picker to swap in the selected provider's import form.
pub fn hx_select(name: &str, hx_get: &str, hx_target: &str, options: Markup) -> Markup {
    html! {
        select
            name=(name)
            hx-get=(hx_get)
            hx-trigger="change"
            hx-target=(hx_target)
            class=(FIELD_INPUT) {
            (options)
        }
    }
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

/// A scrollable monospace block — import previews / extracted-row samples.
pub fn mono_lines(lines: &[String]) -> Markup {
    html! {
        div class="max-h-96 overflow-y-auto rounded-md border border-line bg-slate-50 p-3 font-mono text-xs text-ink" {
            @for l in lines { div { (l) } }
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

/// Per-event-kind dot colour, from the `timeline_kinds` registry (the colour
/// literals live there, still in Rust source, so the CSS scanner sees them).
fn timeline_kind_color(kind: &str) -> &'static str {
    crate::timeline_kinds::color(kind)
}

pub fn timeline_kind_label(kind: &str) -> &'static str {
    crate::timeline_kinds::label(kind)
}

/// Compact inline date input for the timeline window controls (not full-width
/// like the form `input_date`).
pub fn timeline_date_input(name: &str, value: &str) -> Markup {
    html! {
        input type="date" name=(name) value=(value)
            class="rounded-md border border-line px-2 py-1 text-sm text-ink bg-surface";
    }
}

/// Duration-selector tab: htmx-swaps the inner band in place; the `href` is a
/// no-JS fallback (full page load).
pub fn timeline_tab(href: &str, label: impl Render, active: bool) -> Markup {
    let cls = if active {
        "rounded-md bg-brand px-3 py-1 text-sm font-medium text-white"
    } else {
        "rounded-md border border-line px-3 py-1 text-sm text-muted hover:bg-brand-soft hover:text-brand-ink"
    };
    html! {
        a href=(href) hx-get=(href) hx-target="#tl-inner" hx-swap="outerHTML" class=(cls) { (label) }
    }
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
        div class="relative h-16 w-full" data-tl-band="1" {
            div class="absolute left-0 right-0 top-6 border-t border-line" {}
            (inner)
        }
    }
}

/// A faint, centered note drawn on the band when the current window has a time
/// axis but no events. Keeps the band (and the wheel handler's geometry) in
/// place so pan/zoom keep working, rather than swapping in a bare empty-state.
/// `pointer-events-none` so it never intercepts the wheel.
pub fn timeline_band_empty_note() -> Markup {
    html! {
        span class="absolute inset-x-0 top-6 -translate-y-1/2 text-center text-xs text-muted pointer-events-none" {
            "No events in this window — scroll to pan, or zoom out"
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

/// A multi-day event drawn as a horizontal bar on the axis, from `left_pct` for
/// `width_pct` (floored so a short span stays visible). Clickable, with a
/// truncated label above and the full title in the tooltip; sits below the dots
/// (z-0) so a point event on the same day still shows on top.
pub fn timeline_span_bar(
    left_pct: f64,
    width_pct: f64,
    kind: &str,
    title: &str,
    href: Option<&str>,
) -> Markup {
    let style = format!("left:{:.3}%; width:{:.3}%", left_pct, width_pct.max(0.8));
    let inner = html! {
        span class="absolute bottom-full left-0 mb-0.5 inline-block max-w-32 truncate text-xs text-muted pointer-events-none" { (title) }
        span class={ "block h-2 rounded-full opacity-60 " (timeline_kind_color(kind)) } {}
    };
    html! {
        @match href {
            Some(h) => a href=(h) title=(title)
                class="group absolute top-6 -translate-y-1/2 z-0" style=(style) { (inner) },
            None => div title=(title)
                class="absolute top-6 -translate-y-1/2 z-0" style=(style) { (inner) },
        }
    }
}

/// A positioned, clickable event marker: a coloured dot (sized by count) on the
/// axis. Clicking it — or activating it from the keyboard (it's a real
/// `<button>`) — hx-gets that point's events into the persistent `#tl-detail`
/// panel below, so the list stays open to click through (unlike a hover popover
/// that vanishes on mouse-out). `date_iso` is exposed as `data-d` so the
/// wheel-zoom script can tell when a proposed window would contain no events.
pub fn timeline_marker(
    left_pct: f64,
    date_iso: &str,
    kind: &str,
    count: usize,
    detail_url: &str,
    aria: &str,
) -> Markup {
    let size = if count >= 8 {
        "w-5 h-5"
    } else if count > 1 {
        "w-4 h-4"
    } else {
        "w-3 h-3"
    };
    html! {
        button type="button"
            class="group absolute top-6 -translate-x-1/2 -translate-y-1/2 z-10 cursor-pointer focus:outline-none"
            data-d=(date_iso) title=(aria) aria-label=(aria)
            hx-get=(detail_url) hx-target="#tl-detail" hx-swap="innerHTML"
            style={ "left:" (format!("{left_pct:.3}")) "%" } {
            span class={ "flex items-center justify-center rounded-full ring-2 ring-white group-hover:ring-brand group-focus:ring-brand " (timeline_kind_color(kind)) " " (size) } {
                @if count > 1 {
                    span class="text-xs font-bold leading-none text-white" { (count) }
                }
            }
        }
    }
}

/// Placeholder shown in the persistent detail panel until a point is selected.
pub fn timeline_detail_hint() -> Markup {
    html! {
        p class="text-sm text-muted" { "Select a point on the timeline to list its events." }
    }
}

/// Contents of the persistent detail panel for one selected point: a date
/// heading, an event count, and the clickable rows.
pub fn timeline_detail(heading: impl Render, count: usize, rows: Markup) -> Markup {
    html! {
        div class="rounded-lg border border-line bg-surface p-4" {
            div class="flex items-baseline justify-between gap-3 mb-2" {
                span class="text-sm font-semibold text-ink" { (heading) }
                span class="text-xs text-muted whitespace-nowrap" {
                    (count) @if count == 1 { " event" } @else { " events" }
                }
            }
            div class="space-y-0.5" { (rows) }
        }
    }
}

/// One event row (a coloured kind-dot + linked title). Used in the detail panel.
/// `trailing` is an optional subject badge.
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
