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

use roxmltree::Node;

enum CItem {
    Allergy { substance: String, code: Option<String>, code_system: Option<String> },
    Medication { name: String, code: Option<String> },
    Condition { name: String, code: Option<String>, code_system: Option<String>, onset: Option<Date> },
    Immunization { vaccine: String, code: Option<String>, occurred: Option<Date> },
    Observation { display: String, code: Option<String>, value_num: Option<f64>, value_text: Option<String>, unit: Option<String>, effective: Date, category: &'static str },
}

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

fn hid(kind: &str, key: &str) -> String {
    crate::api_auth::sha256_hex(format!("{kind}|{key}").as_bytes())
}

/// Parse a C-CDA document into normalized items. Sync; returns owned data.
fn parse_ccda(xml: &str) -> Vec<(String, CItem)> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<(String, CItem)> = Vec::new();

    for section in doc.descendants().filter(|n| n.tag_name().name() == "section") {
        let sec_code = child(section, "code").and_then(|c| c.attribute("code")).unwrap_or("");
        match sec_code {
            // Allergies
            "48765-2" => {
                for pe in descendants_named(section, "playingEntity") {
                    if let Some(code) = child(pe, "code") {
                        if let Some(name) = code.attribute("displayName") {
                            out.push((
                                hid("allergy", name),
                                CItem::Allergy {
                                    substance: name.to_string(),
                                    code: code.attribute("code").map(str::to_string),
                                    code_system: code.attribute("codeSystemName").map(str::to_string),
                                },
                            ));
                        }
                    }
                }
            }
            // Medications
            "10160-0" => {
                for mm in descendants_named(section, "manufacturedMaterial") {
                    if let Some(code) = child(mm, "code") {
                        if let Some(name) = code.attribute("displayName") {
                            out.push((
                                hid("med", name),
                                CItem::Medication {
                                    name: name.to_string(),
                                    code: code.attribute("code").map(str::to_string),
                                },
                            ));
                        }
                    }
                }
            }
            // Problem list
            "11450-4" => {
                for obs in descendants_named(section, "observation") {
                    if let Some(value) = child(obs, "value") {
                        if let Some(name) = value.attribute("displayName") {
                            let onset = child(obs, "effectiveTime")
                                .and_then(|et| child(et, "low").and_then(|l| l.attribute("value")).or_else(|| et.attribute("value")))
                                .and_then(ccda_date);
                            out.push((
                                hid("cond", name),
                                CItem::Condition {
                                    name: name.to_string(),
                                    code: value.attribute("code").map(str::to_string),
                                    code_system: value.attribute("codeSystemName").map(str::to_string),
                                    onset,
                                },
                            ));
                        }
                    }
                }
            }
            // Immunizations
            "11369-6" => {
                for sa in descendants_named(section, "substanceAdministration") {
                    let occurred = child(sa, "effectiveTime")
                        .and_then(|et| et.attribute("value"))
                        .and_then(ccda_date);
                    if let Some(mm) = descendants_named(sa, "manufacturedMaterial").next() {
                        if let Some(code) = child(mm, "code") {
                            if let Some(name) = code.attribute("displayName") {
                                out.push((
                                    hid("imm", &format!("{name}|{occurred:?}")),
                                    CItem::Immunization {
                                        vaccine: name.to_string(),
                                        code: code.attribute("code").map(str::to_string),
                                        occurred,
                                    },
                                ));
                            }
                        }
                    }
                }
            }
            // Results (labs) + Vital signs
            "30954-2" | "8716-3" => {
                let category = if sec_code == "8716-3" { "vital" } else { "lab" };
                for obs in descendants_named(section, "observation") {
                    let code = child(obs, "code");
                    let display = code.and_then(|c| c.attribute("displayName"));
                    let display = match display {
                        Some(d) => d,
                        None => continue,
                    };
                    let effective = match child(obs, "effectiveTime")
                        .and_then(|et| et.attribute("value").or_else(|| child(et, "low").and_then(|l| l.attribute("value"))))
                        .and_then(ccda_date)
                    {
                        Some(d) => d,
                        None => continue,
                    };
                    let value = child(obs, "value");
                    let value_num = value.and_then(|v| v.attribute("value")).and_then(|s| s.parse::<f64>().ok());
                    let unit = value.and_then(|v| v.attribute("unit")).map(str::to_string);
                    let value_text = if value_num.is_none() {
                        value.and_then(|v| v.attribute("displayName").or_else(|| v.text())).map(str::to_string)
                    } else {
                        None
                    };
                    out.push((
                        hid("obs", &format!("{display}|{effective}")),
                        CItem::Observation {
                            display: display.to_string(),
                            code: code.and_then(|c| c.attribute("code")).map(str::to_string),
                            value_num,
                            value_text,
                            unit,
                            effective,
                            category,
                        },
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// Import a C-CDA document for `subject_id`, attributing rows to `source_id`.
pub async fn import_ccda(
    pool: &PgPool,
    subject_id: Uuid,
    source_id: Uuid,
    xml: &str,
) -> Result<Counts, sqlx::Error> {
    let items = parse_ccda(xml);
    let mut c = Counts::default();
    for (ext_id, item) in items {
        match item {
            CItem::Allergy { substance, code, code_system } => {
                sqlx::query(&format!(
                    "insert into allergies (id, subject_id, substance, code, code_system, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7){ON_CONFLICT}
                        substance=excluded.substance, code=excluded.code, code_system=excluded.code_system, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&substance).bind(&code).bind(&code_system)
                .bind(source_id).bind(&ext_id).execute(pool).await?;
                c.allergies += 1;
            }
            CItem::Medication { name, code } => {
                sqlx::query(&format!(
                    "insert into medications (id, subject_id, name, code, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6){ON_CONFLICT}
                        name=excluded.name, code=excluded.code, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code)
                .bind(source_id).bind(&ext_id).execute(pool).await?;
                c.medications += 1;
            }
            CItem::Condition { name, code, code_system, onset } => {
                sqlx::query(&format!(
                    "insert into conditions (id, subject_id, name, code, code_system, onset_date, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8){ON_CONFLICT}
                        name=excluded.name, code=excluded.code, code_system=excluded.code_system,
                        onset_date=excluded.onset_date, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&name).bind(&code).bind(&code_system).bind(onset)
                .bind(source_id).bind(&ext_id).execute(pool).await?;
                c.conditions += 1;
            }
            CItem::Immunization { vaccine, code, occurred } => {
                sqlx::query(&format!(
                    "insert into immunizations (id, subject_id, vaccine, code, occurred_at, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7){ON_CONFLICT}
                        vaccine=excluded.vaccine, code=excluded.code, occurred_at=excluded.occurred_at, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(&vaccine).bind(&code).bind(occurred)
                .bind(source_id).bind(&ext_id).execute(pool).await?;
                c.immunizations += 1;
            }
            CItem::Observation { display, code, value_num, value_text, unit, effective, category } => {
                sqlx::query(&format!(
                    "insert into observations (id, subject_id, category, code, display, value_num, value_text, unit, effective_on, source_id, external_id)
                     values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11){ON_CONFLICT}
                        category=excluded.category, code=excluded.code, display=excluded.display,
                        value_num=excluded.value_num, value_text=excluded.value_text, unit=excluded.unit,
                        effective_on=excluded.effective_on, updated_at=now()"
                ))
                .bind(Uuid::now_v7()).bind(subject_id).bind(category).bind(&code).bind(&display)
                .bind(value_num).bind(&value_text).bind(&unit).bind(effective)
                .bind(source_id).bind(&ext_id).execute(pool).await?;
                c.observations += 1;
            }
        }
    }
    Ok(c)
}
