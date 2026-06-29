//! Import immunization records from CDPH SMART Health Card URLs.
//!
//! The CDPH Digital Vaccine Record portal (myvaccinerecord.cdph.ca.gov) sends
//! an email with per-person links valid for 24 hours. Paste those links into
//! the import form on Settings → Sync; this module fetches each URL
//! immediately, decodes the SMART Health Card JWS, and upserts the
//! Immunization resources.
//!
//! Subject matching: the SHC FHIR bundle includes a Patient resource. We
//! match it case-insensitively on family+given name against subjects in the
//! DB. No per-subject configuration is required.
//!
//! URL extraction: the page URL (`.../qr/en/DVR/<token>`) is a React SPA.
//! We fetch the HTML and search for the compact JWS embedded in the page
//! state (Next.js `__NEXT_DATA__` or an inline `verifiableCredential` value).
//! If not found we return a clear error.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sqlx::PgPool;
use time::Date;
use time::macros::format_description;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Import vaccine records from one or more CDPH portal URLs.
///
/// Each URL should be the link received in the CDPH email
/// (`https://myvaccinerecord.cdph.ca.gov/qr/en/DVR/<token>`).
/// URLs are fetched immediately; call this within 24 h of receiving the email.
pub async fn import_from_urls(
    pool: &PgPool,
    urls: Vec<String>,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("personal-emr/1.0 (family health record sync)")
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let subjects = sqlx::query_as::<_, (Uuid, String, String)>(
        "select id, given_name, family_name from subjects order by family_name, given_name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error loading subjects: {e}"))?;

    let source_id = ensure_cair_source(pool).await?;

    let mut total_upserted = 0u32;
    let mut messages: Vec<String> = Vec::new();

    for url in &urls {
        let url = url.trim();
        if url.is_empty() {
            continue;
        }
        match process_url(pool, &client, source_id, &subjects, url).await {
            Ok((subject_name, count)) => {
                messages.push(format!("{subject_name}: {count} immunization(s)"));
                total_upserted += count;
            }
            Err(e) => messages.push(format!("Error ({url}): {e}")),
        }
    }

    if messages.is_empty() {
        return Err("No URLs provided.".into());
    }

    let summary = format!(
        "Imported {total_upserted} immunization(s). {}",
        messages.join("; ")
    );

    if messages.iter().any(|m| m.starts_with("Error")) {
        Err(summary)
    } else {
        Ok(summary)
    }
}

// ---------------------------------------------------------------------------
// Per-URL processing
// ---------------------------------------------------------------------------

async fn process_url(
    pool: &PgPool,
    client: &reqwest::Client,
    source_id: Uuid,
    subjects: &[(Uuid, String, String)],
    url: &str,
) -> Result<(String, u32), String> {
    let jws = fetch_jws(client, url).await?;
    let (patient, immunizations) = parse_jws(&jws)?;

    let subject_id = match_subject(subjects, &patient)?;
    let subject_name = format!("{} {}", patient.given, patient.family);

    let mut count = 0u32;
    for (i, imm) in immunizations.iter().enumerate() {
        let ext_id = external_id(imm, i);
        upsert_immunization(pool, source_id, subject_id, imm, &ext_id).await?;
        count += 1;
    }

    tracing::info!(
        subject = %subject_name,
        count,
        "vaccine import completed"
    );
    Ok((subject_name, count))
}

// ---------------------------------------------------------------------------
// Fetching the SHC from a CDPH page URL
// ---------------------------------------------------------------------------

/// Fetches the CDPH page and extracts the compact JWS.
///
/// The portal page is a React SPA. We fetch the HTML and look for the
/// compact JWS (a base64url string of the form `eyJ…`) embedded in the
/// page state. The compact JWS starts with `eyJ` (base64url of `{`) in its
/// header segment.
async fn fetch_jws(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .header("Accept", "application/smart-health-card, application/json, text/html, */*")
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} fetching URL"));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    // Direct JSON response with verifiableCredential
    if content_type.contains("json") || content_type.contains("smart-health-card") {
        return extract_jws_from_json(&body);
    }

    // HTML page — search for the JWS embedded in page state
    extract_jws_from_html(&body)
}

fn extract_jws_from_json(body: &str) -> Result<String, String> {
    // {"verifiableCredential":["<compact-jws>"]}
    #[derive(Deserialize)]
    struct Shc {
        #[serde(rename = "verifiableCredential")]
        verifiable_credential: Vec<String>,
    }
    let shc: Shc = serde_json::from_str(body)
        .map_err(|e| format!("Could not parse SHC JSON: {e}"))?;
    shc.verifiable_credential
        .into_iter()
        .next()
        .ok_or_else(|| "SHC has no verifiableCredential entries".into())
}

/// Scans HTML for a compact JWS embedded in the page state.
///
/// Next.js apps embed server props in a `<script id="__NEXT_DATA__">` JSON
/// block. The JWS may also appear directly as a `verifiableCredential` value.
/// We look for the text `"verifiableCredential"` followed by `["eyJ` and
/// extract the credential string.
fn extract_jws_from_html(html: &str) -> Result<String, String> {
    // Strategy 1: find `"verifiableCredential":["eyJ` pattern
    const NEEDLE: &str = r#""verifiableCredential":["#;
    if let Some(start) = html.find(NEEDLE) {
        let after = &html[start + NEEDLE.len()..];
        // The value is a JSON string: starts with `"` then the JWS, ends with `"`
        let after = after.trim_start();
        if after.starts_with('"') {
            let inner = &after[1..];
            if let Some(end) = inner.find('"') {
                let candidate = &inner[..end];
                if looks_like_jws(candidate) {
                    return Ok(candidate.to_string());
                }
            }
        }
    }

    // Strategy 2: find a bare compact JWS starting with eyJ (header of ES256 SHC)
    // SHC header base64url-decodes to {"alg":"ES256","zip":"DEF","kid":"..."}
    // which always starts with eyJ. Look for eyJ...eyJ...
    // (header.payload — payload starts with eyJ only if uncompressed, but
    //  in SHC it's compressed so payload is NOT eyJ. Two consecutive eyJ
    //  segments separated by `.` means header.something.)
    if let Some(pos) = html.find("eyJhbGciOiJFUzI1NiIsInppcCI6IkRFRiIsImtpZCI6") {
        // Found a known SHC header prefix (base64url of {"alg":"ES256","zip":"DEF","kid":"})
        let fragment = &html[pos..];
        let end = fragment
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.')
            .unwrap_or(fragment.len());
        let candidate = &fragment[..end];
        if looks_like_jws(candidate) {
            return Ok(candidate.to_string());
        }
    }

    Err(
        "Could not find a SMART Health Card in the page response. \
         The portal URL may have expired (24-hour limit) or the page structure \
         has changed. Try downloading the .smart-health-card file from the portal \
         and uploading it below instead."
            .into(),
    )
}

fn looks_like_jws(s: &str) -> bool {
    // Compact JWS has exactly 2 dots separating 3 base64url segments
    s.starts_with("eyJ") && s.matches('.').count() == 2
}

// ---------------------------------------------------------------------------
// JWS decoding → FHIR Bundle → immunizations
// ---------------------------------------------------------------------------

struct PatientInfo {
    given: String,
    family: String,
}

fn parse_jws(jws: &str) -> Result<(PatientInfo, Vec<ParsedImmunization>), String> {
    let parts: Vec<&str> = jws.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err("JWS format invalid (expected header.payload.signature)".into());
    }

    let compressed = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| format!("JWS payload base64 decode failed: {e}"))?;

    let json_bytes = deflate_decompress(&compressed)?;

    let bundle: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|e| format!("FHIR bundle JSON parse failed: {e}"))?;

    let entries = bundle
        .get("entry")
        .and_then(|v| v.as_array())
        .ok_or("FHIR bundle has no entry array")?;

    let mut patient: Option<PatientInfo> = None;
    let mut immunizations = Vec::new();

    for entry in entries {
        let resource = match entry.get("resource") {
            Some(r) => r,
            None => continue,
        };
        match resource.get("resourceType").and_then(|v| v.as_str()) {
            Some("Patient") => {
                patient = extract_patient(resource);
            }
            Some("Immunization") => {
                if let Some(imm) = extract_immunization(resource) {
                    immunizations.push(imm);
                }
            }
            _ => {}
        }
    }

    let patient = patient.ok_or(
        "FHIR bundle has no Patient resource — cannot match to a subject".to_string(),
    )?;

    Ok((patient, immunizations))
}

fn deflate_decompress(compressed: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::read::DeflateDecoder;
    use std::io::Read;
    let mut decoder = DeflateDecoder::new(compressed);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| format!("DEFLATE decompress failed: {e}"))?;
    Ok(out)
}

fn extract_patient(res: &serde_json::Value) -> Option<PatientInfo> {
    let name = res.get("name")?.as_array()?.first()?;
    let family = name.get("family")?.as_str()?.to_string();
    let given = name
        .get("given")
        .and_then(|g| g.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(PatientInfo { given, family })
}

fn match_subject(
    subjects: &[(Uuid, String, String)],
    patient: &PatientInfo,
) -> Result<Uuid, String> {
    let pf = patient.family.to_lowercase();
    let pg = patient.given.to_lowercase();

    // Exact match on family + given
    for (id, given, family) in subjects {
        if family.to_lowercase() == pf && given.to_lowercase() == pg {
            return Ok(*id);
        }
    }

    // Prefix match on given name (e.g. SHC has "ROSHAN" but DB has "Roshan")
    for (id, given, family) in subjects {
        if family.to_lowercase() == pf
            && (given.to_lowercase().starts_with(&pg) || pg.starts_with(&given.to_lowercase()))
        {
            return Ok(*id);
        }
    }

    Err(format!(
        "No subject found for patient \"{} {}\" in the FHIR bundle. \
         Check that the name in your subjects list matches exactly.",
        patient.given, patient.family
    ))
}

// ---------------------------------------------------------------------------
// Immunization extraction
// ---------------------------------------------------------------------------

struct ParsedImmunization {
    vaccine: String,
    cvx_code: Option<String>,
    occurred_at: Option<Date>,
    lot_number: Option<String>,
    status: String,
}

fn extract_immunization(res: &serde_json::Value) -> Option<ParsedImmunization> {
    let fhir_status = res
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("completed");
    let status = match fhir_status {
        "not-done" => "not_given",
        "entered-in-error" => "entered_in_error",
        _ => "completed",
    }
    .to_string();

    let vaccine_code = res.get("vaccineCode")?;
    let vaccine = vaccine_code
        .get("text")
        .and_then(|v| v.as_str())
        .or_else(|| {
            vaccine_code
                .get("coding")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|e| e.get("display"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("Unknown vaccine")
        .to_string();

    let cvx_code = vaccine_code
        .get("coding")
        .and_then(|c| c.as_array())
        .and_then(|a| {
            a.iter().find(|e| {
                e.get("system")
                    .and_then(|s| s.as_str())
                    .map(|s| s.contains("cvx"))
                    .unwrap_or(false)
            })
        })
        .and_then(|e| e.get("code"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let occurred_at = res
        .get("occurrenceDateTime")
        .and_then(|v| v.as_str())
        .and_then(parse_fhir_date);

    let lot_number = res
        .get("lotNumber")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(ParsedImmunization {
        vaccine,
        cvx_code,
        occurred_at,
        lot_number,
        status,
    })
}

fn parse_fhir_date(s: &str) -> Option<Date> {
    let date_str = s.split('T').next()?;
    let fmt = format_description!("[year]-[month]-[day]");
    Date::parse(date_str, fmt).ok()
}

fn external_id(imm: &ParsedImmunization, idx: usize) -> String {
    let code = imm
        .cvx_code
        .clone()
        .unwrap_or_else(|| slug(&imm.vaccine));
    let date = imm
        .occurred_at
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!("shc_{code}_{date}__{idx}")
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

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

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
                 'Auto-created by vaccine import. Source: CDPH Smart Health Card.')",
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
    external_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "insert into immunizations
             (id, subject_id, vaccine, code, code_system, occurred_at,
              lot_number, status, source_id, external_id, source_synced_at)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,now())
         on conflict (source_id, external_id)
         where source_id is not null and external_id is not null
         do update set
             vaccine          = excluded.vaccine,
             code             = excluded.code,
             occurred_at      = excluded.occurred_at,
             lot_number       = excluded.lot_number,
             status           = excluded.status,
             source_synced_at = now()",
    )
    .bind(Uuid::now_v7())
    .bind(subject_id)
    .bind(&imm.vaccine)
    .bind(&imm.cvx_code)
    .bind(imm.cvx_code.as_ref().map(|_| "CVX"))
    .bind(imm.occurred_at)
    .bind(&imm.lot_number)
    .bind(&imm.status)
    .bind(source_id)
    .bind(external_id)
    .execute(pool)
    .await
    .map_err(|e| format!("DB upsert error: {e}"))?;
    Ok(())
}
