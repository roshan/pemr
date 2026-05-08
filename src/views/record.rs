use maud::{Markup, html};
use uuid::Uuid;

use crate::models::{Incident, RECORD_KINDS, Record, Source, Subject, record_kind_label};
use crate::views::components as c;
use crate::views::layout::{Nav, render_date, shell, subject_badge};

pub fn list_page(
    nav: &Nav<'_>,
    records: &[Record],
    subjects: &[Subject],
    kind_filter: Option<&str>,
) -> Markup {
    let body = html! {
        div class="flex items-center justify-between mb-4" {
            (c::page_title("Records"))
            div class="flex gap-2" {
                (c::button_link_secondary(import_url(nav.current_subject), "Import DICOM"))
                (c::button_link_primary(new_url(nav.current_subject), "New record"))
            }
        }
        div class="mb-4 flex flex-wrap items-center gap-1.5 text-xs" {
            (kind_chip(nav, None, kind_filter, "All"))
            @for k in RECORD_KINDS {
                (kind_chip(nav, Some(*k), kind_filter, record_kind_label(k)))
            }
        }
        @if records.is_empty() {
            (c::empty_state("Nothing yet."))
        } @else {
            (c::data_table(
                html! { tr {
                    (c::th("When")) (c::th("Kind")) (c::th("Subject")) (c::th("Title"))
                }},
                html! {
                    @for rec in records {
                        tr class="hover:bg-slate-50" {
                            (c::td(html! { (render_date(rec.occurred_at, &rec.occurred_precision)) }))
                            (c::td(c::badge_kind(record_kind_label(&rec.kind))))
                            (c::td(subject_badge(subjects, rec.subject_id)))
                            (c::td(html! { a href={ "/records/" (rec.id) } { (rec.title) } }))
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
    sources: &[Source],
    selected_subject: Option<Uuid>,
    link_incident: Option<Uuid>,
    error: Option<&str>,
) -> Markup {
    let body = html! {
        (c::page_title("New record"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::form_multipart("/records", html! {
            @if let Some(id) = link_incident {
                input type="hidden" name="link_incident" value=(id);
                (c::alert_info(html! { "Will be linked to incident " (c::code(id.to_string())) "." }))
            }
            (subject_select(subjects, selected_subject))
            (c::field("Kind", c::select_field("kind", true, || html! {
                @for k in RECORD_KINDS { (c::select_option(k, record_kind_label(k), false)) }
            })))
            (c::field("Title", c::input_text("title", "", true, Some(200))))
            (c::field("When (date)", c::input_date("occurred_at", "")))
            (source_select(sources, None))
            (c::field("External ID", c::input_text("external_id", "", false, None)))
            (c::field("External URL", c::input_url("external_url", "")))
            (c::field_with_hint(
                "File (optional)",
                "X-rays, lab PDFs, photos. Up to 256 MB.",
                c::input_file("file"),
            ))
            (c::field("Notes (markdown OK)", c::textarea_field("notes", "", 6)))
            (c::button_primary("Create record"))
        }))
    };
    shell(nav, body)
}

pub fn edit_form(
    nav: &Nav<'_>,
    record: &Record,
    subjects: &[Subject],
    sources: &[Source],
    error: Option<&str>,
) -> Markup {
    let body = html! {
        (c::page_title("Edit record"))
        @if let Some(e) = error { (c::alert_danger(e)) }
        (c::form(format!("/records/{}/edit", record.id), "post", html! {
            (subject_select(subjects, Some(record.subject_id)))
            (c::field("Kind", c::select_field("kind", true, || html! {
                @for k in RECORD_KINDS {
                    (c::select_option(k, record_kind_label(k), *k == record.kind))
                }
            })))
            (c::field("Title", c::input_text("title", &record.title, true, Some(200))))
            (c::field("When", c::input_date(
                "occurred_at",
                &record.occurred_at.map(|d| d.to_string()).unwrap_or_default(),
            )))
            (source_select(sources, record.source_id))
            (c::field("External ID", c::input_text(
                "external_id",
                &record.external_id.clone().unwrap_or_default(),
                false, None,
            )))
            (c::field("External URL", c::input_url(
                "external_url",
                &record.external_url.clone().unwrap_or_default(),
            )))
            (c::field("Notes", c::textarea_field("notes", &record.notes, 8)))
            div class="flex gap-2" {
                (c::button_primary("Save"))
                (c::button_link_secondary(format!("/records/{}", record.id), "Cancel"))
            }
        }))
    };
    shell(nav, body)
}

pub fn import_form(
    nav: &Nav<'_>,
    _subjects: &[Subject],
    link_incident: Option<Uuid>,
    error: Option<&str>,
) -> Markup {
    let body = html! {
        (c::page_title("Import DICOM"))
        @if let Some(e) = error { (c::alert_danger(e)) }

        (c::alert_info(html! {
            "We figure out the rest from the DICOM headers — subject from "
            (c::code("PatientName")) ", source from " (c::code("InstitutionName"))
            ". Anything that isn't DICOM is skipped silently. You can edit any field on the record afterwards."
        }))

        (c::form_multipart("/records/import", html! {
            @if let Some(inc_id) = link_incident {
                input type="hidden" name="link_incident" value=(inc_id);
                (c::alert_info(html! {
                    "All imported records will be linked to incident " (c::code(inc_id.to_string())) "."
                }))
            }

            (folder_picker())
            (zip_picker())

            (c::button_primary("Import"))
        }))
    };
    shell(nav, body)
}

/// Big visual drop-target for picking a folder. The label wraps the hidden
/// `<input>` so the whole area is clickable; clicking opens the OS folder
/// selector via `webkitdirectory`.
fn folder_picker() -> Markup {
    html! {
        label class="block cursor-pointer rounded-lg border-2 border-dashed border-line bg-slate-50 hover:bg-slate-100 px-6 py-10 text-center" {
            div class="text-sm font-medium text-ink mb-1" { "Click to pick a folder of DICOM files" }
            div class="text-xs text-muted" {
                "Your browser will open a folder selector. The Sutter / Lexmark folder works as-is — non-DICOM files (viewer chrome, EULAs, etc.) are skipped."
            }
            input
                type="file"
                name="files"
                multiple
                webkitdirectory
                directory
                class="sr-only";
        }
    }
}

/// Fallback: if the folder selector misbehaves in the browser, the user can
/// zip the folder first and upload that.
fn zip_picker() -> Markup {
    html! {
        label class="block text-xs text-muted" {
            div class="mb-1" { "Or upload a " (c::code(".zip")) " of the folder:" }
            input
                type="file"
                name="files"
                accept=".zip,application/zip"
                multiple
                class="block w-full text-sm \
                       file:mr-3 file:rounded-md file:border-0 file:bg-brand-soft file:text-brand-ink \
                       file:px-3 file:py-1.5 file:text-sm file:font-medium hover:file:bg-indigo-200";
        }
    }
}

pub fn detail_page(
    nav: &Nav<'_>,
    record: &Record,
    subjects: &[Subject],
    source: Option<&Source>,
    linked_incidents: &[Incident],
) -> Markup {
    let body = html! {
        (c::card(html! {
            div class="flex items-start justify-between gap-4 mb-2" {
                h1 class="text-2xl font-semibold tracking-tight text-ink" { (record.title) }
                (c::button_link_secondary(format!("/records/{}/edit", record.id), "Edit"))
            }
            (c::meta_row(html! {
                (subject_badge(subjects, record.subject_id))
                (c::badge_kind(record_kind_label(&record.kind)))
                (c::badge_neutral(render_date(record.occurred_at, &record.occurred_precision)))
                @if let Some(src) = source {
                    (c::badge_source(html! { "from " (src.name) }))
                    (c::external_link(record.external_url.as_deref()))
                }
            }))

            @if record.file_path.is_some() {
                div class="mt-4 rounded-lg overflow-hidden border border-line bg-slate-50" {
                    (file_viewer(record))
                }
                p class="mt-2 text-xs text-muted flex flex-wrap items-center gap-2" {
                    a href={ "/records/" (record.id) "/file" } download class="text-brand hover:underline" {
                        "Download"
                        @if let Some(b) = record.byte_size {
                            " (" (human_size(b)) ")"
                        }
                    }
                    @if let Some(sha) = &record.sha256 {
                        span { "·" }
                        span class="text-muted" { "sha256 " (c::code(format!("{}…", &sha[0..16]))) }
                    }
                }
            }
            @if !record.notes.is_empty() {
                (c::subheading("Notes"))
                (c::prose(&record.notes))
            }
            (dicom_metadata_panel(record))
        }))

        (c::lane(
            html! { (c::section_heading("Linked incidents")) },
            html! {
                @if linked_incidents.is_empty() {
                    (c::empty_state("Not linked to any incident."))
                } @else {
                    ul class="space-y-1.5" {
                        @for inc in linked_incidents {
                            li class="flex items-center gap-2 text-sm" {
                                a href={ "/incidents/" (inc.id) } class="font-medium" { (inc.title) }
                                (subject_badge(subjects, inc.subject_id))
                                span class="text-xs text-muted" {
                                    (render_date(inc.occurred_at, &inc.occurred_precision))
                                }
                            }
                        }
                    }
                }
            },
        ))
    };
    shell(nav, body)
}

fn file_viewer(rec: &Record) -> Markup {
    // Preview takes precedence: a record may have a non-browser-renderable
    // primary file (DICOM) but a server-rendered PNG preview alongside it.
    if let (Some(_), Some(pct)) = (&rec.preview_path, &rec.preview_content_type) {
        let preview_url = format!("/records/{}/preview", rec.id);
        return html! {
            @if pct.starts_with("image/") {
                img src=(preview_url) alt=(rec.title) class="viewer-img";
            } @else {
                iframe src=(preview_url) title=(rec.title) class="viewer-frame" {}
            }
        };
    }
    let url = format!("/records/{}/file", rec.id);
    let ct = rec.content_type.as_deref().unwrap_or("");
    html! {
        @if ct.starts_with("image/") {
            img src=(url) alt=(rec.title) class="viewer-img";
        } @else if ct == "application/pdf" {
            iframe src=(url) title=(rec.title) class="viewer-frame" {}
        } @else {
            p class="p-4 text-sm text-muted italic" {
                "No inline preview for " (c::code(ct)) "; use the download link below."
            }
        }
    }
}

fn subject_select(subjects: &[Subject], selected: Option<Uuid>) -> Markup {
    c::field("Subject", c::select_field("subject_id", true, || html! {
        @for s in subjects {
            (c::select_option(s.id, &s.full_name, Some(s.id) == selected))
        }
    }))
}
fn source_select(sources: &[Source], selected: Option<Uuid>) -> Markup {
    c::field("Source", c::select_field("source_id", false, || html! {
        (c::select_option("", "— none —", false))
        @for sr in sources {
            (c::select_option(sr.id, &sr.name, Some(sr.id) == selected))
        }
    }))
}

fn kind_chip(nav: &Nav<'_>, k: Option<&str>, current: Option<&str>, label: &str) -> Markup {
    let mut href = "/records?".to_string();
    if let Some(s) = nav.current_subject {
        href.push_str(&format!("subject={s}&"));
    }
    if let Some(k) = k { href.push_str(&format!("kind={k}")); }
    let active = current == k;
    let cls = if active {
        "px-2.5 py-1 rounded-full bg-brand text-white"
    } else {
        "px-2.5 py-1 rounded-full bg-slate-100 text-ink hover:bg-slate-200 hover:no-underline"
    };
    html! { a href=(href) class=(cls) { (label) } }
}

fn new_url(subject: Option<Uuid>) -> String {
    match subject {
        Some(id) => format!("/records/new?subject={id}"),
        None => "/records/new".to_string(),
    }
}

fn import_url(subject: Option<Uuid>) -> String {
    match subject {
        Some(id) => format!("/records/import?subject={id}"),
        None => "/records/import".to_string(),
    }
}

/// Structured key/value panel built from `records.dicom_metadata` jsonb.
/// Renders nothing for non-DICOM records.
fn dicom_metadata_panel(rec: &Record) -> Markup {
    let Some(meta) = rec.dicom_metadata.as_ref() else {
        return html! {};
    };
    if !meta.is_object() {
        return html! {};
    }
    // Canonical display order; longer-form fields go last.
    let rows: Vec<(&str, &str)> = vec![
        ("Modality", "modality"),
        ("Body part", "body_part"),
        ("Laterality", "laterality"),
        ("View position", "view_position"),
        ("Study description", "study_description"),
        ("Series description", "series_description"),
        ("Patient (per DICOM)", "patient_name"),
        ("Study date", "study_date"),
        ("Instance #", "instance_number"),
        ("StudyInstanceUID", "study_instance_uid"),
        ("SOPInstanceUID", "sop_instance_uid"),
        ("SOPClassUID", "sop_class_uid"),
    ];
    let mut visible: Vec<(&str, String)> = Vec::new();
    for (label, key) in &rows {
        if let Some(v) = meta.get(*key) {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Null => continue,
                other => other.to_string(),
            };
            if !s.is_empty() {
                visible.push((label, s));
            }
        }
    }
    if visible.is_empty() {
        return html! {};
    }
    html! {
        (c::subheading("DICOM metadata"))
        dl class="dicom-meta-grid text-sm" {
            @for (label, value) in &visible {
                dt class="font-medium text-ink" { (label) }
                dd class="text-muted break-all" {
                    @if label.ends_with("UID") || *label == "Patient (per DICOM)" {
                        (c::code(value))
                    } @else {
                        (value)
                    }
                }
            }
        }
    }
}

fn human_size(b: i64) -> String {
    let b = b as f64;
    if b < 1024.0 { format!("{b:.0} B") }
    else if b < 1024.0 * 1024.0 { format!("{:.1} KB", b / 1024.0) }
    else if b < 1024.0 * 1024.0 * 1024.0 { format!("{:.1} MB", b / (1024.0 * 1024.0)) }
    else { format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0)) }
}
