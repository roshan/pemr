use maud::{Markup, html};
use uuid::Uuid;

use crate::models::{
    Incident, Record, Source, Subject, is_image_kind, record_kind_label, source_kind_label,
};
use crate::views::components as c;
use crate::views::layout::{Nav, render_date, render_date_range, shell, subject_badge};

pub fn list_page(nav: &Nav<'_>, incidents: &[Incident], subjects: &[Subject]) -> Markup {
    let body = html! {
        div class="flex items-center justify-between mb-4" {
            (c::page_title("Events"))
            (c::button_link_primary(new_url(nav.current_subject), "New event"))
        }
        @if incidents.is_empty() {
            (c::empty_state("Nothing yet."))
        } @else {
            (c::data_table(
                html! { tr {
                    (c::th("When"))
                    (c::th("Subject"))
                    (c::th("Title"))
                }},
                html! {
                    @for inc in incidents {
                        tr class="hover:bg-slate-50" {
                            (c::td(html! { (render_date_range(inc.occurred_at, &inc.occurred_precision, inc.ended_at, &inc.ended_precision)) }))
                            (c::td(subject_badge(subjects, inc.subject_id)))
                            (c::td(html! { a href={ "/incidents/" (inc.id) } { (inc.title) } }))
                        }
                    }
                },
            ))
        }
    };
    shell(nav, body)
}

pub fn new_form(
    nav: &Nav<'_>,
    subjects: &[Subject],
    selected_subject: Option<Uuid>,
    error: Option<&str>,
) -> Markup {
    let body = html! {
        (c::page_title("New event"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::alert_info(
            "An event is a real-world happening (a hospital stay, a fall, an ER visit, a surgery). \
             Give an end date for something that spans days. Records and their EMR sources get \
             attached afterwards."
        ))
        (c::form("/incidents", "post", html! {
            (subject_select(subjects, selected_subject))
            (c::field("Title", c::input_text("title", "", true, Some(200))))
            (c::field("Start date", c::input_date("occurred_at", "")))
            (c::field("End date (optional)", c::input_date("ended_at", "")))
            (c::field("Narrative (markdown OK)", c::textarea_field("narrative", "", 8)))
            (c::button_primary("Create event"))
        }))
    };
    shell(nav, body)
}

pub fn detail_page(
    nav: &Nav<'_>,
    incident: &Incident,
    subjects: &[Subject],
    touching_sources: &[Source],
    linked_records: &[Record],
    candidate_records: &[Record],
) -> Markup {
    let body = html! {
        (c::card(html! {
            div class="flex items-start justify-between gap-4 mb-2" {
                h1 class="text-2xl font-semibold tracking-tight text-ink" { (incident.title) }
                (c::button_link_secondary(format!("/incidents/{}/edit", incident.id), "Edit"))
            }
            (c::meta_row(html! {
                (subject_badge(subjects, incident.subject_id))
                (c::badge_neutral(render_date_range(incident.occurred_at, &incident.occurred_precision, incident.ended_at, &incident.ended_precision)))
            }))
            @if !touching_sources.is_empty() {
                div class="mt-2 flex flex-wrap items-center gap-2 text-xs text-muted" {
                    span { "Records from:" }
                    @for src in touching_sources {
                        a href={ "/sources/" (src.id) } class="hover:no-underline" {
                            (c::badge_source(html! {
                                (src.name) " (" (source_kind_label(&src.kind)) ")"
                            }))
                        }
                    }
                }
            }
            @if !incident.narrative.is_empty() {
                div class="mt-3" { (c::prose(&incident.narrative)) }
            }
        }))

        (c::lane(
            html! { (c::section_heading("Linked records")) },
            html! {
                @if linked_records.is_empty() {
                    (c::empty_state("None linked yet."))
                } @else {
                    (linked_records_groups(incident, subjects, linked_records))
                }

                (add_records_actions(incident))

                div class="mt-3" {
                    (c::collapse_section("Or link an existing record", html! {
                        (c::form(format!("/incidents/{}/link", incident.id), "post", html! {
                            (c::field("Record", c::select_field("record_id", true, || html! {
                                (c::select_option("", "— pick one —", false))
                                @for rec in candidate_records {
                                    (c::select_option(rec.id, html! {
                                        (record_kind_label(&rec.kind)) " — " (rec.title)
                                        @if let Some(d) = rec.occurred_at { " (" (d) ")" }
                                    }, false))
                                }
                            })))
                            (c::field("Note", c::input_text("note", "", false, Some(200))))
                            (c::button_primary("Link"))
                        }))
                    }, false))
                }
            },
        ))
    };
    shell(nav, body)
}

/// Group linked records by `study_instance_uid` (one block per imaging
/// study); records without a study UID become a single "Other" group at the
/// end. Within each group, image-kind records render as a thumbnail strip
/// and reports/notes/etc. are listed compactly underneath.
fn linked_records_groups(
    incident: &Incident,
    subjects: &[Subject],
    records: &[Record],
) -> Markup {
    let mut groups: Vec<(Option<&str>, Vec<&Record>)> = Vec::new();
    for rec in records {
        let key = rec.study_instance_uid.as_deref();
        if let Some(g) = groups.iter_mut().find(|(k, _)| *k == key) {
            g.1.push(rec);
        } else {
            groups.push((key, vec![rec]));
        }
    }
    html! {
        div class="space-y-4" {
            @for (uid, recs) in &groups {
                (study_block(incident, subjects, *uid, recs))
            }
        }
    }
}

fn study_block(
    incident: &Incident,
    subjects: &[Subject],
    study_uid: Option<&str>,
    records: &[&Record],
) -> Markup {
    // The "anchor" record drives the block heading: prefer the first
    // non-report image-kind record, then any first record.
    let anchor = records
        .iter()
        .find(|r| is_image_kind(&r.kind))
        .or_else(|| records.first())
        .copied();
    let heading_text = anchor
        .and_then(|r| {
            r.dicom_metadata
                .as_ref()
                .and_then(|m| m.get("study_description").and_then(|v| v.as_str()))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
        .or_else(|| anchor.map(|r| r.title.split(" — ").next().unwrap_or(&r.title).to_string()))
        .unwrap_or_else(|| "Other linked records".to_string());
    let when = anchor
        .and_then(|r| r.occurred_at.map(|d| (d, r.occurred_precision.clone())))
        .map(|(d, p)| render_date(Some(d), &p));

    let images: Vec<&Record> = records
        .iter()
        .copied()
        .filter(|r| is_image_kind(&r.kind))
        .collect();
    let others: Vec<&Record> = records
        .iter()
        .copied()
        .filter(|r| !is_image_kind(&r.kind))
        .collect();

    html! {
        article class="rounded-lg border border-line bg-surface p-4" {
            header class="mb-3 flex flex-wrap items-center gap-2" {
                h3 class="text-base font-semibold tracking-tight text-ink" { (heading_text) }
                @if let Some(w) = &when { (c::badge_neutral(w)) }
                @if study_uid.is_none() { (c::badge_neutral("no study")) }
                @if let Some(a) = anchor { (subject_badge(subjects, a.subject_id)) }
            }

            @if !images.is_empty() {
                div class="tile-grid" {
                    @for rec in &images {
                        (image_tile(incident, rec))
                    }
                }
            }

            @if !others.is_empty() {
                ul class="mt-3 space-y-1.5" {
                    @for rec in &others {
                        li class="flex items-start gap-2 text-sm" {
                            (c::badge_kind(record_kind_label(&rec.kind)))
                            a href={ "/records/" (rec.id) } class="font-medium" { (rec.title) }
                            span class="ml-auto" {
                                (c::button_subtle_danger("Unlink", c::HtmxDelete {
                                    url: format!("/incidents/{}/records/{}", incident.id, rec.id),
                                    target: "closest li",
                                    swap: "outerHTML",
                                    confirm: None,
                                }))
                            }
                        }
                    }
                }
            }
        }
    }
}

fn image_tile(incident: &Incident, rec: &Record) -> Markup {
    let thumb_url = if rec.thumbnail_path.is_some() {
        Some(format!("/records/{}/thumbnail", rec.id))
    } else if rec.preview_path.is_some() {
        Some(format!("/records/{}/preview", rec.id))
    } else if rec
        .content_type
        .as_deref()
        .map(|c| c.starts_with("image/"))
        .unwrap_or(false)
    {
        Some(format!("/records/{}/file", rec.id))
    } else {
        None
    };
    let view_position = view_position_label(rec);
    html! {
        figure class="group relative rounded-md overflow-hidden border border-line bg-slate-50" {
            a href={ "/records/" (rec.id) } class="block hover:no-underline" {
                @match thumb_url {
                    Some(url) => img src=(url) alt=(rec.title) loading="lazy"
                                     class="aspect-square w-full object-contain bg-slate-900";,
                    None => div class="aspect-square w-full flex items-center justify-center text-xs text-muted" {
                        "No preview"
                    },
                }
                figcaption class="px-2 py-1.5 text-xs text-ink bg-surface" {
                    @if let Some(v) = view_position {
                        span class="font-medium" { (v) }
                    } @else {
                        span class="font-medium" { (rec.title) }
                    }
                }
            }
            button
                type="button"
                title="Unlink from this event"
                hx-delete=(format!("/incidents/{}/records/{}", incident.id, rec.id))
                hx-target="closest figure"
                hx-swap="outerHTML"
                class="absolute top-1 right-1 hidden group-hover:block rounded bg-rose-600/90 text-white text-xs px-1.5 py-0.5"
            { "×" }
        }
    }
}

/// Find e.g. "AP" / "Lateral" suffix in the title (after " — ") if present.
fn view_position_label(rec: &Record) -> Option<String> {
    let parts: Vec<&str> = rec.title.splitn(2, " — ").collect();
    if parts.len() == 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

pub fn edit_form(
    nav: &Nav<'_>,
    incident: &Incident,
    subjects: &[Subject],
    error: Option<&str>,
) -> Markup {
    let body = html! {
        (c::page_title("Edit event"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::form(format!("/incidents/{}/edit", incident.id), "post", html! {
            (subject_select(subjects, Some(incident.subject_id)))
            (c::field("Title", c::input_text("title", &incident.title, true, Some(200))))
            (c::field("Start date", c::input_date(
                "occurred_at",
                &incident.occurred_at.map(|d| d.to_string()).unwrap_or_default(),
            )))
            (c::field("End date (optional)", c::input_date(
                "ended_at",
                &incident.ended_at.map(|d| d.to_string()).unwrap_or_default(),
            )))
            (c::field("Narrative", c::textarea_field("narrative", &incident.narrative, 10)))
            div class="flex gap-2" {
                (c::button_primary("Save"))
                (c::button_link_secondary(format!("/incidents/{}", incident.id), "Cancel"))
            }
        }))
    };
    shell(nav, body)
}

fn subject_select(subjects: &[Subject], selected: Option<Uuid>) -> Markup {
    c::field("Subject", c::select_field("subject_id", true, || html! {
        @for s in subjects {
            (c::select_option(s.id, &s.full_name, Some(s.id) == selected))
        }
    }))
}

fn new_url(subject: Option<Uuid>) -> String {
    match subject {
        Some(id) => format!("/incidents/new?subject={id}"),
        None => "/incidents/new".to_string(),
    }
}
fn new_record_url_for_incident(inc: &Incident) -> String {
    format!("/records/new?subject={}&link_incident={}", inc.subject_id, inc.id)
}
fn import_dicom_url_for_incident(inc: &Incident) -> String {
    format!("/records/import?subject={}&link_incident={}", inc.subject_id, inc.id)
}

/// Two side-by-side clickable cards for adding records to this incident.
/// DICOM-import is on the left because it's by far the most common case.
fn add_records_actions(incident: &Incident) -> Markup {
    let import_url = import_dicom_url_for_incident(incident);
    let new_url = new_record_url_for_incident(incident);
    html! {
        div class="mt-4 grid gap-3 grid-cols-1 sm:grid-cols-2" {
            (action_card(
                &import_url,
                "Import DICOM",
                html! {
                    "Drop a Sutter / Lexmark folder or " (c::code(".zip"))
                    ". Subject, source, kinds, thumbnails are all auto-detected from the file headers."
                },
            ))
            (action_card(
                &new_url,
                "Add a single record",
                html! {
                    "Upload one file (PDF, photo, lab result, …) and fill in the details by hand."
                },
            ))
        }
    }
}

fn action_card(href: &str, title: &str, body: Markup) -> Markup {
    html! {
        a href=(href)
          class="group block rounded-lg border-2 border-line bg-surface p-4 \
                 hover:border-brand hover:bg-slate-50 hover:no-underline transition-colors" {
            div class="flex items-start justify-between gap-2" {
                h4 class="text-base font-semibold text-ink" { (title) }
                span class="text-muted text-base font-bold group-hover:text-brand transition-colors" { "→" }
            }
            p class="mt-1 text-xs text-muted" { (body) }
        }
    }
}
