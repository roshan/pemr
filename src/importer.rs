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
use time::Date;
use uuid::Uuid;

#[derive(Debug, Default, serde::Serialize)]
pub struct Counts {
    pub allergies: i64,
    pub medications: i64,
    pub conditions: i64,
    pub immunizations: i64,
    pub observations: i64,
    pub skipped: i64,
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
                let status = match clinical_status(r).as_deref() {
                    Some("resolved") => "resolved",
                    Some("remission") => "remission",
                    _ => "active",
                };
                let onset = str_at(r, "onsetDateTime").and_then(fhir_date);
                sqlx::query(&format!(
                    "insert into conditions (id, subject_id, name, code, code_system, status, onset_date, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8,$9){ON_CONFLICT}
                        name=excluded.name, code=excluded.code, code_system=excluded.code_system,
                        status=excluded.status, onset_date=excluded.onset_date, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code)
                .bind(code_system_label(system.as_deref(), &code)).bind(status).bind(onset)
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
        severity: Option<String>,
    },
    Medication { name: String, code: Option<String>, code_system: Option<String> },
    Condition {
        name: String,
        code: Option<String>,
        code_system: Option<String>,
        status: Option<String>,
        onset: Option<Date>,
    },
    Immunization {
        vaccine: String,
        code: Option<String>,
        code_system: Option<String>,
        occurred: Option<Date>,
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

/// Resolve a node's own `<text><reference value="#id"/>` into the narrative.
fn text_ref(node: Node<'_, '_>, idmap: &HashMap<String, String>) -> Option<String> {
    let r = child(node, "text")
        .and_then(|t| child(t, "reference"))
        .and_then(|r| r.attribute("value"))?;
    idmap
        .get(r.trim_start_matches('#'))
        .filter(|v| !v.is_empty())
        .cloned()
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

/// Parse a C-CDA document into normalized items keyed by their provenance id.
/// Sync; returns owned data (the roxmltree DOM is not held across an await).
fn parse_ccda(xml: &str) -> Vec<(String, CItem)> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let idmap = narrative_map(&doc);
    let mut out: Vec<(String, CItem)> = Vec::new();

    for section in doc.descendants().filter(|n| n.tag_name().name() == "section") {
        let sec_code = child(section, "code")
            .and_then(|c| c.attribute("code"))
            .unwrap_or("");
        match sec_code {
            // Allergies — name lives in participant/playingEntity/name (the coded
            // value is usually nullFlavor); reaction + criticality hang off it.
            "48765-2" => {
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
                        None => continue,
                    };
                    let code_node = pe.and_then(|pe| child(pe, "code"));
                    let reaction = obs
                        .descendants()
                        .find(|n| {
                            n.tag_name().name() == "entryRelationship"
                                && n.attribute("typeCode") == Some("MFST")
                        })
                        .and_then(|er| descendants_named(er, "observation").next())
                        .and_then(|o| {
                            // The narrative reaction ("Asthma") beats the coarse
                            // SNOMED value displayName ("Other").
                            text_ref(o, &idmap)
                                .or_else(|| child(o, "value").and_then(|v| resolve_display(v, &idmap)))
                        });
                    let severity = obs
                        .descendants()
                        .find(|n| {
                            n.tag_name().name() == "observation"
                                && child(*n, "code").and_then(|c| c.attribute("code"))
                                    == Some("82606-5")
                        })
                        .and_then(|o| child(o, "value"))
                        .and_then(|v| v.attribute("displayName"))
                        .map(|s| s.replace(" criticality", "").trim().to_string());
                    let ext_id = entry_id(obs).unwrap_or_else(|| hid("allergy", &substance));
                    out.push((
                        ext_id,
                        CItem::Allergy {
                            substance,
                            code: code_node.and_then(|c| c.attribute("code")).map(str::to_string),
                            code_system: code_node
                                .and_then(|c| norm_system(c.attribute("codeSystemName"))),
                            reaction,
                            severity,
                        },
                    ));
                }
            }
            // Medications
            "10160-0" => {
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
                        None => continue,
                    };
                    let ext_id = entry_id(sa).unwrap_or_else(|| hid("med", &name));
                    out.push((
                        ext_id,
                        CItem::Medication {
                            name,
                            code: code_node.and_then(|c| c.attribute("code")).map(str::to_string),
                            code_system: code_node
                                .and_then(|c| norm_system(c.attribute("codeSystemName"))),
                        },
                    ));
                }
            }
            // Problem list — skip the nested Status observation; the problem name
            // is referenced into the narrative, not on value/@displayName.
            "11450-4" => {
                for obs in descendants_named(section, "observation") {
                    if !has_template(obs, T_PROBLEM_OBS) {
                        continue;
                    }
                    let value = child(obs, "value");
                    let name = match value.and_then(|v| resolve_display(v, &idmap)) {
                        Some(n) => n,
                        None => continue,
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
                    out.push((
                        ext_id,
                        CItem::Condition {
                            name,
                            code: value.and_then(|v| v.attribute("code")).map(str::to_string),
                            code_system: value
                                .and_then(|v| norm_system(v.attribute("codeSystemName"))),
                            status: condition_status(obs),
                            onset,
                        },
                    ));
                }
            }
            // Immunizations — CVX code present, name via originalText/narrative.
            "11369-6" => {
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
                        None => continue,
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
                        },
                    ));
                }
            }
            // Results (labs) + Vital signs — analytes are component observations
            // under an organizer; name in originalText, value/unit in attributes,
            // bounds in referenceRange. Grouped by panel (the organizer).
            "30954-2" | "8716-3" => {
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
                            None => continue,
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
                            None => continue,
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
    out
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
        CItem::Allergy { substance, code, code_system, reaction, severity } => {
            sqlx::query(&format!(
                "insert into allergies (id, subject_id, substance, code, code_system, reaction, severity, source_id, external_id)
                 values ($1,$2,$3,$4,$5,$6,$7,$8,$9){ON_CONFLICT}
                    substance=excluded.substance, code=excluded.code, code_system=excluded.code_system,
                    reaction=excluded.reaction, severity=excluded.severity, updated_at=now()"
            ))
            .bind(Uuid::now_v7()).bind(subject_id).bind(&substance).bind(&code).bind(&code_system)
            .bind(&reaction).bind(&severity).bind(source_id).bind(ext_id).execute(&mut *conn).await?;
        }
        CItem::Medication { name, code, code_system } => {
            sqlx::query(&format!(
                "insert into medications (id, subject_id, name, code, code_system, source_id, external_id)
                 values ($1,$2,$3,$4,$5,$6,$7){ON_CONFLICT}
                    name=excluded.name, code=excluded.code, code_system=excluded.code_system, updated_at=now()"
            ))
            .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code).bind(&code_system)
            .bind(source_id).bind(ext_id).execute(&mut *conn).await?;
        }
        CItem::Condition { name, code, code_system, status, onset } => {
            // conditions.status is NOT NULL (default 'active'); Epic omits the
            // status observation on some problems, so fall back rather than NULL.
            let status = status.unwrap_or_else(|| "active".to_string());
            sqlx::query(&format!(
                "insert into conditions (id, subject_id, name, code, code_system, status, onset_date, source_id, external_id)
                 values ($1,$2,$3,$4,$5,$6,$7,$8,$9){ON_CONFLICT}
                    name=excluded.name, code=excluded.code, code_system=excluded.code_system,
                    status=excluded.status, onset_date=excluded.onset_date, updated_at=now()"
            ))
            .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code).bind(&code_system)
            .bind(&status).bind(onset).bind(source_id).bind(ext_id).execute(&mut *conn).await?;
        }
        CItem::Immunization { vaccine, code, code_system, occurred } => {
            sqlx::query(&format!(
                "insert into immunizations (id, subject_id, vaccine, code, code_system, occurred_at, source_id, external_id)
                 values ($1,$2,$3,$4,$5,$6,$7,$8){ON_CONFLICT}
                    vaccine=excluded.vaccine, code=excluded.code, code_system=excluded.code_system,
                    occurred_at=excluded.occurred_at, updated_at=now()"
            ))
            .bind(Uuid::now_v7()).bind(subject_id).bind(&vaccine).bind(&code).bind(&code_system)
            .bind(occurred).bind(source_id).bind(ext_id).execute(&mut *conn).await?;
        }
        CItem::Observation {
            display, code, code_system, value_num, value_text, unit,
            ref_low, ref_high, abnormal_flag, panel_id, effective, category,
        } => {
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
    Ok(())
}

fn tally(c: &mut Counts, item: &CItem) {
    match item {
        CItem::Allergy { .. } => c.allergies += 1,
        CItem::Medication { .. } => c.medications += 1,
        CItem::Condition { .. } => c.conditions += 1,
        CItem::Immunization { .. } => c.immunizations += 1,
        CItem::Observation { .. } => c.observations += 1,
    }
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
    let mut items: HashMap<String, CItem> = HashMap::new();
    for xml in xmls {
        for (id, item) in parse_ccda(xml) {
            items.insert(id, item);
        }
    }
    let mut c = Counts::default();
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
}

pub fn preview_ccda_docs(xmls: &[String]) -> Preview {
    let mut items: HashMap<String, CItem> = HashMap::new();
    for xml in xmls {
        for (id, item) in parse_ccda(xml) {
            items.insert(id, item);
        }
    }
    let mut p = Preview::default();
    for item in items.values() {
        tally(&mut p.counts, item);
        let sample = match item {
            CItem::Allergy { substance, severity, reaction, .. } => format!(
                "allergy    {substance}{}{}",
                severity.as_deref().map(|s| format!(" [{s}]")).unwrap_or_default(),
                reaction.as_deref().map(|r| format!(" → {r}")).unwrap_or_default(),
            ),
            CItem::Medication { name, .. } => format!("med        {name}"),
            CItem::Condition { name, status, onset, .. } => format!(
                "condition  {name}{}{}",
                status.as_deref().map(|s| format!(" [{s}]")).unwrap_or_default(),
                onset.map(|d| format!(" ({d})")).unwrap_or_default(),
            ),
            CItem::Immunization { vaccine, occurred, .. } => format!(
                "immun      {vaccine}{}",
                occurred.map(|d| format!(" ({d})")).unwrap_or_default(),
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
    p
}
