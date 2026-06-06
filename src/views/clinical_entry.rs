//! Per-subject clinical-entry forms (PEMR-3 UI): add allergies, medications,
//! conditions, immunizations, observations. The chart shows these read-only;
//! this page is the browser write path (the API is the other). Form POST →
//! redirect back to the chart.

use maud::{Markup, html};

use crate::models::{
    ALLERGY_CATEGORIES, ALLERGY_SEVERITIES, ALLERGY_STATUSES, CONDITION_STATUSES,
    IMMUNIZATION_STATUSES, MEDICATION_STATUSES, OBSERVATION_CATEGORIES, Subject,
};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

fn opt_select(name: &str, blank: &str, options: &[&str], default: &str) -> Markup {
    c::select_field(name, false, || {
        html! {
            (c::select_option("", blank, default.is_empty()))
            @for o in options { (c::select_option(o, *o, *o == default)) }
        }
    })
}

pub fn page(nav: &Nav<'_>, subject: &Subject) -> Markup {
    let sid = subject.id;
    let body = html! {
        (c::page_title(format!("{} — add clinical data", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{sid}"), "← Back to chart"))
        p class="text-sm text-muted mt-2 mb-4" {
            "Manual entry. Bulk/structured data normally comes in via the API (parsed from records)."
        }

        div class="space-y-3" {
            (c::collapse_section("Allergy", html! {
                (c::form(format!("/subjects/{sid}/allergies"), "post", html! {
                    (c::field("Substance", c::input_text("substance", "", true, Some(120))))
                    (c::field("Category", opt_select("category", "— none —", ALLERGY_CATEGORIES, "")))
                    (c::field("Severity", opt_select("severity", "— none —", ALLERGY_SEVERITIES, "")))
                    (c::field("Reaction", c::input_text("reaction", "", false, Some(160))))
                    (c::field("Status", opt_select("status", "active", ALLERGY_STATUSES, "active")))
                    (c::field("Onset date", c::input_date("onset_date", "")))
                    (c::field("Notes", c::textarea_field("notes", "", 2)))
                    (c::button_primary("Add allergy"))
                }))
            }, false))

            (c::collapse_section("Medication", html! {
                (c::form(format!("/subjects/{sid}/medications"), "post", html! {
                    (c::field("Name", c::input_text("name", "", true, Some(120))))
                    (c::field("Dose", c::input_text("dose", "", false, Some(60))))
                    (c::field("Frequency", c::input_text("frequency", "", false, Some(80))))
                    (c::field("Status", opt_select("status", "active", MEDICATION_STATUSES, "active")))
                    (c::field("Started on", c::input_date("started_on", "")))
                    (c::field("Reason", c::input_text("reason", "", false, Some(160))))
                    (c::field("Notes", c::textarea_field("notes", "", 2)))
                    (c::button_primary("Add medication"))
                }))
            }, false))

            (c::collapse_section("Condition (problem)", html! {
                (c::form(format!("/subjects/{sid}/conditions"), "post", html! {
                    (c::field("Name", c::input_text("name", "", true, Some(120))))
                    (c::field("Status", opt_select("status", "active", CONDITION_STATUSES, "active")))
                    (c::field("Onset date", c::input_date("onset_date", "")))
                    (c::field("Severity", c::input_text("severity", "", false, Some(40))))
                    (c::field("Notes", c::textarea_field("notes", "", 2)))
                    (c::button_primary("Add condition"))
                }))
            }, false))

            (c::collapse_section("Immunization", html! {
                (c::form(format!("/subjects/{sid}/immunizations"), "post", html! {
                    (c::field("Vaccine", c::input_text("vaccine", "", true, Some(120))))
                    (c::field_with_hint("CVX code", "Optional standard vaccine code.", c::input_text("code", "", false, Some(10))))
                    (c::field("Date given", c::input_date("occurred_at", "")))
                    (c::field("Dose number", c::input_text("dose_number", "", false, Some(3))))
                    (c::field("Status", opt_select("status", "completed", IMMUNIZATION_STATUSES, "completed")))
                    (c::field("Notes", c::textarea_field("notes", "", 2)))
                    (c::button_primary("Add immunization"))
                }))
            }, false))

            (c::collapse_section("Observation (vital / lab)", html! {
                (c::form(format!("/subjects/{sid}/observations"), "post", html! {
                    (c::field("Display", c::input_text("display", "", true, Some(120))))
                    (c::field("Category", opt_select("category", "vital", OBSERVATION_CATEGORIES, "vital")))
                    (c::field_with_hint("LOINC code", "Growth: height 8302-2, weight 29463-7, head-circ 9843-4.", c::input_text("code", "", false, Some(16))))
                    (c::field("Value (number)", c::input_text("value_num", "", false, Some(20))))
                    (c::field("Unit", c::input_text("unit", "", false, Some(20))))
                    (c::field("Effective date", c::input_date("effective_on", "")))
                    (c::field("Notes", c::textarea_field("notes", "", 2)))
                    (c::button_primary("Add observation"))
                }))
            }, false))
        }
    };
    shell(nav, body)
}
