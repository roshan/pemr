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

/// One add-form on the page. `action` is the POST path suffix under
/// `/subjects/{id}/` (routes stay explicit in `main.rs` — they're sub-actions,
/// like the other non-page routes).
struct Section {
    title: &'static str,
    action: &'static str,
    submit: &'static str,
    fields: fn() -> Markup,
}

/// The entry forms, in display order. Adding a clinical entity = one entry
/// (plus its fields fn, POST handler, and route).
const SECTIONS: &[Section] = &[
    Section { title: "Allergy", action: "allergies", submit: "Add allergy", fields: allergy_fields },
    Section {
        title: "Medication",
        action: "medications",
        submit: "Add medication",
        fields: medication_fields,
    },
    Section {
        title: "Condition (problem)",
        action: "conditions",
        submit: "Add condition",
        fields: condition_fields,
    },
    Section {
        title: "Immunization",
        action: "immunizations",
        submit: "Add immunization",
        fields: immunization_fields,
    },
    Section {
        title: "Observation (vital / lab)",
        action: "observations",
        submit: "Add observation",
        fields: observation_fields,
    },
];

pub fn page(nav: &Nav<'_>, subject: &Subject) -> Markup {
    let sid = subject.id;
    let body = html! {
        (c::page_title(format!("{} — add clinical data", subject.full_name)))
        (c::button_link_secondary(format!("/subjects/{sid}"), "← Back to chart"))
        p class="text-sm text-muted mt-2 mb-4" {
            "Manual entry. Bulk/structured data normally comes in via the API (parsed from records)."
        }

        div class="space-y-3" {
            @for sec in SECTIONS {
                (c::collapse_section(sec.title, c::form(
                    format!("/subjects/{sid}/{}", sec.action),
                    "post",
                    html! { ((sec.fields)()) (c::button_primary(sec.submit)) },
                ), false))
            }
        }
    };
    shell(nav, body)
}

fn allergy_fields() -> Markup {
    html! {
        (c::field("Substance", c::input_text("substance", "", true, Some(120))))
        (c::field("Category", opt_select("category", "— none —", ALLERGY_CATEGORIES, "")))
        (c::field("Severity", opt_select("severity", "— none —", ALLERGY_SEVERITIES, "")))
        (c::field("Reaction", c::input_text("reaction", "", false, Some(160))))
        (c::field("Status", opt_select("status", "active", ALLERGY_STATUSES, "active")))
        (c::field("Onset date", c::input_date("onset_date", "")))
        (c::field("Notes", c::textarea_field("notes", "", 2)))
    }
}

fn medication_fields() -> Markup {
    html! {
        (c::field("Name", c::input_text("name", "", true, Some(120))))
        (c::field("Dose", c::input_text("dose", "", false, Some(60))))
        (c::field("Frequency", c::input_text("frequency", "", false, Some(80))))
        (c::field("Status", opt_select("status", "active", MEDICATION_STATUSES, "active")))
        (c::field("Started on", c::input_date("started_on", "")))
        (c::field("Reason", c::input_text("reason", "", false, Some(160))))
        (c::field("Notes", c::textarea_field("notes", "", 2)))
    }
}

fn condition_fields() -> Markup {
    html! {
        (c::field("Name", c::input_text("name", "", true, Some(120))))
        (c::field("Status", opt_select("status", "active", CONDITION_STATUSES, "active")))
        (c::field("Onset date", c::input_date("onset_date", "")))
        (c::field("Severity", c::input_text("severity", "", false, Some(40))))
        (c::field("Notes", c::textarea_field("notes", "", 2)))
    }
}

fn immunization_fields() -> Markup {
    html! {
        (c::field("Vaccine", c::input_text("vaccine", "", true, Some(120))))
        (c::field_with_hint("CVX code", "Optional standard vaccine code.", c::input_text("code", "", false, Some(10))))
        (c::field("Date given", c::input_date("occurred_at", "")))
        (c::field("Dose number", c::input_text("dose_number", "", false, Some(3))))
        (c::field("Status", opt_select("status", "completed", IMMUNIZATION_STATUSES, "completed")))
        (c::field("Notes", c::textarea_field("notes", "", 2)))
    }
}

fn observation_fields() -> Markup {
    html! {
        (c::field("Display", c::input_text("display", "", true, Some(120))))
        (c::field("Category", opt_select("category", "vital", OBSERVATION_CATEGORIES, "vital")))
        (c::field_with_hint("LOINC code", "Growth: height 8302-2, weight 29463-7, head-circ 9843-4.", c::input_text("code", "", false, Some(16))))
        (c::field("Value (number)", c::input_text("value_num", "", false, Some(20))))
        (c::field("Unit", c::input_text("unit", "", false, Some(20))))
        (c::field("Effective date", c::input_date("effective_on", "")))
        (c::field("Notes", c::textarea_field("notes", "", 2)))
    }
}
