use maud::{Markup, html};
use time::OffsetDateTime;

use crate::sync::SyncJob;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub fn page(nav: &Nav<'_>, jobs: &[SyncJob]) -> Markup {
    let body = html! {
        (c::page_title("Sync"))
        p class="text-sm text-muted mb-4" {
            "Background tasks that run on a schedule or on demand. "
            "Set up each task by configuring the required fields on the relevant subjects."
        }

        (c::lane(
            html! { (c::section_heading("Tasks")) },
            html! {
                @if jobs.is_empty() {
                    (c::empty_state("No sync tasks registered yet."))
                } @else {
                    (c::data_table(
                        html! { tr {
                            (c::th("Task"))
                            (c::th("Status"))
                            (c::th("Last run"))
                            (c::th("Next run"))
                            (c::th("Last message"))
                            (c::th(""))
                        } },
                        html! {
                            @for job in jobs {
                                (job_row(job))
                            }
                        },
                    ))
                }
            },
        ))

        div class="mt-4" {
            (c::card(html! {
                (c::subheading("Vaccine records task — setup"))
                p class="text-sm text-ink mt-2" {
                    "This task syncs immunization records from California's immunization registry (CAIR) "
                    "via the CDPH Digital Vaccine Record portal."
                }
                ol class="mt-3 space-y-1 text-sm text-ink list-decimal list-inside" {
                    li {
                        "Go to "
                        a href="https://myvaccinerecord.cdph.ca.gov" target="_blank"
                          class="text-brand hover:underline" {
                            "myvaccinerecord.cdph.ca.gov"
                        }
                        " and complete the lookup (name + DOB + phone/email + PIN)."
                    }
                    li {
                        "On the results page, right-click the "
                        strong { "Download" }
                        " button and choose "
                        em { "Copy link address" }
                        " — "
                        "or download the file and note the URL."
                    }
                    li {
                        "Paste that URL into the "
                        strong { "CDPH vaccine record URL" }
                        " field on the subject's edit page. "
                        "Do this for each family member."
                    }
                }
                p class="mt-3 text-xs text-muted" {
                    "The task runs weekly and re-fetches the same URL. "
                    "If the URL expires, update it by repeating the process above."
                }
            }))
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
            (c::td(html! { (fmt_ts(Some(job.next_run_at))) }))
            (c::td(html! {
                @if let Some(msg) = &job.last_message {
                    span class="text-xs text-muted" { (msg) }
                } @else {
                    "—"
                }
            }))
            (c::td(html! { (run_button(&job.name)) }))
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

fn run_button(name: &str) -> Markup {
    html! {
        form action={ "/settings/sync/" (name) "/run" } method="post" {
            button type="submit"
                   class="inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium text-brand border border-line hover:bg-brand/5" {
                "Run now"
            }
        }
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
