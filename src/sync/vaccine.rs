//! Import immunization records from CDPH Digital Vaccine Record page HTML.
//!
//! The CDPH portal (myvaccinerecord.cdph.ca.gov) sends an email with links
//! valid for 24 h. Each link renders a React/MUI page containing the full
//! CAIR immunization history as HTML tables — there is no embedded SMART
//! Health Card JWS. This module fetches the page and parses the tables
//! directly.
//!
//! Subject matching: the page header shows the patient's name. We match
//! case-insensitively against subjects.full_name.
//!
//! Deduplication: external_id = `cair_{vaccine_slug}_{date}_{dose}`.
//! Combination vaccines (DTaP-IPV/Hib) appear once per disease group but
//! carry the same name+date+dose, so they naturally collapse to one row.

use sqlx::PgPool;
use time::Date;
use time::macros::format_description;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Import from one or more CDPH inputs.
///
/// Each entry in `inputs` can be either:
/// - A `https://myvaccinerecord.cdph.ca.gov/...` URL — the server fetches it
/// - Raw HTML pasted from the CDPH page (starts with `<`) — parsed directly
///
/// URL fetching works when the CDPH page is server-side rendered (which it
/// appears to be based on the rich HTML it returns). If the URL fetch doesn't
/// return the vaccine data, paste the page HTML directly instead.
pub async fn import_from_urls(pool: &PgPool, inputs: Vec<String>) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (personal-emr vaccine sync)")
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let subjects = sqlx::query_as::<_, (Uuid, String)>(
        "select id, full_name from subjects order by family_name, given_name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error loading subjects: {e}"))?;

    let source_id = ensure_cair_source(pool).await?;

    let mut total = 0u32;
    let mut messages: Vec<String> = Vec::new();

    for input in &inputs {
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        match process_input(pool, &client, source_id, &subjects, input).await {
            Ok((name, count)) => {
                messages.push(format!("{name}: {count} immunization(s)"));
                total += count;
            }
            Err(e) => messages.push(format!("Error: {e}")),
        }
    }

    if messages.is_empty() {
        return Err("Nothing to import — paste a CDPH link or the page HTML.".into());
    }

    let summary = format!("Imported {total} immunization(s). {}", messages.join("; "));
    if messages.iter().any(|m| m.starts_with("Error")) {
        Err(summary)
    } else {
        Ok(summary)
    }
}

// ---------------------------------------------------------------------------
// Per-URL processing
// ---------------------------------------------------------------------------

async fn process_input(
    pool: &PgPool,
    client: &reqwest::Client,
    source_id: Uuid,
    subjects: &[(Uuid, String)],
    input: &str,
) -> Result<(String, u32), String> {
    let html = if input.starts_with("http://") || input.starts_with("https://") {
        fetch_html(client, input).await?
    } else {
        input.to_string()
    };

    let (full_name, immunizations) = parse_cdph_html(&html)?;
    let subject_id = match_subject(subjects, &full_name)?;

    let mut count = 0u32;
    // Deduplicate: same vaccine+date+dose appears once per disease group.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for imm in &immunizations {
        let ext_id = external_id(imm);
        if seen.insert(ext_id.clone()) {
            upsert_immunization(pool, source_id, subject_id, imm, &ext_id).await?;
            count += 1;
        }
    }

    tracing::info!(subject = %full_name, count, "vaccine import completed");
    Ok((full_name, count))
}

// ---------------------------------------------------------------------------
// HTTP fetch
// ---------------------------------------------------------------------------

async fn fetch_html(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .header("Accept", "text/html,*/*")
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} — the link may have expired (24-hour limit)"));
    }

    resp.text()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))
}

// ---------------------------------------------------------------------------
// HTML parsing
// ---------------------------------------------------------------------------

struct PatientInfo {
    full_name: String,
}

struct ParsedImmunization {
    vaccine: String,
    dose_number: Option<i32>,
    occurred_at: Option<Date>,
    status: String,
}

/// Extracts patient name and immunization rows from the CDPH page HTML.
///
/// The page structure:
///   <span><span style="font-weight: bold;">Name: </span>Astra Meridian </span>
///   <tbody>
///     <tr><td>DTaP-IPV/Hib</td><td>1</td><td>05/07/2025</td><td>0y 1m 29d</td><td>Clinic</td></tr>
///   </tbody>
fn parse_cdph_html(html: &str) -> Result<(String, Vec<ParsedImmunization>), String> {
    let full_name = extract_patient_name(html)?;
    let immunizations = parse_vaccine_tables(html);
    Ok((full_name, immunizations))
}

fn extract_patient_name(html: &str) -> Result<String, String> {
    const MARKER: &str = "Name: </span>";
    let pos = html
        .find(MARKER)
        .ok_or("Could not find patient name in page (wrong URL or page structure changed)")?;
    let rest = &html[pos + MARKER.len()..];
    let end = rest
        .find('<')
        .ok_or("Could not parse patient name from page")?;
    let name = rest[..end].trim().to_string();
    if name.is_empty() {
        return Err("Patient name was empty".into());
    }
    Ok(name)
}

fn parse_vaccine_tables(html: &str) -> Vec<ParsedImmunization> {
    let mut result = Vec::new();
    let mut search = html;

    while let Some(start) = search.find("<tbody") {
        let rest = &search[start..];
        if let Some(end) = rest.find("</tbody>") {
            let tbody = &rest[..end + 8];
            parse_tbody(tbody, &mut result);
            search = &rest[end + 8..];
        } else {
            break;
        }
    }

    result
}

fn parse_tbody(tbody: &str, result: &mut Vec<ParsedImmunization>) {
    let mut search = tbody;

    while let Some(tr_start) = search.find("<tr") {
        let rest = &search[tr_start..];
        if let Some(tr_end) = rest.find("</tr>") {
            let row = &rest[..tr_end + 5];
            if let Some(imm) = parse_row(row) {
                result.push(imm);
            }
            search = &rest[tr_end + 5..];
        } else {
            break;
        }
    }
}

fn parse_row(row: &str) -> Option<ParsedImmunization> {
    let cells = extract_td_texts(row);
    if cells.len() < 5 {
        return None; // skip header/recommendation rows (< 5 columns)
    }

    let vaccine = cells[0].trim().to_string();
    if vaccine.is_empty() || vaccine == "Vaccine" {
        return None;
    }

    let dose_text = cells[1].trim().to_string();
    let date_text = cells[2].trim();

    // "Invalid" means given too soon per CDC; dose still counts clinically.
    let is_invalid = dose_text.to_lowercase().starts_with("invalid");
    let dose_number = if is_invalid {
        None
    } else {
        dose_text.parse::<i32>().ok()
    };

    let occurred_at = parse_mmddyyyy(date_text);
    if occurred_at.is_none() && date_text.is_empty() {
        return None; // no date → skip (shouldn't happen in practice)
    }

    Some(ParsedImmunization {
        vaccine,
        dose_number,
        occurred_at,
        status: "completed".into(),
    })
}

/// Extracts the text content of each `<td>...</td>` in a `<tr>` block.
fn extract_td_texts(html: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut search = html;

    while let Some(td_start) = search.find("<td") {
        let rest = &search[td_start..];
        if let Some(tag_close) = rest.find('>') {
            let after_open = &rest[tag_close + 1..];
            if let Some(td_end) = after_open.find("</td>") {
                let content = strip_tags(&after_open[..td_end]);
                result.push(content.trim().to_string());
                search = &after_open[td_end + 5..];
            } else {
                break;
            }
        } else {
            break;
        }
    }

    result
}

/// Removes all HTML tags, leaving only text nodes.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn parse_mmddyyyy(s: &str) -> Option<Date> {
    let fmt = format_description!("[month]/[day]/[year]");
    Date::parse(s, fmt).ok()
}

// ---------------------------------------------------------------------------
// Subject matching
// ---------------------------------------------------------------------------

fn match_subject(subjects: &[(Uuid, String)], name: &str) -> Result<Uuid, String> {
    let needle = name.to_lowercase();

    // Exact full_name match
    for (id, full_name) in subjects {
        if full_name.to_lowercase() == needle {
            return Ok(*id);
        }
    }

    // Substring match (handles trailing spaces, middle names, etc.)
    for (id, full_name) in subjects {
        let fn_lower = full_name.to_lowercase();
        if fn_lower.contains(&needle) || needle.contains(&fn_lower) {
            return Ok(*id);
        }
    }

    let known: Vec<&str> = subjects.iter().map(|(_, n)| n.as_str()).collect();
    Err(format!(
        "No subject matched \"{name}\" — known subjects: {}. \
         Check that the name in your subjects list matches what CAIR has on file.",
        known.join(", ")
    ))
}

// ---------------------------------------------------------------------------
// Deduplication and DB
// ---------------------------------------------------------------------------

fn external_id(imm: &ParsedImmunization) -> String {
    let vax = slug(&imm.vaccine);
    let date = imm
        .occurred_at
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".into());
    let dose = imm.dose_number.unwrap_or(0);
    format!("cair_{vax}_{date}_{dose}")
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

async fn ensure_cair_source(pool: &PgPool) -> Result<Uuid, String> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "select id from sources where lower(name) = 'ca immunization registry (cair)'",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB error finding CAIR source: {e}"))?
    {
        return Ok(id);
    }

    let id = Uuid::now_v7();
    sqlx::query(
        "insert into sources (id, name, kind, notes)
         values ($1, 'CA Immunization Registry (CAIR)', 'registry',
                 'Auto-created by CDPH vaccine import.')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| format!("DB error creating CAIR source: {e}"))?;
    Ok(id)
}

async fn upsert_immunization(
    pool: &PgPool,
    source_id: Uuid,
    subject_id: Uuid,
    imm: &ParsedImmunization,
    ext_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "insert into immunizations
             (id, subject_id, vaccine, occurred_at, dose_number,
              status, source_id, external_id, source_synced_at)
         values ($1,$2,$3,$4,$5,$6,$7,$8,now())
         on conflict (source_id, external_id)
         where source_id is not null and external_id is not null
         do update set
             vaccine          = excluded.vaccine,
             occurred_at      = excluded.occurred_at,
             dose_number      = excluded.dose_number,
             status           = excluded.status,
             source_synced_at = now()",
    )
    .bind(Uuid::now_v7())
    .bind(subject_id)
    .bind(&imm.vaccine)
    .bind(imm.occurred_at)
    .bind(imm.dose_number)
    .bind(&imm.status)
    .bind(source_id)
    .bind(ext_id)
    .execute(pool)
    .await
    .map_err(|e| format!("DB upsert error: {e}"))?;
    Ok(())
}
