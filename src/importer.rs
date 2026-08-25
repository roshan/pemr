//! Bundle importers (PEMR-26): map external clinical exports → our tables.
//!
//! - FHIR R4 `Bundle` (JSON) — Apple Health clinical records, Epic FHIR export,
//!   Blue Button. Implemented here, no extra deps (serde_json).
//! - C-CDA (XML) — classic MyChart "Download My Record". See `importer_ccda`.
//!
//! Built against the specs; validate against a real export before trusting a new
//! source. Every row is upserted on the provenance key (source_id, external_id =
//! the resource id) so re-importing the same bundle is idempotent. The subject
//! is supplied explicitly by the caller (cross-system Patient→subject matching
//! is a separate concern — see subject_identifiers).

use serde_json::Value;
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use time::Date;
use uuid::Uuid;

#[derive(Debug, Default, serde::Serialize)]
pub struct Counts {
    pub allergies: i64,
    pub medications: i64,
    pub conditions: i64,
    pub incidents: i64,
    pub immunizations: i64,
    pub observations: i64,
    pub skipped: i64,
    /// Low-fidelity signals (parse failures, categories that came through empty,
    /// dropped entries) so an importing caller can react rather than assume a
    /// clean import. Empty == high fidelity.
    pub warnings: Vec<String>,
}

/// First 10 chars of a FHIR date/dateTime → `Date`.
fn fhir_date(s: &str) -> Option<Date> {
    let d = s.get(..10)?;
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(d, fmt).ok()
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

/// (display, code, system) from a FHIR CodeableConcept.
fn codeable(cc: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let coding0 = cc.get("coding").and_then(|c| c.get(0));
    let display = str_at(cc, "text")
        .map(str::to_string)
        .or_else(|| coding0.and_then(|c| str_at(c, "display")).map(str::to_string));
    let code = coding0.and_then(|c| str_at(c, "code")).map(str::to_string);
    let system = coding0.and_then(|c| str_at(c, "system")).map(str::to_string);
    (display, code, system)
}

/// Shorten a FHIR coding `system` URI to our code_system labels.
fn code_system_label(system: Option<&str>, code: &Option<String>) -> Option<String> {
    let sys = system?;
    let label = if sys.contains("loinc") {
        "LOINC"
    } else if sys.contains("rxnorm") {
        "RxNorm"
    } else if sys.contains("cvx") {
        "CVX"
    } else if sys.contains("icd-10") || sys.contains("icd10") {
        "ICD-10"
    } else if sys.contains("snomed") {
        "SNOMED"
    } else {
        return code.as_ref().map(|_| sys.to_string());
    };
    Some(label.to_string())
}

fn clinical_status(resource: &Value) -> Option<String> {
    let cc = resource.get("clinicalStatus")?;
    let (_, code, _) = codeable(cc);
    code
}

const ON_CONFLICT: &str = " on conflict (source_id, external_id) \
     where source_id is not null and external_id is not null do update set ";

/// Import a FHIR R4 Bundle for `subject_id`, attributing rows to `source_id`.
pub async fn import_fhir(
    pool: &PgPool,
    subject_id: Uuid,
    source_id: Uuid,
    bundle: &Value,
) -> Result<Counts, sqlx::Error> {
    let mut c = Counts::default();
    let entries = bundle
        .get("entry")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();

    for entry in &entries {
        let r = match entry.get("resource") {
            Some(r) => r,
            None => continue,
        };
        let rtype = str_at(r, "resourceType").unwrap_or("");
        let ext_id = str_at(r, "id").map(str::to_string);
        // Without a resource id we can't dedup; synthesize one is unsafe, so skip.
        let ext_id = match ext_id {
            Some(i) => i,
            None => {
                c.skipped += 1;
                continue;
            }
        };

        match rtype {
            "AllergyIntolerance" => {
                let (substance, code, system) = r
                    .get("code")
                    .map(codeable)
                    .unwrap_or((None, None, None));
                let substance = substance.unwrap_or_else(|| "Unknown allergen".into());
                let status = match clinical_status(r).as_deref() {
                    Some("inactive") => "inactive",
                    Some("resolved") => "resolved",
                    _ => "active",
                };
                let reaction = r
                    .get("reaction")
                    .and_then(|x| x.get(0))
                    .and_then(|x| x.get("manifestation"))
                    .and_then(|m| m.get(0))
                    .map(codeable)
                    .and_then(|(d, _, _)| d);
                sqlx::query(&format!(
                    "insert into allergies (id, subject_id, substance, code, code_system, reaction, status, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8,$9){ON_CONFLICT}
                        substance=excluded.substance, code=excluded.code, code_system=excluded.code_system,
                        reaction=excluded.reaction, status=excluded.status, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&substance).bind(&code)
                .bind(code_system_label(system.as_deref(), &code)).bind(reaction).bind(status)
                .bind(source_id).bind(&ext_id)
                .execute(pool).await?;
                c.allergies += 1;
            }
            "Condition" => {
                let (name, code, system) = r.get("code").map(codeable).unwrap_or((None, None, None));
                let name = name.unwrap_or_else(|| "Unknown condition".into());
                let cs = code_system_label(system.as_deref(), &code);
                let onset = str_at(r, "onsetDateTime").and_then(fhir_date);
                // A delivery/birth is a real-world event, not a chronic condition.
                if is_birth_event(&name, code.as_deref()) {
                    let existing: Option<Uuid> = sqlx::query_scalar(
                        "select id from incidents where subject_id=$1 and lower(title)=lower($2)
                           and occurred_at is not distinct from $3 limit 1",
                    )
                    .bind(subject_id).bind(&name).bind(onset).fetch_optional(pool).await?;
                    if existing.is_none() {
                        sqlx::query(
                            "insert into incidents (id, subject_id, title, narrative, occurred_at, occurred_precision)
                             values ($1,$2,$3,'',$4,'day')",
                        )
                        .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(onset)
                        .execute(pool).await?;
                    }
                    c.incidents += 1;
                    continue;
                }
                let status = match clinical_status(r).as_deref() {
                    Some("resolved") => "resolved",
                    Some("remission") => "remission",
                    _ => "active",
                };
                // Dedup chronic problems by code (same problem, different visit id).
                if let Some(codev) = &code {
                    let existing: Option<Uuid> = sqlx::query_scalar(
                        "select id from conditions where subject_id=$1 and code=$2
                           and code_system is not distinct from $3 limit 1",
                    )
                    .bind(subject_id).bind(codev).bind(&cs).fetch_optional(pool).await?;
                    if let Some(id) = existing {
                        sqlx::query(
                            "update conditions set name=$2, status=$3,
                               onset_date=coalesce($4, onset_date), updated_at=now() where id=$1",
                        )
                        .bind(id).bind(&name).bind(status).bind(onset).execute(pool).await?;
                        c.conditions += 1;
                        continue;
                    }
                }
                sqlx::query(&format!(
                    "insert into conditions (id, subject_id, name, code, code_system, status, onset_date, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8,$9){ON_CONFLICT}
                        name=excluded.name, code=excluded.code, code_system=excluded.code_system,
                        status=excluded.status, onset_date=excluded.onset_date, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code)
                .bind(&cs).bind(status).bind(onset)
                .bind(source_id).bind(&ext_id)
                .execute(pool).await?;
                c.conditions += 1;
            }
            "MedicationStatement" | "MedicationRequest" => {
                let cc = r
                    .get("medicationCodeableConcept")
                    .map(codeable)
                    .unwrap_or((None, None, None));
                let name = cc.0.unwrap_or_else(|| "Unknown medication".into());
                let status = match str_at(r, "status") {
                    Some("completed") => "completed",
                    Some("stopped") | Some("cancelled") => "stopped",
                    Some("on-hold") => "on_hold",
                    _ => "active",
                };
                let dosage = r
                    .get("dosage")
                    .or_else(|| r.get("dosageInstruction"))
                    .and_then(|d| d.get(0))
                    .and_then(|d| str_at(d, "text"))
                    .map(str::to_string);
                sqlx::query(&format!(
                    "insert into medications (id, subject_id, name, code, code_system, frequency, status, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8,$9){ON_CONFLICT}
                        name=excluded.name, code=excluded.code, code_system=excluded.code_system,
                        frequency=excluded.frequency, status=excluded.status, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&cc.1)
                .bind(code_system_label(cc.2.as_deref(), &cc.1)).bind(dosage).bind(status)
                .bind(source_id).bind(&ext_id)
                .execute(pool).await?;
                c.medications += 1;
            }
            "Immunization" => {
                let (vaccine, code, system) = r
                    .get("vaccineCode")
                    .map(codeable)
                    .unwrap_or((None, None, None));
                let vaccine = vaccine.unwrap_or_else(|| "Unknown vaccine".into());
                let status = match str_at(r, "status") {
                    Some("not-done") => "not_given",
                    Some("entered-in-error") => "entered_in_error",
                    _ => "completed",
                };
                let occurred = str_at(r, "occurrenceDateTime").and_then(fhir_date);
                // Cross-source dedup (PEMR-48): if a row for this subject already
                // carries the same shot (same date + vaccine family/CVX concept),
                // merge the richer fields into it rather than insert a duplicate.
                if let Some(existing_id) = crate::dedupe::find_immunization_match(
                    pool,
                    subject_id,
                    &vaccine,
                    code.as_deref(),
                    occurred,
                ).await? {
                    crate::dedupe::merge_immunization(
                        pool,
                        existing_id,
                        &vaccine,
                        code.as_deref(),
                        code_system_label(system.as_deref(), &code).as_deref(),
                        occurred,
                        None, None, None, None,
                    ).await?;
                } else {
                    sqlx::query(&format!(
                        "insert into immunizations (id, subject_id, vaccine, code, code_system, occurred_at, status, source_id, external_id)
                         values ($1,$2,$3,$4,$5,$6,$7,$8,$9){ON_CONFLICT}
                            vaccine=excluded.vaccine, code=excluded.code, code_system=excluded.code_system,
                            occurred_at=excluded.occurred_at, status=excluded.status, updated_at=now()"
                    ))
                    .bind(Uuid::now_v7()).bind(subject_id).bind(&vaccine).bind(&code)
                    .bind(code_system_label(system.as_deref(), &code)).bind(occurred).bind(status)
                    .bind(source_id).bind(&ext_id)
                    .execute(pool).await?;
                }
                c.immunizations += 1;
            }
            "Observation" => {
                let imported = import_fhir_observation(pool, subject_id, source_id, r, &ext_id).await?;
                if imported {
                    c.observations += 1;
                } else {
                    c.skipped += 1;
                }
            }
            _ => {
                c.skipped += 1;
            }
        }
    }
    Ok(c)
}

async fn import_fhir_observation(
    pool: &PgPool,
    subject_id: Uuid,
    source_id: Uuid,
    r: &Value,
    ext_id: &str,
) -> Result<bool, sqlx::Error> {
    let (display, code, system) = r.get("code").map(codeable).unwrap_or((None, None, None));
    let display = match display {
        Some(d) => d,
        None => return Ok(false),
    };
    // effective_on is required (not null) — skip if absent.
    let effective_on = match str_at(r, "effectiveDateTime")
        .and_then(fhir_date)
        .or_else(|| {
            r.get("effectivePeriod")
                .and_then(|p| str_at(p, "start"))
                .and_then(fhir_date)
        }) {
        Some(d) => d,
        None => return Ok(false),
    };
    let category = match r
        .get("category")
        .and_then(|c| c.get(0))
        .map(codeable)
        .and_then(|(_, code, _)| code)
        .as_deref()
    {
        Some("vital-signs") => "vital",
        Some("laboratory") => "lab",
        _ => "measurement",
    };
    let (value_num, value_text, unit) = match r.get("valueQuantity") {
        Some(q) => (
            q.get("value").and_then(|v| v.as_f64()),
            None,
            str_at(q, "unit").map(str::to_string),
        ),
        None => (None, str_at(r, "valueString").map(str::to_string), None),
    };
    // Cross-source dedup (PEMR-48): the same measurement (LOINC code or lower
    // display) on the same effective date → merge richer fields in place rather
    // than insert a duplicate row.
    if let Some(existing_id) = crate::dedupe::find_observation_match(
        pool, subject_id, code.as_deref(), &display, effective_on,
    )
    .await?
    {
        crate::dedupe::merge_observation(
            pool,
            existing_id,
            category,
            code.as_deref(),
            code_system_label(system.as_deref(), &code).as_deref(),
            &display,
            value_num,
            value_text.as_deref(),
            unit.as_deref(),
            None,
            None,
            None,
            None,
            effective_on,
        )
        .await?;
        return Ok(true);
    }
    sqlx::query(&format!(
        "insert into observations (id, subject_id, category, code, code_system, display, value_num, value_text, unit, effective_on, source_id, external_id)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12){ON_CONFLICT}
            category=excluded.category, code=excluded.code, code_system=excluded.code_system,
            display=excluded.display, value_num=excluded.value_num, value_text=excluded.value_text,
            unit=excluded.unit, effective_on=excluded.effective_on, updated_at=now()"
    ))
    .bind(Uuid::now_v7()).bind(subject_id).bind(category).bind(&code)
    .bind(code_system_label(system.as_deref(), &code)).bind(&display)
    .bind(value_num).bind(value_text).bind(unit).bind(effective_on)
    .bind(source_id).bind(ext_id)
    .execute(pool).await?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// C-CDA (XML) — classic MyChart "Download My Record". Best-effort against the
// C-CDA R2.1 section/entry templates; VALIDATE against a real export before
// trusting a new source. Parsed into owned items first (roxmltree DOM is not
// held across an await), then upserted like the FHIR path.
// ---------------------------------------------------------------------------

use std::collections::HashMap;

use roxmltree::Node;

// C-CDA entry template ids (the `root` OIDs). We key on these rather than on
// section/element `displayName`s because real Epic exports routinely omit
// `displayName` and stash the human-readable value in the narrative <text> block
// (resolved via `resolve_display`). The original parser keyed on displayName and
// so extracted almost nothing from a real Sutter export.
const T_ALLERGY_OBS: &str = "2.16.840.1.113883.10.20.22.4.7"; // Allergy-Intolerance Observation
const T_SEVERITY_OBS: &str = "2.16.840.1.113883.10.20.22.4.8"; // Reaction Severity Observation
const T_PROBLEM_OBS: &str = "2.16.840.1.113883.10.20.22.4.4"; // Problem Observation
const T_MED_ACT: &str = "2.16.840.1.113883.10.20.22.4.16"; // Medication Activity
const T_IMMUNIZATION: &str = "2.16.840.1.113883.10.20.22.4.52"; // Immunization Activity
const T_RESULT_OBS: &str = "2.16.840.1.113883.10.20.22.4.2"; // Result Observation
const T_VITAL_OBS: &str = "2.16.840.1.113883.10.20.22.4.27"; // Vital Sign Observation

enum CItem {
    Allergy {
        substance: String,
        code: Option<String>,
        code_system: Option<String>,
        reaction: Option<String>,
        reaction_code: Option<String>,
        reaction_code_system: Option<String>,
        criticality: Option<String>,
        severity: Option<String>,
    },
    Medication {
        name: String,
        code: Option<String>,
        code_system: Option<String>,
        dose: Option<String>,
        route: Option<String>,
        status: Option<String>,
        started: Option<Date>,
    },
    Condition {
        name: String,
        code: Option<String>,
        code_system: Option<String>,
        status: Option<String>,
        onset: Option<Date>,
        resolved: Option<Date>,
    },
    /// A real-world event Epic filed in the problem list (e.g. a delivery) —
    /// routed to an incident rather than a chronic condition.
    Incident { title: String, occurred: Option<Date> },
    Immunization {
        vaccine: String,
        code: Option<String>,
        code_system: Option<String>,
        occurred: Option<Date>,
        dose_number: Option<i32>,
        lot_number: Option<String>,
        site: Option<String>,
        route: Option<String>,
    },
    Observation {
        display: String,
        code: Option<String>,
        code_system: Option<String>,
        value_num: Option<f64>,
        value_text: Option<String>,
        unit: Option<String>,
        ref_low: Option<f64>,
        ref_high: Option<f64>,
        abnormal_flag: Option<String>,
        panel_id: Option<Uuid>,
        effective: Date,
        category: &'static str,
    },
}

/// True if a "problem" is really a delivery/birth *event*, not a chronic
/// condition. Epic files the delivery in the OB problem list; we route those to
/// an incident instead. Matched by well-known SNOMED delivery codes or by name
/// (a problem-list "delivery"/"cesarean" is childbirth).
fn is_birth_event(name: &str, code: Option<&str>) -> bool {
    const DELIVERY_CODES: &[&str] = &[
        "289259007", // Vaginal delivery
        "11466000",  // Cesarean section
        "177184002", // Normal delivery procedure
    ];
    if let Some(c) = code {
        if DELIVERY_CODES.contains(&c) {
            return true;
        }
    }
    let n = name.to_lowercase();
    n.contains("delivery") || n.contains("cesarean") || n.contains("caesarean") || n.contains("c-section")
}

/// First 8 chars (`YYYYMMDD`) of an HL7 timestamp → `Date`.
fn ccda_date(s: &str) -> Option<Date> {
    let d = s.get(..8)?;
    let fmt = time::macros::format_description!("[year][month][day]");
    Date::parse(d, fmt).ok()
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn descendants_named<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.descendants().filter(move |d| d.tag_name().name() == name)
}

/// True if `n` carries a `<templateId root="...">` child with the given root.
fn has_template(n: Node<'_, '_>, root: &str) -> bool {
    n.children()
        .any(|c| c.tag_name().name() == "templateId" && c.attribute("root") == Some(root))
}

/// Concatenated, whitespace-collapsed text of all descendant text nodes. Only
/// genuine text nodes are collected — calling `.text()` on element nodes too
/// would double-count (it returns the element's first text child).
fn el_text(n: Node<'_, '_>) -> String {
    let mut s = String::new();
    for d in n.descendants().filter(Node::is_text) {
        if let Some(t) = d.text() {
            let t = t.trim();
            if !t.is_empty() {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(t);
            }
        }
    }
    s
}

/// The entry's HL7 `<id root^extension>` — our idempotent provenance key. The
/// same allergy/problem carries the same id in every document, so re-importing
/// the whole IHE_XDM package upserts instead of duplicating.
fn entry_id(n: Node<'_, '_>) -> Option<String> {
    let id = child(n, "id")?;
    match (id.attribute("root"), id.attribute("extension")) {
        (Some(r), Some(e)) => Some(format!("{r}^{e}")),
        (None, Some(e)) => Some(e.to_string()),
        (Some(r), None) => Some(r.to_string()),
        _ => None,
    }
}

/// Map every element bearing an `ID` attribute → its text. Structured entries
/// point at these (`<reference value="#problem114name"/>`) for the readable name
/// when no coded `displayName` is present.
fn narrative_map(doc: &roxmltree::Document) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for n in doc.descendants() {
        if let Some(id) = n.attribute("ID") {
            m.insert(id.to_string(), el_text(n));
        }
    }
    m
}

/// Resolve a readable name for a `<code>`/`<value>` node, trying in order:
/// inline `<originalText>`, a narrative `<reference>`, the `@displayName`, then a
/// `<translation>`'s displayName.
fn resolve_display(node: Node<'_, '_>, idmap: &HashMap<String, String>) -> Option<String> {
    if let Some(ot) = child(node, "originalText") {
        let t = el_text(ot);
        if !t.is_empty() {
            return Some(t);
        }
        if let Some(r) = child(ot, "reference").and_then(|r| r.attribute("value")) {
            if let Some(v) = idmap.get(r.trim_start_matches('#')) {
                if !v.is_empty() {
                    return Some(v.clone());
                }
            }
        }
    }
    if let Some(d) = node.attribute("displayName").filter(|s| !s.is_empty()) {
        return Some(d.to_string());
    }
    child(node, "translation")
        .and_then(|t| t.attribute("displayName"))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Allergy criticality (FHIR: potential seriousness on re-exposure) from the
/// Criticality observation (LOINC 82606-5). Maps HL7 CRITH/CRITL/CRITU →
/// high | low | unable-to-assess. Distinct from a reaction's `severity`.
fn criticality_of(obs: Node<'_, '_>) -> Option<String> {
    let value = descendants_named(obs, "observation")
        .find(|o| child(*o, "code").and_then(|c| c.attribute("code")) == Some("82606-5"))
        .and_then(|o| child(o, "value"))?;
    let code = value.attribute("code").unwrap_or("");
    let disp = value.attribute("displayName").unwrap_or("").to_lowercase();
    Some(
        match code {
            "CRITH" => "high",
            "CRITL" => "low",
            _ if disp.contains("high") => "high",
            _ if disp.contains("low") => "low",
            _ => "unable-to-assess",
        }
        .to_string(),
    )
}

/// Reaction severity (FHIR: how bad a reaction WAS) from the Reaction Severity
/// Observation (template …22.4.8), mapped to the SNOMED CT clinical-severity
/// value set that C-CDA / Epic emit and that `models::ALLERGY_SEVERITIES`
/// mirrors: mild | moderate | severe | life-threatening | fatal. Off-vocabulary
/// values are dropped (None) rather than stored verbatim. Often absent.
fn severity_of(obs: Node<'_, '_>) -> Option<String> {
    let value = descendants_named(obs, "observation")
        .find(|o| has_template(*o, T_SEVERITY_OBS))
        .and_then(|o| child(o, "value"))?;
    let code = value.attribute("code").unwrap_or("");
    let disp = value.attribute("displayName").unwrap_or("").to_lowercase();
    Some(
        match code {
            "255604002" => "mild",
            "6736007" => "moderate",
            "24484000" => "severe",
            "442452003" => "life-threatening",
            "399166001" => "fatal",
            _ if disp.contains("fatal") => "fatal",
            _ if disp.contains("life") => "life-threatening",
            _ if disp.contains("severe") => "severe",
            _ if disp.contains("moderate") => "moderate",
            _ if disp.contains("mild") => "mild",
            _ => return None,
        }
        .to_string(),
    )
}

/// Normalize an HL7 `codeSystemName` to our short `code_system` labels.
fn norm_system(s: Option<&str>) -> Option<String> {
    let up = s?.to_uppercase();
    let label = if up.contains("LOINC") {
        "LOINC"
    } else if up.contains("RXNORM") {
        "RxNorm"
    } else if up.contains("CVX") {
        "CVX"
    } else if up.contains("ICD-10") || up.contains("ICD10") {
        "ICD-10"
    } else if up.contains("ICD-9") || up.contains("ICD9") {
        "ICD-9"
    } else if up.contains("SNOMED") {
        "SNOMED"
    } else {
        return s.map(str::to_string);
    };
    Some(label.to_string())
}

/// HL7 ObservationInterpretation code → our `abnormal_flag` vocabulary.
fn abnormal_flag(obs: Node<'_, '_>) -> Option<String> {
    let code = descendants_named(obs, "interpretationCode")
        .next()?
        .attribute("code")?;
    Some(
        match code {
            "N" => "normal",
            "H" | "HH" | "HU" | ">" => "high",
            "L" | "LL" | "LU" | "<" => "low",
            _ => "abnormal",
        }
        .to_string(),
    )
}

/// active / resolved / inactive, from the nested Status observation (LOINC
/// 33999-4) Epic hangs off each problem.
fn condition_status(obs: Node<'_, '_>) -> Option<String> {
    for o in descendants_named(obs, "observation") {
        if child(o, "code").and_then(|c| c.attribute("code")) == Some("33999-4") {
            if let Some(v) = child(o, "value").and_then(|v| v.attribute("displayName")) {
                return Some(v.to_lowercase());
            }
        }
    }
    None
}

/// Deterministic panel id for a result `<organizer>` (one lab draw / battery), so
/// the analytes of a CBC etc. share `observations.panel_id` across re-imports.
fn panel_uuid(org: Node<'_, '_>) -> Option<Uuid> {
    let id = entry_id(org)?;
    let bytes = hex::decode(crate::api_auth::sha256_hex(id.as_bytes())).ok()?;
    Uuid::from_slice(&bytes[..16]).ok()
}

fn hid(kind: &str, key: &str) -> String {
    crate::api_auth::sha256_hex(format!("{kind}|{key}").as_bytes())
}

/// The document subject's `(given, family)` name from `recordTarget`, used to
/// match the import against a `subjects` row.
pub fn ccda_patient_name(xml: &str) -> Option<(String, String)> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let name = doc
        .descendants()
        .find(|n| n.tag_name().name() == "patientRole")
        .and_then(|pr| child(pr, "patient"))
        .and_then(|p| child(p, "name"))?;
    let given = child(name, "given").map(el_text).unwrap_or_default();
    let family = child(name, "family").map(el_text).unwrap_or_default();
    if given.is_empty() && family.is_empty() {
        None
    } else {
        Some((given, family))
    }
}

/// Per-document parse fidelity: did it parse, which target sections appeared, and
/// how many entries were dropped — aggregated into importer warnings.
#[derive(Default)]
struct ParseStats {
    parse_failed: bool,
    skipped: i64,
    sections: std::collections::HashSet<&'static str>,
}

/// Parse a C-CDA document into normalized items keyed by their provenance id.
/// Sync; returns owned data (the roxmltree DOM is not held across an await).
fn parse_ccda(xml: &str) -> (Vec<(String, CItem)>, ParseStats) {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return (Vec::new(), ParseStats { parse_failed: true, ..Default::default() }),
    };
    let idmap = narrative_map(&doc);
    let mut out: Vec<(String, CItem)> = Vec::new();
    let mut stats = ParseStats::default();

    for section in doc.descendants().filter(|n| n.tag_name().name() == "section") {
        let sec_code = child(section, "code")
            .and_then(|c| c.attribute("code"))
            .unwrap_or("");
        match sec_code {
            // Allergies — name lives in participant/playingEntity/name (the coded
            // value is usually nullFlavor); reaction + criticality hang off it.
            "48765-2" => {
                stats.sections.insert("allergies");
                for obs in descendants_named(section, "observation") {
                    if !has_template(obs, T_ALLERGY_OBS) {
                        continue;
                    }
                    let pe = descendants_named(obs, "playingEntity").next();
                    let substance = match pe
                        .and_then(|pe| child(pe, "name"))
                        .map(el_text)
                        .filter(|s| !s.is_empty())
                    {
                        Some(s) => s,
                        None => {
                            stats.skipped += 1;
                            continue;
                        }
                    };
                    let code_node = pe.and_then(|pe| child(pe, "code"));
                    // Reaction manifestation(s) — FHIR reaction.manifestation: a
                    // coded concept (SNOMED CT) plus display text. An allergy may
                    // list several; join the text, keep the first code.
                    let mut manifestations: Vec<String> = Vec::new();
                    let mut reaction_code = None;
                    let mut reaction_code_system = None;
                    for er in obs.children().filter(|n| {
                        n.tag_name().name() == "entryRelationship"
                            && n.attribute("typeCode") == Some("MFST")
                    }) {
                        if let Some(val) =
                            descendants_named(er, "observation").next().and_then(|o| child(o, "value"))
                        {
                            if let Some(d) = resolve_display(val, &idmap) {
                                manifestations.push(d);
                            }
                            if reaction_code.is_none() {
                                reaction_code = val.attribute("code").map(str::to_string);
                                reaction_code_system = norm_system(val.attribute("codeSystemName"));
                            }
                        }
                    }
                    let ext_id = entry_id(obs).unwrap_or_else(|| hid("allergy", &substance));
                    out.push((
                        ext_id,
                        CItem::Allergy {
                            substance,
                            code: code_node.and_then(|c| c.attribute("code")).map(str::to_string),
                            code_system: code_node
                                .and_then(|c| norm_system(c.attribute("codeSystemName"))),
                            reaction: (!manifestations.is_empty()).then(|| manifestations.join(", ")),
                            reaction_code,
                            reaction_code_system,
                            criticality: criticality_of(obs),
                            severity: severity_of(obs),
                        },
                    ));
                }
            }
            // Medications
            "10160-0" => {
                stats.sections.insert("medications");
                for sa in descendants_named(section, "substanceAdministration") {
                    if !has_template(sa, T_MED_ACT) {
                        continue;
                    }
                    let mm = descendants_named(sa, "manufacturedMaterial").next();
                    let code_node = mm.and_then(|m| child(m, "code"));
                    let name = match code_node
                        .and_then(|c| resolve_display(c, &idmap))
                        .or_else(|| mm.and_then(|m| child(m, "name")).map(el_text))
                        .filter(|s| !s.is_empty())
                    {
                        Some(n) => n,
                        None => {
                            stats.skipped += 1;
                            continue;
                        }
                    };
                    let ext_id = entry_id(sa).unwrap_or_else(|| hid("med", &name));
                    out.push((
                        ext_id,
                        CItem::Medication {
                            name,
                            code: code_node.and_then(|c| c.attribute("code")).map(str::to_string),
                            code_system: code_node
                                .and_then(|c| norm_system(c.attribute("codeSystemName"))),
                            dose: None,
                            route: None,
                            status: None,
                            started: None,
                        },
                    ));
                }
            }
            // Problem list — skip the nested Status observation; the problem name
            // is referenced into the narrative, not on value/@displayName.
            "11450-4" => {
                stats.sections.insert("conditions");
                for obs in descendants_named(section, "observation") {
                    if !has_template(obs, T_PROBLEM_OBS) {
                        continue;
                    }
                    let value = child(obs, "value");
                    let name = match value.and_then(|v| resolve_display(v, &idmap)) {
                        Some(n) => n,
                        None => {
                            stats.skipped += 1;
                            continue;
                        }
                    };
                    // Skip Epic's "no known problems" negation assertions — these
                    // are not real conditions.
                    let lname = name.to_lowercase();
                    if lname.starts_with("no known") || lname.contains("no current problem") {
                        continue;
                    }
                    let onset = child(obs, "effectiveTime")
                        .and_then(|et| {
                            child(et, "low")
                                .and_then(|l| l.attribute("value"))
                                .or_else(|| et.attribute("value"))
                        })
                        .and_then(ccda_date);
                    let ext_id = entry_id(obs).unwrap_or_else(|| hid("cond", &name));
                    let code = value.and_then(|v| v.attribute("code")).map(str::to_string);
                    let code_system =
                        value.and_then(|v| norm_system(v.attribute("codeSystemName")));
                    // Epic files the delivery itself in the problem list — a
                    // delivery/birth is a real-world event, not a chronic
                    // condition, so route it to an incident.
                    let item = if is_birth_event(&name, code.as_deref()) {
                        CItem::Incident { title: name, occurred: onset }
                    } else {
                        CItem::Condition {
                            name,
                            code,
                            code_system,
                            status: condition_status(obs),
                            onset,
                            resolved: None,
                        }
                    };
                    out.push((ext_id, item));
                }
            }
            // Immunizations — CVX code present, name via originalText/narrative.
            "11369-6" => {
                stats.sections.insert("immunizations");
                for sa in descendants_named(section, "substanceAdministration") {
                    if !has_template(sa, T_IMMUNIZATION) {
                        continue;
                    }
                    let mm = descendants_named(sa, "manufacturedMaterial").next();
                    let code_node = mm.and_then(|m| child(m, "code"));
                    let vaccine = match code_node
                        .and_then(|c| resolve_display(c, &idmap))
                        .filter(|s| !s.is_empty())
                    {
                        Some(v) => v,
                        None => {
                            stats.skipped += 1;
                            continue;
                        }
                    };
                    let occurred = child(sa, "effectiveTime")
                        .and_then(|et| {
                            et.attribute("value")
                                .or_else(|| child(et, "low").and_then(|l| l.attribute("value")))
                        })
                        .and_then(ccda_date);
                    let ext_id = entry_id(sa)
                        .unwrap_or_else(|| hid("imm", &format!("{vaccine}|{occurred:?}")));
                    out.push((
                        ext_id,
                        CItem::Immunization {
                            vaccine,
                            code: code_node.and_then(|c| c.attribute("code")).map(str::to_string),
                            code_system: code_node
                                .and_then(|c| norm_system(c.attribute("codeSystemName"))),
                            occurred,
                            dose_number: None,
                            lot_number: None,
                            site: None,
                            route: None,
                        },
                    ));
                }
            }
            // Results (labs) + Vital signs — analytes are component observations
            // under an organizer; name in originalText, value/unit in attributes,
            // bounds in referenceRange. Grouped by panel (the organizer).
            "30954-2" | "8716-3" => {
                stats.sections.insert(if sec_code == "8716-3" { "vitals" } else { "labs" });
                let category = if sec_code == "8716-3" { "vital" } else { "lab" };
                let tmpl = if sec_code == "8716-3" {
                    T_VITAL_OBS
                } else {
                    T_RESULT_OBS
                };
                for organizer in descendants_named(section, "organizer") {
                    let panel_id = panel_uuid(organizer);
                    for obs in descendants_named(organizer, "observation") {
                        if !has_template(obs, tmpl) {
                            continue;
                        }
                        let code_node = child(obs, "code");
                        let display = match code_node.and_then(|c| resolve_display(c, &idmap)) {
                            Some(d) => d,
                            None => {
                                stats.skipped += 1;
                                continue;
                            }
                        };
                        let effective = match child(obs, "effectiveTime")
                            .and_then(|et| {
                                et.attribute("value").or_else(|| {
                                    child(et, "low").and_then(|l| l.attribute("value"))
                                })
                            })
                            .and_then(ccda_date)
                        {
                            Some(d) => d,
                            None => {
                                stats.skipped += 1;
                                continue;
                            }
                        };
                        let value = child(obs, "value");
                        let value_num = value
                            .and_then(|v| v.attribute("value"))
                            .and_then(|s| s.parse::<f64>().ok());
                        let unit = value.and_then(|v| v.attribute("unit")).map(str::to_string);
                        let value_text = if value_num.is_none() {
                            value
                                .map(el_text)
                                .filter(|s| !s.is_empty())
                                .or_else(|| {
                                    value
                                        .and_then(|v| v.attribute("displayName"))
                                        .map(str::to_string)
                                })
                        } else {
                            None
                        };
                        let (ref_low, ref_high) = child(obs, "referenceRange")
                            .and_then(|rr| child(rr, "observationRange"))
                            .and_then(|or| child(or, "value"))
                            .map(|iv| {
                                let n = |t| {
                                    child(iv, t)
                                        .and_then(|x| x.attribute("value"))
                                        .and_then(|s| s.parse::<f64>().ok())
                                };
                                (n("low"), n("high"))
                            })
                            .unwrap_or((None, None));
                        let ext_id = entry_id(obs)
                            .unwrap_or_else(|| hid("obs", &format!("{display}|{effective}")));
                        out.push((
                            ext_id,
                            CItem::Observation {
                                display,
                                code: code_node
                                    .and_then(|c| c.attribute("code"))
                                    .map(str::to_string),
                                code_system: code_node
                                    .and_then(|c| norm_system(c.attribute("codeSystemName"))),
                                value_num,
                                value_text,
                                unit,
                                ref_low,
                                ref_high,
                                abnormal_flag: abnormal_flag(obs),
                                panel_id,
                                effective,
                                category,
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    (out, stats)
}

/// Upsert one parsed C-CDA item onto an open connection/transaction.
async fn upsert_item(
    conn: &mut sqlx::PgConnection,
    subject_id: Uuid,
    source_id: Uuid,
    ext_id: &str,
    item: CItem,
) -> Result<(), sqlx::Error> {
    match item {
        CItem::Allergy { substance, code, code_system, reaction, reaction_code, reaction_code_system, criticality, severity } => {
            sqlx::query(&format!(
                "insert into allergies (id, subject_id, substance, code, code_system, reaction, reaction_code, reaction_code_system, criticality, severity, source_id, external_id)
                 values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12){ON_CONFLICT}
                    substance=excluded.substance, code=excluded.code, code_system=excluded.code_system,
                    reaction=excluded.reaction, reaction_code=excluded.reaction_code,
                    reaction_code_system=excluded.reaction_code_system, criticality=excluded.criticality,
                    severity=excluded.severity, updated_at=now()"
            ))
            .bind(Uuid::now_v7()).bind(subject_id).bind(&substance).bind(&code).bind(&code_system)
            .bind(&reaction).bind(&reaction_code).bind(&reaction_code_system).bind(&criticality).bind(&severity)
            .bind(source_id).bind(ext_id).execute(&mut *conn).await?;
        }
        CItem::Medication { name, code, code_system, dose, route, status, started } => {
            // medications.status is NOT NULL (default 'active'); fall back rather than NULL.
            let status = status.unwrap_or_else(|| "active".to_string());
            sqlx::query(&format!(
                "insert into medications (id, subject_id, name, code, code_system, dose, route, status, started_on, source_id, external_id)
                 values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11){ON_CONFLICT}
                    name=excluded.name, code=excluded.code, code_system=excluded.code_system,
                    dose=coalesce(excluded.dose, medications.dose),
                    route=coalesce(excluded.route, medications.route),
                    status=excluded.status,
                    started_on=coalesce(excluded.started_on, medications.started_on), updated_at=now()"
            ))
            .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code).bind(&code_system)
            .bind(&dose).bind(&route).bind(&status).bind(started)
            .bind(source_id).bind(ext_id).execute(&mut *conn).await?;
        }
        CItem::Condition { name, code, code_system, status, onset, resolved } => {
            // conditions.status is NOT NULL (default 'active'); Epic omits the
            // status observation on some problems, so fall back rather than NULL.
            let status = status.unwrap_or_else(|| "active".to_string());
            // Dedup chronic problems by code: the same problem recurs across
            // visit documents with a different HL7 entry-id, so when it's coded,
            // key on (subject, code, code_system) and update in place rather than
            // insert a duplicate.
            if let Some(code) = &code {
                let existing: Option<Uuid> = sqlx::query_scalar(
                    "select id from conditions where subject_id=$1 and code=$2
                       and code_system is not distinct from $3 limit 1",
                )
                .bind(subject_id).bind(code).bind(&code_system)
                .fetch_optional(&mut *conn).await?;
                if let Some(id) = existing {
                    sqlx::query(
                        "update conditions set name=$2, status=$3,
                           onset_date=coalesce($4, onset_date),
                           resolved_date=coalesce($5, resolved_date), updated_at=now() where id=$1",
                    )
                    .bind(id).bind(&name).bind(&status).bind(onset).bind(resolved)
                    .execute(&mut *conn).await?;
                    return Ok(());
                }
            }
            sqlx::query(&format!(
                "insert into conditions (id, subject_id, name, code, code_system, status, onset_date, resolved_date, source_id, external_id)
                 values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10){ON_CONFLICT}
                    name=excluded.name, code=excluded.code, code_system=excluded.code_system,
                    status=excluded.status, onset_date=excluded.onset_date,
                    resolved_date=excluded.resolved_date, updated_at=now()"
            ))
            .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code).bind(&code_system)
            .bind(&status).bind(onset).bind(resolved).bind(source_id).bind(ext_id).execute(&mut *conn).await?;
        }
        CItem::Incident { title, occurred } => {
            // Incidents carry no provenance (source_id/external_id) by schema
            // design, so dedup on content: same subject + title + date = same
            // event, idempotent across re-imports.
            let existing: Option<Uuid> = sqlx::query_scalar(
                "select id from incidents where subject_id=$1 and lower(title)=lower($2)
                   and occurred_at is not distinct from $3 limit 1",
            )
            .bind(subject_id).bind(&title).bind(occurred)
            .fetch_optional(&mut *conn).await?;
            if existing.is_none() {
                sqlx::query(
                    "insert into incidents (id, subject_id, title, narrative, occurred_at, occurred_precision)
                     values ($1,$2,$3,'',$4,'day')",
                )
                .bind(Uuid::now_v7()).bind(subject_id).bind(&title).bind(occurred)
                .execute(&mut *conn).await?;
            }
        }
        CItem::Immunization { vaccine, code, code_system, occurred, dose_number, lot_number, site, route } => {
            // Cross-source dedup (PEMR-48): same subject + date + vaccine family →
            // merge richer fields (lot/site/route/dose) into the existing row.
            if let Some(existing_id) = crate::dedupe::find_immunization_match(
                &mut *conn,
                subject_id,
                &vaccine,
                code.as_deref(),
                occurred,
            )
            .await?
            {
                crate::dedupe::merge_immunization(
                    &mut *conn,
                    existing_id,
                    &vaccine,
                    code.as_deref(),
                    code_system.as_deref(),
                    occurred,
                    dose_number,
                    lot_number.as_deref(),
                    site.as_deref(),
                    route.as_deref(),
                )
                .await?;
            } else {
                sqlx::query(&format!(
                    "insert into immunizations (id, subject_id, vaccine, code, code_system, occurred_at, dose_number, lot_number, site, route, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12){ON_CONFLICT}
                        vaccine=excluded.vaccine, code=excluded.code, code_system=excluded.code_system,
                        occurred_at=excluded.occurred_at,
                        dose_number=coalesce(excluded.dose_number, immunizations.dose_number),
                        lot_number=coalesce(excluded.lot_number, immunizations.lot_number),
                        site=coalesce(excluded.site, immunizations.site),
                        route=coalesce(excluded.route, immunizations.route), updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&vaccine).bind(&code).bind(&code_system)
                .bind(occurred).bind(dose_number).bind(&lot_number).bind(&site).bind(&route)
                .bind(source_id).bind(ext_id).execute(&mut *conn).await?;
            }
        }
        CItem::Observation {
            display, code, code_system, value_num, value_text, unit,
            ref_low, ref_high, abnormal_flag, panel_id, effective, category,
        } => {
            // Cross-source dedup (PEMR-48): same measurement (LOINC code or
            // lower display) on the same effective date → merge richer fields
            // (numeric value, unit, reference range, abnormal flag) in place.
            if let Some(existing_id) = crate::dedupe::find_observation_match(
                &mut *conn, subject_id, code.as_deref(), &display, effective,
            )
            .await?
            {
                crate::dedupe::merge_observation(
                    &mut *conn,
                    existing_id,
                    category,
                    code.as_deref(),
                    code_system.as_deref(),
                    &display,
                    value_num,
                    value_text.as_deref(),
                    unit.as_deref(),
                    ref_low,
                    ref_high,
                    abnormal_flag.as_deref(),
                    panel_id.as_ref(),
                    effective,
                )
                .await?;
            } else {
                sqlx::query(&format!(
                    "insert into observations (id, subject_id, category, code, code_system, display, value_num, value_text, unit, ref_low, ref_high, abnormal_flag, panel_id, effective_on, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16){ON_CONFLICT}
                        category=excluded.category, code=excluded.code, code_system=excluded.code_system, display=excluded.display,
                        value_num=excluded.value_num, value_text=excluded.value_text, unit=excluded.unit,
                        ref_low=excluded.ref_low, ref_high=excluded.ref_high, abnormal_flag=excluded.abnormal_flag,
                        panel_id=excluded.panel_id, effective_on=excluded.effective_on, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(category).bind(&code).bind(&code_system).bind(&display)
                .bind(value_num).bind(&value_text).bind(&unit).bind(ref_low).bind(ref_high).bind(&abnormal_flag)
                .bind(panel_id).bind(effective).bind(source_id).bind(ext_id).execute(&mut *conn).await?;
            }
        }
    }
    Ok(())
}

fn tally(c: &mut Counts, item: &CItem) {
    match item {
        CItem::Allergy { .. } => c.allergies += 1,
        CItem::Medication { .. } => c.medications += 1,
        CItem::Condition { .. } => c.conditions += 1,
        CItem::Incident { .. } => c.incidents += 1,
        CItem::Immunization { .. } => c.immunizations += 1,
        CItem::Observation { .. } => c.observations += 1,
    }
}

fn category_label(item: &CItem) -> &'static str {
    match item {
        CItem::Allergy { .. } => "allergies",
        CItem::Medication { .. } => "medications",
        CItem::Condition { .. } => "conditions",
        CItem::Incident { .. } => "incidents",
        CItem::Immunization { .. } => "immunizations",
        CItem::Observation { category, .. } => {
            if *category == "vital" { "vitals" } else { "labs" }
        }
    }
}

/// Parse + dedup a C-CDA package, returning the deduped items (by provenance id)
/// and fidelity warnings — low-fidelity signals (parse failures, a category that
/// came through empty despite being present, dropped entries) an importer should
/// react to. Empty warnings == high fidelity.
fn collect_ccda(xmls: &[String]) -> (HashMap<String, CItem>, Vec<String>) {
    let mut items: HashMap<String, CItem> = HashMap::new();
    let mut sections: std::collections::HashSet<&'static str> = Default::default();
    let mut docs_failed = 0usize;
    let mut skipped = 0i64;
    for xml in xmls {
        let (its, st) = parse_ccda(xml);
        if st.parse_failed {
            docs_failed += 1;
        }
        sections.extend(st.sections);
        // max (not sum): the same dropped entry recurs across visit summaries, so
        // the richest document's count approximates the unique drop count.
        skipped = skipped.max(st.skipped);
        for (id, item) in its {
            items.insert(id, item);
        }
    }

    let mut per: HashMap<&'static str, usize> = HashMap::new();
    for item in items.values() {
        *per.entry(category_label(item)).or_default() += 1;
    }

    let mut warnings = Vec::new();
    if docs_failed > 0 {
        warnings.push(format!(
            "{docs_failed} document(s) could not be parsed as C-CDA and were skipped"
        ));
    }
    for label in ["allergies", "medications", "conditions", "immunizations", "labs", "vitals"] {
        if sections.contains(label) && per.get(label).copied().unwrap_or(0) == 0 {
            warnings.push(format!(
                "{label}: present in the source but no structured entries imported \
                 (narrative-only or unsupported encoding)"
            ));
        }
    }
    if skipped > 0 {
        warnings.push(format!("~{skipped} entr(ies) skipped for a missing name or date"));
    }
    (items, warnings)
}

/// Import a single C-CDA document for `subject_id`, attributing rows to `source_id`.
pub async fn import_ccda(
    pool: &PgPool,
    subject_id: Uuid,
    source_id: Uuid,
    xml: &str,
) -> Result<Counts, sqlx::Error> {
    let docs = [xml.to_string()];
    import_ccda_docs(pool, subject_id, source_id, &docs).await
}

/// Import a whole C-CDA package (e.g. every `DOC*.XML` in an IHE_XDM bundle).
/// Items are deduped on their provenance id across documents before upsert, so
/// the same allergy/problem appearing in 30 visit summaries lands once; distinct
/// per-visit results are all kept. The whole package commits atomically.
pub async fn import_ccda_docs(
    pool: &PgPool,
    subject_id: Uuid,
    source_id: Uuid,
    xmls: &[String],
) -> Result<Counts, sqlx::Error> {
    let (items, warnings) = collect_ccda(xmls);
    let mut c = Counts { warnings, ..Default::default() };
    let mut tx = pool.begin().await?;
    for (ext_id, item) in items {
        tally(&mut c, &item);
        upsert_item(&mut tx, subject_id, source_id, &ext_id, item).await?;
    }
    tx.commit().await?;
    Ok(c)
}

/// Dry-run summary of what a set of C-CDA documents would import (no DB writes).
#[derive(Debug, Default, serde::Serialize)]
pub struct Preview {
    pub counts: Counts,
    pub labs: i64,
    pub vitals: i64,
    pub samples: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn preview_ccda_docs(xmls: &[String]) -> Preview {
    let (items, warnings) = collect_ccda(xmls);
    let mut p = Preview { warnings, ..Default::default() };
    for item in items.values() {
        preview_push(&mut p, item);
    }
    p
}

/// Tally one item into a `Preview` and append a human-readable sample line
/// (capped at 60). Shared by the C-CDA and EHI preview paths.
fn preview_push(p: &mut Preview, item: &CItem) {
    tally(&mut p.counts, item);
    let sample = match item {
        CItem::Allergy { substance, criticality, severity, reaction, .. } => format!(
            "allergy    {substance}{}{}{}",
            criticality.as_deref().map(|c| format!(" [crit:{c}]")).unwrap_or_default(),
            severity.as_deref().map(|s| format!(" [sev:{s}]")).unwrap_or_default(),
            reaction.as_deref().map(|r| format!(" → {r}")).unwrap_or_default(),
        ),
        CItem::Medication { name, dose, route, .. } => format!(
            "med        {name}{}{}",
            dose.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default(),
            route.as_deref().map(|r| format!(" {r}")).unwrap_or_default(),
        ),
        CItem::Condition { name, status, onset, resolved, .. } => format!(
            "condition  {name}{}{}{}",
            status.as_deref().map(|s| format!(" [{s}]")).unwrap_or_default(),
            onset.map(|d| format!(" ({d}")).unwrap_or_default(),
            resolved.map(|d| format!("→{d})")).or(onset.map(|_| ")".into())).unwrap_or_default(),
        ),
        CItem::Incident { title, occurred } => format!(
            "incident   {title}{}",
            occurred.map(|d| format!(" ({d})")).unwrap_or_default(),
        ),
        CItem::Immunization { vaccine, occurred, lot_number, site, .. } => format!(
            "immun      {vaccine}{}{}{}",
            occurred.map(|d| format!(" ({d})")).unwrap_or_default(),
            site.as_deref().map(|s| format!(" @{s}")).unwrap_or_default(),
            lot_number.as_deref().map(|l| format!(" lot {l}")).unwrap_or_default(),
        ),
        CItem::Observation { display, value_num, value_text, unit, category, effective, .. } => {
            if *category == "vital" {
                p.vitals += 1
            } else {
                p.labs += 1
            }
            let val = value_num
                .map(|v| v.to_string())
                .or_else(|| value_text.clone())
                .unwrap_or_default();
            format!("{category:<6}     {display} = {val} {} @{effective}", unit.as_deref().unwrap_or(""))
        }
    };
    if p.samples.len() < 60 {
        p.samples.push(sample);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Epic EHI export (TSV) — the "EHI Export" / "Requested Records" download.
//
// NOT C-CDA and NOT FHIR: an Epic EHI export is ~3,900 tab-separated Chronicles
// tables (`EHITables/*.tsv`) plus a per-table HTML schema. Almost all are audit
// or billing noise; the clinical payload is a handful of well-known tables. We
// read those and map each row to a `CItem`, reusing the same upsert + dedup core
// as the C-CDA path.
//
// Hard-won facts about the real Epic EHI format (validated against a pediatric
// export — do not regress):
//   - It is NAME-DENORMALIZED: the human-readable value is in `*_ID_*_NAME` /
//     `*_C_NAME` columns; the raw *code* is omitted almost everywhere. There are
//     NO CVX / ICD-10 / LOINC / SNOMED / CPT codes with data. The only real
//     machine code present is NDC (on immunizations). So we map on names, assign
//     canonical growth LOINCs ourselves, and carry NDC where present.
//   - Flowsheet VALUES are NOT in `IP_FLWSHT_MEAS`; they live in the companion
//     `V_EHI_FLO_MEAS_VALUE` (which — despite the `V_EHI_` prefix — is a data
//     table, not an audit trail), joined on `(FSD_ID, LINE)`.
//   - Vaccine administrations appear BOTH in `IMMUNE` and as `ORDER_MED` orders;
//     `IMMUNE` is authoritative, so vaccine med-orders are skipped.
//   - Allergies use `ALLERGY_FLAG` (Y = positively No Known Allergies) with an
//     empty `ALLERGY` table — a clinical fact, not "no data".
//
// Subject is caller-specified (an EHI export identifies the patient only by an
// internal id — never auto-detect). Provenance key is `ehi_{table}_{row-id}`, so
// re-import is idempotent on (source_id, external_id).
// ─────────────────────────────────────────────────────────────────────────────

/// The `EHITables/` directory given an export root — or the dir itself if it is
/// already `EHITables/`. None if neither exists (i.e. not an EHI export).
pub fn ehi_tables_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() && path.file_name().is_some_and(|n| n == "EHITables") {
        return Some(path.to_path_buf());
    }
    let nested = path.join("EHITables");
    nested.is_dir().then_some(nested)
}

/// A parsed EHI `.tsv`: a header→column-index map + the data rows. Columns are
/// keyed by name so the mapping code reads against the schema docs.
struct Tsv {
    idx: HashMap<String, usize>,
    rows: Vec<Vec<String>>,
}

impl Tsv {
    /// Open `<tables>/<name>.tsv`. None if missing, unreadable, or headers-only.
    fn open(tables: &Path, name: &str) -> Option<Tsv> {
        let text = std::fs::read_to_string(tables.join(format!("{name}.tsv"))).ok()?;
        let mut lines = text.lines();
        let header = lines.next()?;
        let idx: HashMap<String, usize> = header
            .split('\t')
            .enumerate()
            .map(|(i, h)| (h.to_string(), i))
            .collect();
        let rows: Vec<Vec<String>> = lines
            .filter(|l| !l.is_empty())
            .map(|l| l.split('\t').map(str::to_string).collect())
            .collect();
        (!rows.is_empty()).then_some(Tsv { idx, rows })
    }

    /// Trimmed, non-empty value at (row, column-name). None for absent/blank cells.
    fn get<'a>(&self, row: &'a [String], col: &str) -> Option<&'a str> {
        let v = row.get(*self.idx.get(col)?)?.trim();
        (!v.is_empty()).then_some(v)
    }
}

/// Parse an Epic EHI timestamp (`M/D/YYYY` or `M/D/YYYY h:mm:ss AM`) → `Date`.
fn ehi_date(s: &str) -> Option<Date> {
    let d = s.split_whitespace().next()?;
    let fmt = time::macros::format_description!("[month padding:none]/[day padding:none]/[year]");
    Date::parse(d, fmt).ok()
}

fn oz_to_kg(oz: f64) -> f64 {
    (oz * 0.028349523 * 1000.0).round() / 1000.0 // gram precision
}
fn in_to_cm(inch: f64) -> f64 {
    (inch * 2.54 * 10.0).round() / 10.0 // 0.1 cm precision
}

fn map_immunizations(tables: &Path, out: &mut Vec<(String, CItem)>) {
    let Some(t) = Tsv::open(tables, "IMMUNE") else { return };
    for row in &t.rows {
        let Some(vaccine) = t.get(row, "IMMUNZATN_ID_NAME") else { continue };
        let ext_id = format!("ehi_immune_{}", t.get(row, "IMMUNE_ID").unwrap_or(vaccine));
        // NDC is the only real machine code Epic exports for vaccines.
        let ndc = t.get(row, "NDC_NUM_ID_NDC_CODE");
        out.push((
            ext_id,
            CItem::Immunization {
                vaccine: vaccine.to_string(),
                code: ndc.map(str::to_string),
                code_system: ndc.map(|_| "NDC".to_string()),
                occurred: t.get(row, "IMMUNE_DATE").and_then(ehi_date),
                dose_number: None, // IMMUNE carries dose *amount* (".5 mL"), not sequence
                lot_number: t.get(row, "LOT").map(str::to_string),
                site: t.get(row, "SITE_C_NAME").map(str::to_string),
                route: t.get(row, "ROUTE_C_NAME").map(str::to_string),
            },
        ));
    }
}

/// Growth vitals. The measured value lives in `V_EHI_FLO_MEAS_VALUE` (joined on
/// `(FSD_ID, LINE)`), NOT in `IP_FLWSHT_MEAS`. We whitelist the real growth
/// measures, assign canonical growth LOINCs, and convert to the metric units the
/// growth charts expect (kg / cm — see `handlers::subjects::growth`).
fn map_vitals(tables: &Path, out: &mut Vec<(String, CItem)>) {
    let (Some(meas), Some(vals)) =
        (Tsv::open(tables, "IP_FLWSHT_MEAS"), Tsv::open(tables, "V_EHI_FLO_MEAS_VALUE"))
    else {
        return;
    };
    // (FSD_ID, LINE) → (raw value, source unit).
    let mut value_of: HashMap<(String, String), (String, Option<String>)> = HashMap::new();
    for row in &vals.rows {
        if let (Some(fsd), Some(line), Some(v)) =
            (vals.get(row, "FSD_ID"), vals.get(row, "LINE"), vals.get(row, "MEAS_VALUE_EXTERNAL"))
        {
            value_of.insert(
                (fsd.to_string(), line.to_string()),
                (v.to_string(), vals.get(row, "UNITS").map(str::to_string)),
            );
        }
    }
    for row in &meas.rows {
        let Some(name) = meas.get(row, "FLO_MEAS_ID_FLO_MEAS_NAME") else { continue };
        // Whitelist the real growth vitals; the ~40 other measure names are
        // flowsheet-template formula rows (BMI, BSA, bariatric/frailty forms).
        let (display, code, conv, out_unit): (&str, &str, Option<fn(f64) -> f64>, Option<&str>) =
            match name.to_ascii_uppercase().as_str() {
                "WEIGHT/SCALE" | "WEIGHT" => ("Body weight", "29463-7", Some(oz_to_kg), Some("kg")),
                "HEIGHT" | "LENGTH" => ("Body length", "8302-2", Some(in_to_cm), Some("cm")),
                "HEAD CIRCUMFERENCE" => ("Head circumference", "9843-4", Some(in_to_cm), Some("cm")),
                "TEMPERATURE" => ("Body temperature", "8310-5", None, None),
                _ => continue,
            };
        let (Some(fsd), Some(line)) = (meas.get(row, "FSD_ID"), meas.get(row, "LINE")) else {
            continue;
        };
        let Some((raw, src_unit)) = value_of.get(&(fsd.to_string(), line.to_string())) else {
            continue;
        };
        let Some(effective) = meas.get(row, "RECORDED_TIME").and_then(ehi_date) else { continue };
        let parsed = raw.parse::<f64>().ok();
        let value_num = match conv {
            Some(f) => parsed.map(f),
            None => parsed,
        };
        out.push((
            format!("ehi_flo_{fsd}_{line}"),
            CItem::Observation {
                display: display.to_string(),
                code: Some(code.to_string()),
                code_system: Some("LOINC".to_string()),
                value_num,
                value_text: value_num.is_none().then(|| raw.clone()),
                unit: out_unit.map(str::to_string).or_else(|| src_unit.clone()),
                ref_low: None,
                ref_high: None,
                abnormal_flag: None,
                panel_id: None,
                effective,
                category: "vital",
            },
        ));
    }
}

fn map_problems(tables: &Path, out: &mut Vec<(String, CItem)>) {
    let Some(t) = Tsv::open(tables, "PROBLEM_LIST") else { return };
    for row in &t.rows {
        let Some(name) = t.get(row, "DX_ID_DX_NAME").or_else(|| t.get(row, "DESCRIPTION")) else {
            continue;
        };
        let name = name.to_string();
        let ext_id = format!("ehi_problem_{}", t.get(row, "PROBLEM_LIST_ID").unwrap_or(name.as_str()));
        let onset = t.get(row, "NOTED_DATE").and_then(ehi_date);
        // Epic files delivery/birth in the problem list — a birth is a real-world
        // event, not a chronic condition, so route it to an incident.
        if is_birth_event(&name, None) {
            out.push((ext_id, CItem::Incident { title: name, occurred: onset }));
            continue;
        }
        out.push((
            ext_id,
            CItem::Condition {
                name,
                code: None,
                code_system: None,
                status: t.get(row, "PROBLEM_STATUS_C_NAME").map(|s| s.to_lowercase()),
                onset,
                resolved: t.get(row, "RESOLVED_DATE").and_then(ehi_date),
            },
        ));
    }
}

/// Vaccine name tokens: `ORDER_MED` lists vaccine *administrations* as med orders
/// that duplicate the authoritative `IMMUNE` rows, so we skip them here.
fn looks_like_vaccine(name: &str) -> bool {
    const TOKENS: &[&str] = &[
        "vaccine", "rotateq", "rotavirus", "pentacel", "daptacel", "pediarix", "kinrix",
        "quadracel", "dtap", "tdap", "vaxneuvance", "prevnar", "pneumococcal", "vaqta",
        "havrix", "hepatitis a", "hepatitis b", "recombivax", "engerix", "varivax",
        "varicella", "proquad", "measles", "mumps", "rubella", "influenza", "fluzone",
        "flulaval", "fluarix", "m-m-r",
    ];
    let n = name.to_lowercase();
    TOKENS.iter().any(|t| n.contains(t))
}

fn map_medications(tables: &Path, out: &mut Vec<(String, CItem)>) {
    let Some(t) = Tsv::open(tables, "ORDER_MED") else { return };
    for row in &t.rows {
        let Some(name) = t
            .get(row, "DISPLAY_NAME")
            .or_else(|| t.get(row, "MEDICATION_ID_MEDICATION_NAME"))
            .or_else(|| t.get(row, "DESCRIPTION"))
        else {
            continue;
        };
        if looks_like_vaccine(name) {
            continue; // administered vaccine — captured via IMMUNE
        }
        let ext_id = format!("ehi_med_{}", t.get(row, "ORDER_MED_ID").unwrap_or(name));
        let status = t.get(row, "ORDER_STATUS_C_NAME").map(|s| {
            let s = s.to_lowercase();
            if s.contains("complet") {
                "completed"
            } else if s.contains("discontin") || s.contains("stop") {
                "stopped"
            } else if s.contains("hold") {
                "on_hold"
            } else {
                "active"
            }
            .to_string()
        });
        out.push((
            ext_id,
            CItem::Medication {
                name: name.to_string(),
                code: None,
                code_system: None,
                dose: t.get(row, "DOSAGE").or_else(|| t.get(row, "HV_DISCRETE_DOSE")).map(str::to_string),
                route: t.get(row, "MED_ROUTE_C_NAME").map(str::to_string),
                status,
                started: t.get(row, "START_DATE").or_else(|| t.get(row, "ORDERING_DATE")).and_then(ehi_date),
            },
        ));
    }
}

/// True if the export positively asserts No Known Allergies (`ALLERGY_FLAG` Y).
fn ehi_nkda(tables: &Path) -> bool {
    let Some(t) = Tsv::open(tables, "ALLERGY_FLAG") else { return false };
    t.rows.iter().any(|r| t.get(r, "ALRGY_FLAG_YN") == Some("Y"))
}

fn collect_ehi(tables: &Path) -> (Vec<(String, CItem)>, Vec<String>) {
    let mut out = Vec::new();
    map_immunizations(tables, &mut out);
    map_problems(tables, &mut out);
    map_medications(tables, &mut out);
    map_vitals(tables, &mut out);

    // Surface categories present in the export but out of the v1 parser's scope,
    // so partial coverage isn't mistaken for a clean, complete import.
    let mut warnings = Vec::new();
    if let Some(t) = Tsv::open(tables, "PAT_ENC_DX") {
        warnings.push(format!(
            "{} encounter diagnoses (PAT_ENC_DX) not imported — v1 maps the problem list only \
             (this can include acute events, e.g. a fracture)",
            t.rows.len()
        ));
    }
    if let Some(t) = Tsv::open(tables, "HNO_INFO") {
        warnings.push(format!(
            "{} clinical notes (RTF) not imported — v1 imports structured data only",
            t.rows.len()
        ));
    }
    if Tsv::open(tables, "ORDER_RESULTS").is_none() {
        warnings.push(
            "no structured lab results in this export (ORDER_RESULTS empty) — any lab values are \
             narrative-only in the RTF notes"
                .to_string(),
        );
    }
    if out.is_empty() {
        warnings.push("no clinical rows mapped — check the EHITables path".to_string());
    }
    (out, warnings)
}

/// Import an Epic EHI export directory for `subject_id`, attributing rows to
/// `source_id`. Commits atomically. Subject is caller-specified (never detected).
pub async fn import_ehi(
    pool: &PgPool,
    subject_id: Uuid,
    source_id: Uuid,
    export_dir: &Path,
) -> Result<Counts, sqlx::Error> {
    let Some(tables) = ehi_tables_dir(export_dir) else {
        let mut c = Counts::default();
        c.warnings.push(format!("no EHITables/ directory under {}", export_dir.display()));
        return Ok(c);
    };
    let (items, warnings) = collect_ehi(&tables);
    let mut c = Counts { warnings, ..Default::default() };
    let mut tx = pool.begin().await?;
    for (ext_id, item) in items {
        tally(&mut c, &item);
        upsert_item(&mut tx, subject_id, source_id, &ext_id, item).await?;
    }
    // No Known Allergies is a positive assertion on the subject, not a row.
    if ehi_nkda(&tables) {
        sqlx::query("update subjects set no_known_allergies = true where id = $1")
            .bind(subject_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(c)
}

/// Dry-run summary of what an Epic EHI export would import (no DB writes).
pub fn preview_ehi(export_dir: &Path) -> Preview {
    let Some(tables) = ehi_tables_dir(export_dir) else {
        let mut p = Preview::default();
        p.warnings.push(format!("no EHITables/ directory under {}", export_dir.display()));
        return p;
    };
    let (items, warnings) = collect_ehi(&tables);
    let mut p = Preview { warnings, ..Default::default() };
    for (_ext, item) in &items {
        preview_push(&mut p, item);
    }
    if ehi_nkda(&tables) {
        p.samples.push("allergy    NKDA — No Known Allergies asserted (sets subject flag)".to_string());
    }
    p
}

/// Look up a source by case-insensitive name, creating it (`kind='other'`) if
/// absent. Shared by the offline CLI and the web upload handler.
pub async fn ensure_source(pool: &PgPool, name: &str) -> Result<Uuid, sqlx::Error> {
    if let Some(id) =
        sqlx::query_scalar::<_, Uuid>("select id from sources where lower(name) = lower($1)")
            .bind(name)
            .fetch_optional(pool)
            .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    sqlx::query("insert into sources (id, name, kind, notes) values ($1,$2,'other',$3)")
        .bind(id)
        .bind(name)
        .bind("Auto-created by import.")
        .execute(pool)
        .await?;
    Ok(id)
}
