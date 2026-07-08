use maud::{Markup, html};
use time::OffsetDateTime;

use crate::models::Subject;
use crate::sync::{self, SyncJob};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub fn page(
    nav: &Nav<'_>,
    jobs: &[SyncJob],
    subjects: &[Subject],
    provider_key: &str,
    import_result: Option<(&str, &str)>,
) -> Markup {
    let body = html! {
        (c::page_title("Sync"))

        section class="mb-6 max-w-xl" {
            (c::field_with_hint(
                "Sync source",
                "Choose which portal to import from. More sources will appear here as we add them.",
                c::hx_select("provider", "/settings/sync/form", "#sync-form", html! {
                    @for p in sync::SYNC_PROVIDERS {
                        (c::select_option(p.key, p.label, p.key == provider_key))
                    }
                }),
            ))
        }

        // Swapped in place by the picker; the picker itself lives outside so it persists.
        div id="sync-form" {
            (provider_form(provider_key, subjects, import_result))
        }

        (history_table(jobs))
    };
    shell(nav, body)
}

/// The sync-run history table (import runs recorded in `sync_jobs`). Shared by
/// the unified import page.
pub fn history_table(jobs: &[SyncJob]) -> Markup {
    html! {
        @if !jobs.is_empty() {
            (c::lane(
                html! { (c::section_heading("Recent import runs")) },
                html! {
                    (c::data_table(
                        html! { tr {
                            (c::th("Task"))
                            (c::th("Status"))
                            (c::th("Last run"))
                            (c::th("Message"))
                        } },
                        html! {
                            @for job in jobs {
                                (job_row(job))
                            }
                        },
                    ))
                },
            ))
        }
    }
}

/// The import form for the selected sync source. This is what the picker swaps
/// into `#sync-form`, so it's also rendered directly on first page load. Owns the
/// heading + blurb (from the registry) so every provider gets a consistent header.
pub fn provider_form(
    provider_key: &str,
    subjects: &[Subject],
    import_result: Option<(&str, &str)>,
) -> Markup {
    html! {
        section class="mb-8" {
            @if let Some(p) = sync::provider(provider_key) {
                (c::section_heading(p.label))
                p class="mb-4 text-sm text-muted" { (p.blurb) }
            }
            @match provider_key {
                "dvr" => (dvr_form(subjects, import_result)),
                _ => (c::alert_info("This sync source isn't supported yet.")),
            }
        }
    }
}

/// CDPH Digital Vaccination Record import form (details + form). Rendered on the
/// unified import page under its own heading.
pub fn dvr_form(subjects: &[Subject], import_result: Option<(&str, &str)>) -> Markup {
    html! {
        div class="mb-4 space-y-2 text-sm text-muted" {
            p {
                strong class="text-ink" { "Option A — paste the link(s) from the CDPH email" }
                ", one per line. Links expire in 24 h — import before they do."
            }
            p {
                strong class="text-ink" { "Option B — paste the page HTML" }
                " if Option A fails: open the link in a browser, press "
                code class="text-xs bg-slate-100 px-1 rounded" { "F12" }
                " → Elements tab → right-click the "
                code class="text-xs bg-slate-100 px-1 rounded" { "<html>" }
                " node → Copy → Copy outerHTML, then paste below. One person at a time."
            }
            p {
                "Re-importing the same record is safe — immunizations are matched per "
                "subject on vaccine + date, so a repeat import updates in place instead of "
                "creating duplicates."
            }
        }

        @if let Some((status, message)) = import_result {
            div class="mb-4" {
                @if status == "ok" {
                    (c::card(html! { p class="text-sm text-ink" { (message) } }))
                } @else {
                    (c::alert_danger(message))
                }
            }
        }

        (c::form("/settings/sync/vaccine-import", "post", html! {
            (c::field_with_hint(
                "Subject",
                "Required — pick whose record this is. CDPH labels minors as \"Dependent Minor N\", so imports never guess the subject.",
                c::select_field("subject_id", true, || html! {
                    option value="" disabled selected { "— select a subject —" }
                    @for s in subjects {
                        (c::select_option(s.id, html! { (s.given_name) " " (s.family_name) }, false))
                    }
                }),
            ))
            (c::field(
                "CDPH link(s) or page HTML",
                html! {
                    textarea name="urls" rows="5"
                        placeholder="https://myvaccinerecord.cdph.ca.gov/qr/en/DVR/…\nhttps://myvaccinerecord.cdph.ca.gov/qr/en/DVR/…"
                        class="w-full rounded-md border border-line bg-surface px-3 py-2 text-sm font-mono text-ink placeholder:text-muted focus:outline-none focus:ring-2 focus:ring-brand/40" {}
                },
            ))
            (c::button_primary("Import"))
        }))
    }
}

fn job_row(job: &SyncJob) -> Markup {
    html! {
        tr class="hover:bg-slate-50" {
            (c::td(html! { span class="font-medium" { (job.name.replace('_', " ")) } }))
            (c::td(html! { (status_badge(job.last_status.as_deref())) }))
            (c::td(html! { (fmt_ts(job.last_finished_at)) }))
            (c::td(html! {
                @if let Some(msg) = &job.last_message {
                    span class="text-xs text-muted" { (msg) }
                } @else {
                    "—"
                }
            }))
        }
    }
}

fn status_badge(status: Option<&str>) -> Markup {
    match status {
        None => html! { span class="text-xs text-muted" { "never run" } },
        Some("ok") => html! { (c::badge_neutral("ok")) },
        Some("running") => html! { (c::badge_warn("running")) },
        Some("error") => html! { span class="text-xs font-medium text-danger" { "error" } },
        Some(other) => html! { span class="text-xs text-muted" { (other) } },
    }
}

fn fmt_ts(t: Option<OffsetDateTime>) -> String {
    match t {
        None => "—".into(),
        Some(d) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02} UTC",
            d.year(),
            u8::from(d.month()),
            d.day(),
            d.hour(),
            d.minute(),
        ),
    }
}
