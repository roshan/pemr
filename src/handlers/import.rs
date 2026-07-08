//! Web upload for structured clinical bundles — currently the Epic **EHI
//! export** (the "Requested Records" zip). Runs the same `importer` core as the
//! offline CLI, but in-process against the live DB, so the user can just drop a
//! zip in the browser. **Preview** (dry-run) by default; **Import** commits.
//! Subject is required and never auto-detected.

use std::path::Path;

use axum::extract::{Multipart, State};
use maud::Markup;
use uuid::Uuid;

use crate::error::AppResult;
use crate::handlers::{AppState, load_subjects};
use crate::importer;
use crate::models::{Subject, empty_to_none};
use crate::viewer::ViewerContext;
use crate::views::import::{self as views, Outcome};
use crate::views::layout::Nav;

pub(crate) const DEFAULT_SOURCE: &str = "MyChart EHI export";

pub async fn page(State(state): State<AppState>, viewer: ViewerContext) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let jobs = crate::sync::all_jobs(&state.pool).await?;
    let nav = nav(&subjects, &viewer);
    Ok(views::page(&nav, &subjects, &jobs, DEFAULT_SOURCE, None, None))
}

pub async fn upload(
    State(state): State<AppState>,
    viewer: ViewerContext,
    mut multipart: Multipart,
) -> AppResult<Markup> {
    let mut subject_sel: Option<String> = None;
    let mut source = String::new();
    let mut action = String::new();
    let mut file: Option<bytes::Bytes> = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name().unwrap_or("") {
            "subject_id" => subject_sel = empty_to_none(field.text().await?),
            "source" => source = field.text().await?,
            "action" => action = field.text().await?,
            "file" => {
                let b = field.bytes().await?;
                if !b.is_empty() {
                    file = Some(b);
                }
            }
            _ => {
                let _ = field.bytes().await?;
            }
        }
    }

    let source_value = if source.trim().is_empty() { DEFAULT_SOURCE } else { source.trim() };
    let subjects = load_subjects(&state.pool).await?;
    let jobs = crate::sync::all_jobs(&state.pool).await?;
    let nav = nav(&subjects, &viewer);
    let render = |o: Outcome| views::page(&nav, &subjects, &jobs, source_value, Some(o), None);

    // Subject is required — never guessed (the EHI export names the patient only
    // by an internal id).
    let Some(subject_id) = subject_sel.as_deref().and_then(|s| s.parse::<Uuid>().ok()) else {
        return Ok(render(Outcome::Error("Pick a subject before importing.".into())));
    };
    let Some(bytes) = file else {
        return Ok(render(Outcome::Error("Choose an EHI export .zip to upload.".into())));
    };
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok(render(Outcome::Error(
            "That file isn't a .zip — upload the EHI export zip (it contains an EHITables/ folder).".into(),
        )));
    }

    // Extract the EHITables/*.tsv into a temp dir under FILES_DIR (writable in the
    // distroless container, unlike /tmp), run the same importer as the CLI, then
    // clean up.
    let tmp = state.files_dir.join(format!("import-tmp-{}", Uuid::now_v7()));
    let outcome = match extract_ehi_tables(&bytes, &tmp) {
        Err(e) => Outcome::Error(format!("Could not read the zip: {e}")),
        Ok(()) => match importer::ehi_tables_dir(&tmp) {
            None => Outcome::Error(
                "Not an Epic EHI export — no EHITables/ folder found in the zip.".into(),
            ),
            Some(_) if action == "commit" => {
                let source_id = importer::ensure_source(&state.pool, source_value).await?;
                match importer::import_ehi(&state.pool, subject_id, source_id, &tmp).await {
                    Ok(counts) => Outcome::Committed(counts),
                    Err(e) => Outcome::Error(format!("Import failed (nothing written): {e}")),
                }
            }
            Some(_) => Outcome::Preview(importer::preview_ehi(&tmp)),
        },
    };
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(render(outcome))
}

fn nav<'a>(subjects: &'a [Subject], viewer: &'a ViewerContext) -> Nav<'a> {
    Nav {
        title: "Import",
        current_path: "/settings/import",
        subjects,
        current_subject: viewer.default_subject_id,
        viewer,
    }
}

/// Extract just the `EHITables/*.tsv` entries from an EHI-export zip into
/// `dir/EHITables/`. We skip the schema HTML, `Media/`, and RTF notes — the
/// parser only reads the TSV tables, so there's no point writing thousands of
/// files we won't open.
fn extract_ehi_tables(bytes: &[u8], dir: &Path) -> std::io::Result<()> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(std::io::Error::other)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(std::io::Error::other)?;
        if !entry.is_file() {
            continue;
        }
        // Epic zips these on Windows, so entry names use backslash separators
        // (e.g. `EHITables\IMMUNE.tsv`). Normalize to forward slashes first.
        let name = entry.name().replace('\\', "/");
        // Accept "EHITables/FOO.tsv" or "<prefix>/EHITables/FOO.tsv"; the table
        // files sit directly under EHITables/ (no nested subdirs).
        let Some(rel) = name
            .split_once("EHITables/")
            .map(|(_, r)| r)
            .filter(|r| r.ends_with(".tsv") && !r.contains('/'))
        else {
            continue;
        };
        let dest = dir.join("EHITables").join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(&dest, buf)?;
    }
    Ok(())
}
