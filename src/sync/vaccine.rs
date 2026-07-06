//! Import immunization records from CDPH Digital Vaccine Record page HTML.
//!
//! The CDPH portal (myvaccinerecord.cdph.ca.gov) sends an email with links
//! valid for 24 h. Each link renders a React/MUI page containing the full
//! CAIR immunization history as HTML tables — there is no embedded SMART
//! Health Card JWS. This module fetches the page and parses the tables
//! directly.
//!
//! Subject: the caller MUST specify who the record belongs to — we never
//! auto-detect from the name on the page (CAIR labels minors as "Dependent
//! Minor N", and guessing the subject is exactly the kind of mistake that must
//! not happen). The page's printed name is surfaced back for a sanity check.
//!
//! Deduplication: external_id = `cair_{subject}_{vaccine_slug}_{date}`. The
//! subject is in the key so two family members with the same vaccine on the same
//! day never collide. The dose number is NOT in the key: a person can't get the
//! same vaccine twice on one day, and combination vaccines (DTaP-IPV/Hib) are
//! listed once per disease group with the dose number filled in one group and
//! blank in another — keying on dose would store those as duplicates.

use sqlx::PgPool;
use time::Date;
use time::macros::format_description;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Import from one or more CDPH inputs, all assigned to `subject_id`.
///
/// Each entry in `inputs` can be either:
/// - A `https://myvaccinerecord.cdph.ca.gov/...` URL — the server fetches it
/// - Raw HTML pasted from the CDPH page (starts with `<`) — parsed directly
///
/// URL fetching works when the CDPH page is server-side rendered (which it
/// appears to be based on the rich HTML it returns). If the URL fetch doesn't
/// return the vaccine data, paste the page HTML directly instead.
///
/// The subject is **always** the caller-supplied `subject_id` — we do NOT
/// auto-detect from the name on the page (CAIR frequently labels minors as
/// "Dependent Minor N", and imports must never guess whose record this is). The
/// name CDPH printed on the page is surfaced in the result message so the caller
/// can eyeball that they picked the right person.
pub async fn import_from_urls(
    pool: &PgPool,
    inputs: Vec<String>,
    subject_id: Uuid,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (personal-emr vaccine sync)")
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let subject_name = sqlx::query_scalar::<_, String>("select full_name from subjects where id = $1")
        .bind(subject_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("DB error loading subject: {e}"))?
        .ok_or("Selected subject no longer exists.")?;

    let source_id = ensure_cair_source(pool).await?;

    let mut total = 0u32;
    let mut messages: Vec<String> = Vec::new();

    for input in &inputs {
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        match process_input(pool, &client, source_id, subject_id, input).await {
            Ok((page_name, count)) => {
                messages.push(format!("{count} immunization(s) (CDPH record for \"{page_name}\")"));
                total += count;
            }
            Err(e) => messages.push(format!("Error: {e}")),
        }
    }

    if messages.is_empty() {
        return Err("Nothing to import — paste a CDPH link or the page HTML.".into());
    }

    let summary = format!("{subject_name}: imported {total} immunization(s). {}", messages.join("; "));
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
    subject_id: Uuid,
    input: &str,
) -> Result<(String, u32), String> {
    let html = if input.starts_with("http://") || input.starts_with("https://") {
        fetch_html(client, input).await?
    } else {
        input.to_string()
    };

    let (page_name, immunizations) = parse_cdph_html(&html)?;

    let by_key = dedup_immunizations(subject_id, immunizations);
    let count = by_key.len() as u32;
    for (ext_id, imm) in &by_key {
        upsert_immunization(pool, source_id, subject_id, imm, ext_id).await?;
    }

    tracing::info!(subject = %subject_id, page_name = %page_name, count, "vaccine import completed");
    Ok((page_name, count))
}

/// Collapse the parsed rows to one per physical shot, keyed by
/// `external_id = (subject, vaccine, date)`. A person can't get the same vaccine
/// twice on one day, so date+vaccine identifies the shot regardless of the dose
/// number CDPH shows. Combination vaccines are listed once per disease group and
/// the dose column is sometimes filled in one group and blank in another (e.g.
/// DTaP-IPV/Hib shows "4" under DTP but blank under Polio) — keying on dose would
/// store those as two rows. We drop dose from the key and keep whichever listing
/// carries a real dose number.
fn dedup_immunizations(
    subject_id: Uuid,
    immunizations: Vec<ParsedImmunization>,
) -> std::collections::HashMap<String, ParsedImmunization> {
    let mut by_key: std::collections::HashMap<String, ParsedImmunization> =
        std::collections::HashMap::new();
    for imm in immunizations {
        let ext_id = external_id(subject_id, &imm);
        match by_key.get(&ext_id) {
            // Already have this shot with a dose number — keep it.
            Some(existing) if existing.dose_number.is_some() => {}
            // Otherwise take this one (fills in a dose the earlier copy lacked).
            _ => {
                by_key.insert(ext_id, imm);
            }
        }
    }
    by_key
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
// Deduplication and DB
// ---------------------------------------------------------------------------

/// Stable per-record dedup key: subject + vaccine + date. The subject is baked
/// in so two family members with an identical vaccine on the same day never
/// collide (the `(source_id, external_id)` unique index is global). The dose
/// number is deliberately NOT in the key — see `process_input`.
fn external_id(subject_id: Uuid, imm: &ParsedImmunization) -> String {
    let vax = slug(&imm.vaccine);
    let date = imm
        .occurred_at
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!("cair_{}_{vax}_{date}", subject_id.as_simple())
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
             -- keep a real dose number rather than let a blank listing wipe it
             dose_number      = coalesce(excluded.dose_number, immunizations.dose_number),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One `<tbody>` row: (vaccine, dose, date). Age/clinic columns are filler so
    /// the row has the ≥5 cells `parse_row` requires.
    fn row(vax: &str, dose: &str, date: &str) -> String {
        format!(
            "<tr><td>{vax}</td><td>{dose}</td><td>{date}</td><td>0y 1m</td><td>Clinic</td></tr>"
        )
    }

    fn tbody(rows: &[String]) -> String {
        format!("<table><tbody>{}</tbody></table>", rows.concat())
    }

    /// Reproduces the real CDPH bug: a combination vaccine (DTaP-IPV/Hib) is
    /// listed under multiple disease groups, with the dose number filled in one
    /// group ("4") and blank in another. It must collapse to ONE row that keeps
    /// the dose number — regardless of which listing is parsed first.
    #[test]
    fn combination_vaccine_with_blank_dose_dedups_to_one() {
        let subject = Uuid::now_v7();
        let html = format!(
            "<span>Name: </span>Astra Meridian </span>{}{}{}",
            // DTP group: dose "4"
            tbody(&[row("DTaP-IPV/Hib", "4", "07/01/2026")]),
            // Polio group: same shot, blank dose
            tbody(&[row("DTaP-IPV/Hib", "", "07/01/2026")]),
            // an unrelated shot on the same day
            tbody(&[row("Pneumococcal conjugate PCV15", "4", "07/01/2026")]),
        );

        let (_name, imms) = parse_cdph_html(&html).expect("parses");
        assert_eq!(imms.len(), 3, "three raw rows parsed");

        let deduped = dedup_immunizations(subject, imms);
        assert_eq!(deduped.len(), 2, "DTaP-IPV/Hib collapses to one; PCV15 distinct");

        let dtap: Vec<_> = deduped
            .values()
            .filter(|i| i.vaccine == "DTaP-IPV/Hib")
            .collect();
        assert_eq!(dtap.len(), 1, "exactly one DTaP-IPV/Hib row");
        assert_eq!(dtap[0].dose_number, Some(4), "keeps the real dose, not the blank");
    }

    /// The blank-first ordering must still keep the real dose (order-independent).
    #[test]
    fn blank_dose_first_still_keeps_real_dose() {
        let subject = Uuid::now_v7();
        let html = format!(
            "<span>Name: </span>X </span>{}{}",
            tbody(&[row("DTaP-IPV/Hib", "", "07/01/2026")]),
            tbody(&[row("DTaP-IPV/Hib", "4", "07/01/2026")]),
        );
        let (_n, imms) = parse_cdph_html(&html).expect("parses");
        let deduped = dedup_immunizations(subject, imms);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped.values().next().unwrap().dose_number, Some(4));
    }

    /// external_id is subject-scoped: the same shot for two people is two keys.
    #[test]
    fn external_id_is_subject_scoped() {
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let imm = ParsedImmunization {
            vaccine: "MMR".into(),
            dose_number: Some(1),
            occurred_at: Date::from_calendar_date(2026, time::Month::April, 27).ok(),
            status: "completed".into(),
        };
        assert_ne!(external_id(a, &imm), external_id(b, &imm));
        // ...but stable for the same subject (idempotent re-import).
        assert_eq!(external_id(a, &imm), external_id(a, &imm));
    }
}
