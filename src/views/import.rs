//! Web upload page for structured clinical bundles (Epic EHI export). Pick a
//! subject, drop the zip, Preview (dry-run) then Import. See
//! `handlers::import` for the request side.

use maud::{Markup, html};

use crate::importer;
use crate::models::Subject;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

/// The result of a submitted upload, rendered above the form.
pub enum Outcome {
    Error(String),
    Preview(importer::Preview),
    Committed(importer::Counts),
}

pub fn page(nav: &Nav<'_>, subjects: &[Subject], source_value: &str, outcome: Option<Outcome>) -> Markup {
    let body = html! {
        (c::page_title("Import records"))

        div class="mb-4 max-w-xl space-y-2 text-sm text-muted" {
            p {
                "Upload an Epic "
                strong class="text-ink" { "EHI export" }
                " — the \"Requested Records\" / \"Computer-readable EHI export\" zip from a MyChart records request. Pick the subject, "
                strong class="text-ink" { "Preview" }
                " to see what will be imported, then "
                strong class="text-ink" { "Import" }
                "."
            }
            p { "Re-importing the same export is safe — rows upsert on their source key, so a repeat run updates in place instead of duplicating." }
        }

        @if let Some(o) = &outcome {
            div class="mb-6 max-w-xl" { (outcome_view(o)) }
        }

        (c::form_multipart("/settings/import", html! {
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
        }))
    };
    shell(nav, body)
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
