use maud::{Markup, html};
use time::OffsetDateTime;

use crate::sync::SyncJob;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub fn page(
    nav: &Nav<'_>,
    jobs: &[SyncJob],
    import_result: Option<(&str, &str)>,
) -> Markup {
    let body = html! {
        (c::page_title("Sync"))

        // Vaccine import section
        section class="mb-8" {
            (c::section_heading("Import vaccine records (CDPH)"))
            p class="text-sm text-muted mb-4" {
                "Go to "
                a href="https://myvaccinerecord.cdph.ca.gov" target="_blank" rel="noopener"
                  class="text-brand hover:underline" {
                    "myvaccinerecord.cdph.ca.gov"
                }
                ", complete the lookup, and paste the links from the email you receive."
                " Each link is valid for 24 hours — import before they expire."
                " One link per family member is fine; paste all on separate lines."
            }

            @if let Some((status, message)) = import_result {
                div class="mb-4" {
                    @if status == "ok" {
                        (c::card(html! {
                            p class="text-sm text-ink" { (message) }
                        }))
                    } @else {
                        (c::alert_danger(message))
                    }
                }
            }

            (c::form("/settings/sync/vaccine-import", "post", html! {
                (c::field_with_hint(
                    "CDPH vaccine record URLs",
                    "Paste one URL per line — one per family member from the CDPH email.",
                    html! {
                        textarea name="urls" rows="4"
                            placeholder="https://myvaccinerecord.cdph.ca.gov/qr/en/DVR/…"
                            class="w-full rounded-md border border-line bg-surface px-3 py-2 text-sm font-mono text-ink placeholder:text-muted focus:outline-none focus:ring-2 focus:ring-brand/40" {}
                    },
                ))
                (c::button_primary("Import now"))
            }))
        }

        // History of past imports / scheduled tasks
        @if !jobs.is_empty() {
            (c::lane(
                html! { (c::section_heading("History")) },
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
    };
    shell(nav, body)
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
