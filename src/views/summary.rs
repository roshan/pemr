//! Printable one-page health summary (PEMR-27). The clinical sections are
//! rendered by the subject modules in `Mode::Print` (see `subject_modules`); this
//! view is just the header + layout. App chrome is `print:hidden` (see layout), so
//! Browser → Print → Save as PDF yields a clean one-pager to hand a new provider or
//! the ER. The immunizations section doubles as a school/camp printout.

use maud::{Markup, html};

use crate::models::Subject;
use crate::peds;
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub fn page(nav: &Nav<'_>, subject: &Subject, sections: &[Markup]) -> Markup {
    let dob = subject.dob.map(|d| d.to_string()).unwrap_or_else(|| "—".into());
    let body = html! {
        (c::page_title(format!("{} — health summary", subject.full_name)))
        (c::meta_row(html! {
            span { "DOB " (dob) }
            @if let Some(sex) = &subject.sex_at_birth { span class="mx-2 text-muted/60" { "·" } span { (sex) } }
            @if let Some(bt) = &subject.blood_type { span class="mx-2 text-muted/60" { "·" } span { "Blood " (bt) } }
            span class="mx-2 text-muted/60" { "·" }
            span { "generated " (peds::today()) }
        }))
        div class="my-3 print:hidden" {
            (c::alert_info("Use your browser's Print → Save as PDF to export this page. \
                Generated from personal-emr; not a complete medical record."))
        }
        @for sec in sections { (sec) }
    };
    shell(nav, body)
}
