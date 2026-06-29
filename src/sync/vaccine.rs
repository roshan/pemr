//! Sync immunization records from CDPH's SMART Health Card download URL.
//!
//! Setup per subject:
//!   1. Go to myvaccinerecord.cdph.ca.gov, complete the OTP flow.
//!   2. On the results page, click the Download button to get the
//!      `.smart-health-card` file, OR right-click → Copy link address.
//!   3. Paste that URL into the subject's `cdph_shc_url` field.
//!
//! The task fetches each stored URL, parses the SMART Health Card JWS
//! (DEFLATE-compressed FHIR R4 bundle), and upserts the Immunization resources.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sqlx::PgPool;
use time::Date;
use time::macros::format_description;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ShcFile {
    #[serde(rename = "verifiableCredential")]
    verifiable_credential: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FhirBundle {
    entry: Option<Vec<FhirEntry>>,
}

#[derive(Debug, Deserialize)]
struct FhirEntry {
    resource: serde_json::Value,
}

#[derive(Debug)]
struct ParsedImmunization {
    vaccine: String,
    cvx_code: Option<String>,
    occurred_at: Option<Date>,
    lot_number: Option<String>,
    status: String,
}

pub async fn run(pool: PgPool) -> Result<String, String> {
    let subjects = sqlx::query_as::<_, (Uuid, String, String, Option<String>)>(
        "select id, given_name, family_name, cdph_shc_url
           from subjects
          where cdph_shc_url is not null
          order by family_name, given_name",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("DB error querying subjects: {e}"))?;

    if subjects.is_empty() {
        return Ok("No subjects have a CDPH vaccine record URL configured.".into());
    }

    let source_id = ensure_cair_source(&pool).await?;
    let client = reqwest::Client::builder()
        .user_agent("personal-emr/1.0 (family health record)")
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let mut synced = 0u32;
    let mut errors: Vec<String> = Vec::new();

    for (subject_id, given, family, url_opt) in subjects {
        let url = url_opt.unwrap();
        match sync_subject(&pool, &client, source_id, subject_id, &given, &family, &url).await {
            Ok(n) => synced += n,
            Err(e) => errors.push(format!("{given} {family}: {e}")),
        }
    }

    if errors.is_empty() {
        Ok(format!("Upserted {synced} immunization(s) from CAIR."))
    } else {
        Err(format!(
            "Upserted {synced} immunization(s); {} error(s): {}",
            errors.len(),
            errors.join("; ")
        ))
    }
}

async fn sync_subject(
    pool: &PgPool,
    client: &reqwest::Client,
    source_id: Uuid,
    subject_id: Uuid,
    given: &str,
    family: &str,
    url: &str,
) -> Result<u32, String> {
    let jws = fetch_jws(client, url).await?;
    let immunizations = parse_jws(&jws)?;
    let mut count = 0u32;
    for (i, imm) in immunizations.iter().enumerate() {
        let ext_id = external_id(imm, i);
        upsert_immunization(pool, source_id, subject_id, imm, &ext_id).await?;
        count += 1;
    }
    tracing::info!(
        subject = format!("{given} {family}"),
        count,
        "vaccine sync completed"
    );
    Ok(count)
}

/// Fetches the SHC from the URL and returns the compact JWS string.
///
/// Tries the URL as-is first (expects a `.smart-health-card` JSON file or an
/// `application/smart-health-card` response). If the response looks like HTML
/// instead, returns an error with guidance — the user should provide a direct
/// download URL, not the page URL.
async fn fetch_jws(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .header("Accept", "application/smart-health-card, application/json, */*")
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} fetching vaccine record URL"));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;

    if content_type.contains("html") {
        return Err(
            "URL returned an HTML page. Please provide the direct download URL for the \
             .smart-health-card file (right-click the Download button on the portal page \
             and copy the link address)."
                .into(),
        );
    }

    // Parse as SHC file JSON: {"verifiableCredential": ["<compact-jws>"]}
    let shc: ShcFile = serde_json::from_slice(&body)
        .map_err(|e| format!("Could not parse SMART Health Card JSON: {e}"))?;

    shc.verifiable_credential
        .into_iter()
        .next()
        .ok_or_else(|| "SMART Health Card has no verifiable credentials".into())
}

/// Decodes a SMART Health Card compact JWS into a list of immunizations.
///
/// SHC JWS payload is base64url-encoded DEFLATE-compressed FHIR R4 Bundle JSON.
fn parse_jws(jws: &str) -> Result<Vec<ParsedImmunization>, String> {
    let parts: Vec<&str> = jws.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err("JWS format invalid (expected header.payload.signature)".into());
    }

    let compressed = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| format!("JWS payload base64 decode failed: {e}"))?;

    let json = deflate_decompress(&compressed)?;

    let bundle: FhirBundle = serde_json::from_slice(&json)
        .map_err(|e| format!("FHIR bundle JSON parse failed: {e}"))?;

    let mut result = Vec::new();
    for entry in bundle.entry.unwrap_or_default() {
        let res = &entry.resource;
        if res.get("resourceType").and_then(|v| v.as_str()) != Some("Immunization") {
            continue;
        }
        if let Some(imm) = extract_immunization(res) {
            result.push(imm);
        }
    }
    Ok(result)
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

fn extract_immunization(res: &serde_json::Value) -> Option<ParsedImmunization> {
    let status = res
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("completed");

    // Map FHIR status to our vocabulary: completed / not_given / entered_in_error
    let status = match status {
        "not-done" => "not_given",
        "entered-in-error" => "entered_in_error",
        _ => "completed",
    }
    .to_string();

    // Vaccine code + display name
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

    // CVX code
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

    // Occurrence date — SHC uses occurrenceDateTime (ISO 8601)
    let occurred_at = res
        .get("occurrenceDateTime")
        .and_then(|v| v.as_str())
        .and_then(|s| parse_fhir_date(s));

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
    // FHIR date can be YYYY-MM-DD or YYYY-MM-DDThh:mm:ss... — take the date part
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
    format!("shc_{}_{}__{}", code, date, idx)
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
                 'Auto-created by vaccine sync task. CDPH SMART Health Card source.')",
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
         values
             ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now())
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
