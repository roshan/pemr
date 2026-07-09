//! The unified **Import** page: pick an *import type*, then fill in that method's
//! form (htmx-swapped into `#import-form`, so the page stays one picker + one
//! form rather than a growing stack). The run history persists below. Adding a
//! new import method = one `IMPORT_TYPES` entry (its form fn owns the whole
//! section). See `handlers::import` (page + picker + file upload) and
//! `handlers::sync` (the CDPH portal import).

use maud::{Markup, html};

use crate::importer;
use crate::models::Subject;
use crate::sync::SyncJob;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};
use crate::views::sync;

/// The result of a submitted file upload, rendered above the upload form.
pub enum Outcome {
    Error(String),
    Preview(importer::Preview),
    Committed(importer::Counts),
}

/// Everything a method's form might need to render itself (and its submit
/// result). Each form fn picks what it uses.
pub struct FormCtx<'a> {
    pub subjects: &'a [Subject],
    /// Prefill for the EHI source field.
    pub source_value: &'a str,
    /// Result of a submitted EHI upload.
    pub ehi: Option<Outcome>,
    /// Result of a submitted CDPH import: (status, message).
    pub vaccine_result: Option<(&'a str, &'a str)>,
}

pub struct ImportType {
    /// URL-safe key the picker passes back (`?import_type=<key>`).
    pub key: &'static str,
    /// The picker option's label.
    pub label: &'static str,
    /// Renders this method's whole section (heading, blurb, result, form) —
    /// what the picker swaps into `#import-form`.
    pub form: fn(&FormCtx) -> Markup,
}

/// Import methods, in picker order (first entry = the default). The single
/// source of truth: each entry owns its form, so adding a method is one entry
/// here (plus its POST handler). Mirrors the `subject_modules` registry.
pub const IMPORT_TYPES: &[ImportType] = &[
    ImportType { key: "ehi", label: "Epic EHI export (file upload)", form: ehi_section },
    ImportType { key: "dvr", label: "CDPH Digital Vaccination Record", form: dvr_section },
];

/// The picker's default (and the fallback for an unknown key).
pub fn default_type() -> &'static str {
    IMPORT_TYPES[0].key
}

pub fn get(key: &str) -> Option<&'static ImportType> {
    IMPORT_TYPES.iter().find(|t| t.key == key)
}

pub fn page(nav: &Nav<'_>, jobs: &[SyncJob], selected_type: &str, ctx: &FormCtx<'_>) -> Markup {
    let body = html! {
        (c::page_title("Import"))
        p class="mb-6 max-w-xl text-sm text-muted" {
            "Bring clinical data into the EMR. Pick what you're importing, then fill in that form."
        }

        section class="mb-6 max-w-xl" {
            (c::field_with_hint(
                "Import type",
                "Choose the source. More types appear here as we add them.",
                c::hx_select("import_type", "/settings/import/form", "#import-form", html! {
                    @for t in IMPORT_TYPES {
                        (c::select_option(t.key, t.label, t.key == selected_type))
                    }
                }),
            ))
        }

        // Swapped in place by the picker; the picker itself lives outside so it persists.
        div id="import-form" {
            (type_form(selected_type, ctx))
        }

        (sync::history_table(jobs))
    };
    shell(nav, body)
}

/// The form for the selected import type — what the picker swaps into
/// `#import-form`, and what a submit re-renders (carrying its result).
pub fn type_form(key: &str, ctx: &FormCtx<'_>) -> Markup {
    match get(key) {
        Some(t) => (t.form)(ctx),
        None => c::alert_info("Unsupported import type."),
    }
}

fn ehi_section(ctx: &FormCtx<'_>) -> Markup {
    html! {
        (c::section_heading("Epic EHI export"))
        div class="mb-4 mt-1 max-w-xl space-y-2 text-sm text-muted" {
            p {
                "The \"Requested Records\" / \"Computer-readable EHI export\" zip from a MyChart records request. "
                strong class="text-ink" { "Preview" }
                " to see what will import, then "
                strong class="text-ink" { "Import" }
                ". Re-importing the same export is safe (idempotent)."
            }
        }
        @if let Some(o) = &ctx.ehi {
            div class="mb-4 max-w-xl" { (outcome_view(o)) }
        }
        (ehi_form(ctx.subjects, ctx.source_value))
    }
}

fn dvr_section(ctx: &FormCtx<'_>) -> Markup {
    html! {
        (c::section_heading("CDPH Digital Vaccination Record"))
        div class="mt-1" { (sync::dvr_form(ctx.subjects, ctx.vaccine_result)) }
    }
}

fn ehi_form(subjects: &[Subject], source_value: &str) -> Markup {
    c::form_multipart("/settings/import", html! {
        (c::field_with_hint(
            "Subject",
            "Required — an EHI export names the patient only by internal id, so we never guess.",
            c::select_field("subject_id", true, || html! {
                option value="" disabled selected { "— select a subject —" }
                @for s in subjects {
                    (c::select_option(s.id, html! { (s.given_name) " " (s.family_name) }, false))
                }
            }),
        ))
        (c::field_with_hint(
            "Source",
            "Where these records came from — used for provenance and dedup. Keep it stable across re-imports.",
            c::input_text("source", source_value, true, Some(120)),
        ))
        (c::field_with_hint(
            "EHI export (.zip)",
            "The zip containing an EHITables/ folder.",
            c::input_file("file"),
        ))
        div class="flex items-center gap-2" {
            (c::submit_action("action", "preview", "Preview", false))
            (c::submit_action("action", "commit", "Import", true))
        }
    })
}

fn outcome_view(o: &Outcome) -> Markup {
    match o {
        Outcome::Error(msg) => c::alert_danger(msg),
        Outcome::Preview(p) => c::card(html! {
            p class="mb-2 text-sm font-medium text-ink" { "Dry run — nothing written yet. Review, then click Import." }
            (counts_table(&p.counts, p.labs, p.vitals))
            (warnings_view(&p.counts.warnings))
            @if !p.samples.is_empty() {
                div class="mt-3" {
                    (c::collapse_section(
                        format!("Show {} extracted rows", p.samples.len()),
                        c::mono_lines(&p.samples),
                        false,
                    ))
                }
            }
        }),
        Outcome::Committed(counts) => c::card(html! {
            p class="mb-2 text-sm font-medium text-ink" { "✓ Imported. Re-running is safe (idempotent)." }
            (counts_table(counts, 0, 0))
            (warnings_view(&counts.warnings))
        }),
    }
}

fn counts_table(counts: &importer::Counts, labs: i64, vitals: i64) -> Markup {
    // Preview splits observations into vitals/labs; a commit shows just the total.
    let observations = if labs + vitals > 0 {
        format!("{} ({vitals} vitals, {labs} labs)", counts.observations)
    } else {
        counts.observations.to_string()
    };
    let rows: [(&str, String); 6] = [
        ("Immunizations", counts.immunizations.to_string()),
        ("Observations", observations),
        ("Conditions", counts.conditions.to_string()),
        ("Medications", counts.medications.to_string()),
        ("Incidents", counts.incidents.to_string()),
        ("Allergies", counts.allergies.to_string()),
    ];
    c::data_table(
        html! { tr { (c::th("Type")) (c::th("Count")) } },
        html! {
            @for (label, val) in rows {
                tr {
                    (c::td(html! { (label) }))
                    (c::td(html! { span class="font-medium" { (val) } }))
                }
            }
        },
    )
}

fn warnings_view(warnings: &[String]) -> Markup {
    html! {
        @if !warnings.is_empty() {
            div class="mt-3 space-y-1" {
                p class="text-xs font-medium text-muted" { "Not imported / notes:" }
                @for w in warnings {
                    (c::alert_info(w))
                }
            }
        }
    }
}
