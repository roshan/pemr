use axum::body::Body;
use axum::extract::{Form, Multipart, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use sqlx::PgPool;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::dicom_import;
use crate::error::{AppError, AppResult};
use crate::files::{self, StoredFile};
use crate::handlers::{AppState, load_subjects};
use crate::images;
use crate::models::{
    Incident, RECORD_KINDS, Record, Source, Subject, empty_to_none, parse_date,
    parse_subject_filter,
};
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::views::record;

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub subject: Option<String>,
    pub kind: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<ListQuery>,
) -> AppResult<Markup> {
    let subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    list_render(&state, viewer, subject, q.kind.as_deref()).await
}

pub async fn list_for_subject(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(subject_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> AppResult<Markup> {
    list_render(&state, viewer, Some(subject_id), q.kind.as_deref()).await
}

async fn list_render(
    state: &AppState,
    viewer: ViewerContext,
    subject: Option<Uuid>,
    kind_filter: Option<&str>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let kind = kind_filter
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let records = sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records
          where ($1::uuid is null or subject_id = $1)
            and ($2::text is null or kind = $2)
          order by occurred_at desc nulls last, created_at desc
          limit 300",
    )
    .bind(subject)
    .bind(kind.as_deref())
    .fetch_all(&state.pool)
    .await?;

    let nav = Nav {
        title: "Records",
        current_path: "/records",
        subjects: &subjects,
        current_subject: subject,
        viewer: &viewer,
    };
    Ok(record::list_page(&nav, &records, &subjects, kind.as_deref()))
}

#[derive(Debug, Deserialize, Default)]
pub struct NewQuery {
    pub subject: Option<String>,
    pub link_incident: Option<String>,
}

pub async fn new(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<NewQuery>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let sources = all_sources(&state.pool).await?;
    let url_subject =
        parse_subject_filter(q.subject.as_deref()).map_err(AppError::BadRequest)?;
    let pre = url_subject
        .or(viewer.default_subject_id)
        .or_else(|| subjects.first().map(|s| s.id));
    let link = match q.link_incident.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => {
            Some(Uuid::parse_str(s).map_err(|e| AppError::BadRequest(e.to_string()))?)
        }
    };
    let nav = Nav {
        title: "New record",
        current_path: "/records",
        subjects: &subjects,
        current_subject: pre,
        viewer: &viewer,
    };
    Ok(record::new_form(&nav, &subjects, &sources, pre, link, None))
}

const MAX_UPLOAD_BYTES: usize = 256 * 1024 * 1024;

pub async fn create(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let mut subject_id: Option<Uuid> = None;
    let mut kind: Option<String> = None;
    let mut title: Option<String> = None;
    let mut notes: String = String::new();
    let mut occurred_at: Option<String> = None;
    let mut source_id: Option<Uuid> = None;
    let mut external_id: Option<String> = None;
    let mut external_url: Option<String> = None;
    let mut link_incident: Option<Uuid> = None;
    let mut file_bytes: Option<bytes::Bytes> = None;
    let mut file_filename: Option<String> = None;
    let mut file_content_type: Option<String> = None;

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "subject_id" => {
                let s = field.text().await?;
                subject_id = Some(
                    Uuid::parse_str(s.trim())
                        .map_err(|e| AppError::BadRequest(e.to_string()))?,
                );
            }
            "kind" => kind = Some(field.text().await?),
            "title" => title = Some(field.text().await?),
            "notes" => notes = field.text().await?,
            "occurred_at" => occurred_at = Some(field.text().await?),
            "source_id" => {
                let s = field.text().await?;
                source_id = match empty_to_none(s) {
                    None => None,
                    Some(s) => Some(
                        Uuid::parse_str(&s).map_err(|e| AppError::BadRequest(e.to_string()))?,
                    ),
                };
            }
            "external_id" => external_id = empty_to_none(field.text().await?),
            "external_url" => external_url = empty_to_none(field.text().await?),
            "link_incident" => {
                let s = field.text().await?;
                link_incident = match empty_to_none(s) {
                    None => None,
                    Some(s) => Some(
                        Uuid::parse_str(&s).map_err(|e| AppError::BadRequest(e.to_string()))?,
                    ),
                };
            }
            "file" => {
                file_filename = field.file_name().map(|s| s.to_string());
                file_content_type = field.content_type().map(|s| s.to_string());
                let bytes = field.bytes().await?;
                if bytes.len() > MAX_UPLOAD_BYTES {
                    return Err(AppError::BadRequest(format!(
                        "upload too large: {} bytes (max {})",
                        bytes.len(),
                        MAX_UPLOAD_BYTES
                    )));
                }
                if !bytes.is_empty() {
                    file_bytes = Some(bytes);
                }
            }
            _ => {
                // ignore unknown fields
                let _ = field.bytes().await?;
            }
        }
    }

    let subject_id = subject_id.ok_or_else(|| AppError::BadRequest("subject_id required".into()))?;
    let kind = kind
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("kind required".into()))?;
    if !RECORD_KINDS.iter().any(|k| *k == kind) {
        return Err(AppError::BadRequest(format!("unknown kind: {kind}")));
    }
    let title = title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("title required".into()))?;
    let occurred_at = parse_date(occurred_at.as_deref().unwrap_or(""))
        .map_err(AppError::BadRequest)?;

    let stored: Option<StoredFile> = match &file_bytes {
        None => None,
        Some(bytes) => {
            let ext = file_filename
                .as_deref()
                .and_then(files::extension_from_filename)
                .or_else(|| {
                    file_content_type
                        .as_deref()
                        .and_then(files::extension_from_content_type)
                });
            Some(files::store_bytes(&state.files_dir, bytes, ext).await?)
        }
    };

    // Generate a thumbnail if the upload is a known image format.
    let stored_thumb: Option<StoredFile> = match (&file_bytes, file_content_type.as_deref()) {
        (Some(bytes), Some(ct)) if images::can_thumbnail(ct) => {
            match images::thumbnail_webp(bytes, 400) {
                Ok(webp) => Some(files::store_bytes(&state.files_dir, &webp, Some("webp")).await?),
                Err(e) => {
                    tracing::warn!(error = %e, "thumbnail generation failed; continuing without one");
                    None
                }
            }
        }
        _ => None,
    };

    let id = Uuid::now_v7();

    sqlx::query(
        "insert into records
            (id, subject_id, kind, title, notes, occurred_at,
             file_path, content_type, byte_size, sha256,
             thumbnail_path, thumbnail_content_type,
             source_id, external_id, external_url)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(id)
    .bind(subject_id)
    .bind(&kind)
    .bind(&title)
    .bind(&notes)
    .bind(occurred_at)
    .bind(stored.as_ref().map(|s| s.relative_path.as_str()))
    .bind(file_content_type.as_deref())
    .bind(stored.as_ref().map(|s| s.byte_size))
    .bind(stored.as_ref().map(|s| s.sha256_hex.as_str()))
    .bind(stored_thumb.as_ref().map(|s| s.relative_path.as_str()))
    .bind(stored_thumb.as_ref().map(|_| "image/webp"))
    .bind(source_id)
    .bind(external_id.as_deref())
    .bind(external_url.as_deref())
    .execute(&state.pool)
    .await?;

    if let Some(inc_id) = link_incident {
        sqlx::query(
            "insert into incident_records (incident_id, record_id) values ($1,$2)
             on conflict do nothing",
        )
        .bind(inc_id)
        .bind(id)
        .execute(&state.pool)
        .await?;
        return Ok(Redirect::to(&format!("/incidents/{inc_id}")).into_response());
    }

    Ok(Redirect::to(&format!("/records/{id}")).into_response())
}

// ---------------------------------------------------------------------------
// DICOM import (Sutter / Lexmark exports + similar)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct ImportQuery {
    pub subject: Option<String>,
    pub link_incident: Option<String>,
}

pub async fn import_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Query(q): Query<ImportQuery>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let pre = parse_subject_filter(q.subject.as_deref())
        .map_err(AppError::BadRequest)?
        .or(viewer.default_subject_id)
        .or_else(|| subjects.first().map(|s| s.id));
    let link_incident = match q.link_incident.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(s) => Some(Uuid::parse_str(s).map_err(|e| AppError::BadRequest(e.to_string()))?),
    };
    let nav = Nav {
        title: "Import DICOM",
        current_path: "/records",
        subjects: &subjects,
        current_subject: pre,
        viewer: &viewer,
    };
    Ok(record::import_form(&nav, &subjects, link_incident, None))
}

pub struct ImportResult {
    pub created: usize,
    pub skipped: usize,
    pub patient_name_mismatches: usize,
    pub subjects_touched: std::collections::BTreeSet<Uuid>,
}

pub async fn import(
    State(state): State<AppState>,
    viewer: ViewerContext,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let mut link_incident: Option<Uuid> = None;
    let mut files_in: Vec<(Option<String>, bytes::Bytes)> = Vec::new();

    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "link_incident" => {
                let s = field.text().await?;
                link_incident = match empty_to_none(s) {
                    None => None,
                    Some(s) => Some(
                        Uuid::parse_str(&s).map_err(|e| AppError::BadRequest(e.to_string()))?,
                    ),
                };
            }
            "files" => {
                let filename = field.file_name().map(|s| s.to_string());
                let bytes = field.bytes().await?;
                if !bytes.is_empty() {
                    files_in.push((filename, bytes));
                }
            }
            _ => {
                let _ = field.bytes().await?;
            }
        }
    }

    let subjects = load_subjects(&state.pool).await?;
    if subjects.is_empty() {
        return Err(AppError::BadRequest(
            "no subjects in the database; create one first".into(),
        ));
    }
    let fallback_subject_id = viewer.default_subject_id.unwrap_or(subjects[0].id);

    // Cache of source name → source_id, populated lazily as we encounter
    // InstitutionName values across the imported files.
    let mut source_cache: std::collections::HashMap<String, Uuid> = std::collections::HashMap::new();

    let mut result = ImportResult {
        created: 0,
        skipped: 0,
        patient_name_mismatches: 0,
        subjects_touched: std::collections::BTreeSet::new(),
    };

    // Flatten zips into individual files. We don't recurse zips-in-zips,
    // because that's never been seen in the wild for medical imaging exports.
    let mut flat: Vec<(Option<String>, bytes::Bytes)> = Vec::new();
    for (filename, bytes) in files_in {
        if is_zip(&bytes) {
            match expand_zip(&bytes) {
                Ok(entries) => flat.extend(entries),
                Err(e) => {
                    tracing::warn!(?filename, error = %e, "zip extraction failed; skipping");
                    result.skipped += 1;
                }
            }
        } else {
            flat.push((filename, bytes));
        }
    }

    for (filename, bytes) in flat {
        if !dicom_import::is_dicom(&bytes) {
            result.skipped += 1;
            continue;
        }
        let obj = match dicom_import::parse(&bytes) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(?filename, error = %e, "dicom parse failed; skipping");
                result.skipped += 1;
                continue;
            }
        };
        let meta = dicom_import::extract_metadata(&obj);
        let rendered = match dicom_import::render_images(&obj) {
            Ok(r) => r,
            Err(e) => {
                tracing::info!(?filename, error = %e, "skipping DICOM with no decodable pixel data");
                result.skipped += 1;
                continue;
            }
        };

        let stored_dcm = files::store_bytes(&state.files_dir, &bytes, Some("dcm")).await?;
        let stored_png = files::store_bytes(&state.files_dir, &rendered.png, Some("png")).await?;
        let stored_thumb =
            files::store_bytes(&state.files_dir, &rendered.thumbnail_webp, Some("webp")).await?;

        // Auto-detect subject from PatientName. Falls back to the viewer's
        // default subject (or the first row in the table) when the DICOM
        // doesn't match anyone we know about.
        let (resolved_subject_id, name_matched) =
            resolve_subject(&subjects, meta.patient_name.as_deref(), fallback_subject_id);
        if !name_matched && meta.patient_name.is_some() {
            tracing::warn!(
                ?filename,
                dicom_patient = ?meta.patient_name,
                "no subject matched PatientName; using fallback"
            );
            result.patient_name_mismatches += 1;
        }

        // Auto-detect/create source from InstitutionName.
        let resolved_source_id = if let Some(institution) = meta.institution_name.as_deref() {
            Some(find_or_create_source(&state.pool, &mut source_cache, institution).await?)
        } else {
            None
        };

        let id = Uuid::now_v7();
        let title = dicom_import::derive_title(&meta);
        let kind = dicom_import::derive_kind(&meta);
        let metadata_json = serde_json::to_value(&meta).unwrap_or(serde_json::Value::Null);

        sqlx::query(
            "insert into records
                (id, subject_id, kind, title, notes, occurred_at,
                 file_path, content_type, byte_size, sha256,
                 preview_path, preview_content_type,
                 thumbnail_path, thumbnail_content_type,
                 study_instance_uid,
                 dicom_metadata, instance_number,
                 source_id, external_id)
             values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)
             on conflict (source_id, external_id)
                where source_id is not null and external_id is not null
                do nothing",
        )
        .bind(id)
        .bind(resolved_subject_id)
        .bind(kind)
        .bind(&title)
        .bind("")
        .bind(meta.study_date)
        .bind(&stored_dcm.relative_path)
        .bind("application/dicom")
        .bind(stored_dcm.byte_size)
        .bind(&stored_dcm.sha256_hex)
        .bind(&stored_png.relative_path)
        .bind("image/png")
        .bind(&stored_thumb.relative_path)
        .bind("image/webp")
        .bind(meta.study_instance_uid.as_deref())
        .bind(&metadata_json)
        .bind(meta.instance_number)
        .bind(resolved_source_id)
        .bind(meta.sop_instance_uid.as_deref())
        .execute(&state.pool)
        .await?;

        if let Some(inc_id) = link_incident {
            sqlx::query(
                "insert into incident_records (incident_id, record_id) values ($1,$2)
                 on conflict do nothing",
            )
            .bind(inc_id)
            .bind(id)
            .execute(&state.pool)
            .await?;
        }
        result.created += 1;
        result.subjects_touched.insert(resolved_subject_id);
    }

    tracing::info!(
        created = result.created,
        skipped = result.skipped,
        patient_name_mismatches = result.patient_name_mismatches,
        subjects_touched = result.subjects_touched.len(),
        "DICOM import done"
    );

    let qs = format!(
        "import=created:{}+skipped:{}+patient_name_mismatches:{}",
        result.created, result.skipped, result.patient_name_mismatches
    );

    // If we know exactly which subject everything landed on, take the user
    // to that subject's records list. Otherwise show them everything.
    let dest = if let Some(inc_id) = link_incident {
        format!("/incidents/{inc_id}?{qs}")
    } else if result.subjects_touched.len() == 1 {
        let s = result.subjects_touched.iter().next().unwrap();
        format!("/subjects/{s}/records?{qs}")
    } else {
        format!("/records?{qs}")
    };
    Ok(Redirect::to(&dest).into_response())
}

/// Match `PatientName` against `subjects` (case-insensitive given+family).
/// Returns `(subject_id, matched)` — the second element is `false` when we
/// fell back to `fallback_id` because nothing matched.
fn resolve_subject(
    subjects: &[Subject],
    patient_name: Option<&str>,
    fallback_id: Uuid,
) -> (Uuid, bool) {
    let Some(name) = patient_name else {
        return (fallback_id, false);
    };
    for s in subjects {
        if dicom_import::patient_name_matches(name, &s.given_name, &s.family_name) {
            return (s.id, true);
        }
    }
    (fallback_id, false)
}

async fn find_or_create_source(
    pool: &sqlx::PgPool,
    cache: &mut std::collections::HashMap<String, Uuid>,
    name: &str,
) -> AppResult<Uuid> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("empty institution name".into()));
    }
    if let Some(id) = cache.get(trimmed) {
        return Ok(*id);
    }
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "select id from sources where lower(name) = lower($1) limit 1",
    )
    .bind(trimmed)
    .fetch_optional(pool)
    .await?
    {
        cache.insert(trimmed.to_string(), id);
        return Ok(id);
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into sources (id, name, kind, notes) values ($1,$2,$3,$4)
         on conflict do nothing",
    )
    .bind(id)
    .bind(trimmed)
    .bind("hospital")
    .bind("Auto-created from DICOM InstitutionName at import time.")
    .execute(pool)
    .await?;
    cache.insert(trimmed.to_string(), id);
    Ok(id)
}

/// Detect a zip archive by its 4-byte signature `PK\x03\x04`.
fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x50, 0x4b, 0x03, 0x04])
}

fn expand_zip(bytes: &[u8]) -> std::io::Result<Vec<(Option<String>, bytes::Bytes)>> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(std::io::Error::other)?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(std::io::Error::other)?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_string();
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        out.push((Some(name), bytes::Bytes::from(buf)));
    }
    Ok(out)
}


pub async fn detail(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let rec = fetch_record(&state.pool, id).await?;
    let source = match rec.source_id {
        Some(sid) => sqlx::query_as::<_, Source>("select * from sources where id = $1")
            .bind(sid)
            .fetch_optional(&state.pool)
            .await?,
        None => None,
    };
    let linked = sqlx::query_as::<_, Incident>(
        "select i.id, i.subject_id, i.title, i.narrative, i.occurred_at, i.occurred_precision,
                i.created_at, i.updated_at
           from incidents i
           join incident_records ir on ir.incident_id = i.id
          where ir.record_id = $1
          order by i.occurred_at desc nulls last, i.created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let nav = Nav {
        title: &rec.title,
        current_path: "/records",
        subjects: &subjects,
        current_subject: Some(rec.subject_id),
        viewer: &viewer,
    };
    Ok(record::detail_page(
        &nav,
        &rec,
        &subjects,
        source.as_ref(),
        &linked,
    ))
}

pub async fn edit_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let sources = all_sources(&state.pool).await?;
    let rec = fetch_record(&state.pool, id).await?;
    let nav = Nav {
        title: "Edit record",
        current_path: "/records",
        subjects: &subjects,
        current_subject: Some(rec.subject_id),
        viewer: &viewer,
    };
    Ok(record::edit_form(&nav, &rec, &subjects, &sources, None))
}

#[derive(Debug, Deserialize)]
pub struct EditForm {
    pub subject_id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub occurred_at: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub external_id: String,
    #[serde(default)]
    pub external_url: String,
    #[serde(default)]
    pub notes: String,
}

pub async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<EditForm>,
) -> AppResult<Response> {
    let subject_id =
        Uuid::parse_str(form.subject_id.trim()).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let kind = form.kind.trim().to_string();
    if !RECORD_KINDS.iter().any(|k| *k == kind) {
        return Err(AppError::BadRequest(format!("unknown kind: {kind}")));
    }
    let title = form.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::BadRequest("title required".into()));
    }
    let occurred_at = parse_date(&form.occurred_at).map_err(AppError::BadRequest)?;
    let source_id = match empty_to_none(form.source_id) {
        None => None,
        Some(s) => Some(Uuid::parse_str(&s).map_err(|e| AppError::BadRequest(e.to_string()))?),
    };
    sqlx::query(
        "update records set
            subject_id   = $2,
            kind         = $3,
            title        = $4,
            notes        = $5,
            occurred_at  = $6,
            source_id    = $7,
            external_id  = $8,
            external_url = $9,
            updated_at   = now()
          where id = $1",
    )
    .bind(id)
    .bind(subject_id)
    .bind(&kind)
    .bind(&title)
    .bind(&form.notes)
    .bind(occurred_at)
    .bind(source_id)
    .bind(empty_to_none(form.external_id))
    .bind(empty_to_none(form.external_url))
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/records/{id}")).into_response())
}

pub async fn file(state: AppState, id: Uuid) -> AppResult<Response> {
    let row: Option<(Option<String>, Option<String>, Option<i64>, String)> = sqlx::query_as(
        "select file_path, content_type, byte_size, title from records where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (file_path, content_type, byte_size, title) = row.ok_or(AppError::NotFound)?;
    serve_file(&state, file_path, content_type, byte_size, &title).await
}

pub async fn file_route(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    file(state, id).await
}

pub async fn preview_route(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    let row: Option<(Option<String>, Option<String>, String)> = sqlx::query_as(
        "select preview_path, preview_content_type, title from records where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (preview_path, preview_ct, title) = row.ok_or(AppError::NotFound)?;
    serve_file(&state, preview_path, preview_ct, None, &title).await
}

pub async fn thumbnail_route(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    let row: Option<(Option<String>, Option<String>, String)> = sqlx::query_as(
        "select thumbnail_path, thumbnail_content_type, title from records where id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (path, ct, title) = row.ok_or(AppError::NotFound)?;
    serve_file(&state, path, ct, None, &title).await
}

async fn serve_file(
    state: &AppState,
    file_path: Option<String>,
    content_type: Option<String>,
    byte_size: Option<i64>,
    title: &str,
) -> AppResult<Response> {
    let rel = file_path.ok_or(AppError::NotFound)?;
    let abs = files::resolve(&state.files_dir, &rel)
        .ok_or_else(|| AppError::BadRequest("invalid path".into()))?;
    let f = File::open(&abs).await?;
    let stream = ReaderStream::new(f);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    let ct = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    if let Ok(v) = HeaderValue::from_str(&ct) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    if let Some(n) = byte_size {
        if let Ok(v) = HeaderValue::from_str(&n.to_string()) {
            headers.insert(header::CONTENT_LENGTH, v);
        }
    }
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{}\"", sanitize(&title))) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok((StatusCode::OK, headers, body).into_response())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

async fn fetch_record(pool: &PgPool, id: Uuid) -> Result<Record, sqlx::Error> {
    sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records where id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

async fn all_sources(pool: &PgPool) -> Result<Vec<Source>, sqlx::Error> {
    sqlx::query_as::<_, Source>("select * from sources order by name")
        .fetch_all(pool)
        .await
}
