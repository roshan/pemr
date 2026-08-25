//! Pediatric clinical computation (PEMR-25): immunization forecasting against a
//! vendored, **simplified routine** childhood schedule (ACIP-based recommended
//! ages, in months). This is decision *support*, NOT a substitute for the
//! pediatrician — catch-up rules, contraindications, and brand-specific series
//! (e.g. RV1 vs RV5) are out of scope. No external/runtime calls: the schedule
//! is a `const` table here.

use time::{Date, OffsetDateTime};

use crate::models::Immunization;

struct VaccineSchedule {
    family: &'static str,
    /// CVX codes that count toward this family.
    cvx: &'static [&'static str],
    /// Lowercase substrings of the vaccine display name that count (fallback).
    keywords: &'static [&'static str],
    /// Recommended age (months) for each dose, in order.
    doses_months: &'static [f64],
}

/// Simplified routine schedule (recommended ages, months). Clearly a
/// simplification — see module docs.
const SCHEDULE: &[VaccineSchedule] = &[
    VaccineSchedule { family: "Hepatitis B", cvx: &["08", "44", "45", "51"], keywords: &["hep b", "hepb", "hepatitis b"], doses_months: &[0.0, 1.0, 6.0] },
    VaccineSchedule { family: "Rotavirus", cvx: &["116", "119", "122"], keywords: &["rota"], doses_months: &[2.0, 4.0, 6.0] },
    VaccineSchedule { family: "DTaP", cvx: &["20", "106", "107", "110", "120", "146"], keywords: &["dtap", "dtap-"], doses_months: &[2.0, 4.0, 6.0, 15.0, 48.0] },
    VaccineSchedule { family: "Hib", cvx: &["17", "46", "47", "48", "49", "51", "120", "148"], keywords: &["hib", "haemophilus"], doses_months: &[2.0, 4.0, 6.0, 12.0] },
    VaccineSchedule { family: "Pneumococcal (PCV)", cvx: &["133", "152", "100"], keywords: &["pcv", "pneumo", "prevnar"], doses_months: &[2.0, 4.0, 6.0, 12.0] },
    VaccineSchedule { family: "Polio (IPV)", cvx: &["10", "110", "120", "146"], keywords: &["ipv", "polio"], doses_months: &[2.0, 4.0, 6.0, 48.0] },
    VaccineSchedule { family: "MMR", cvx: &["03", "94"], keywords: &["mmr", "measles"], doses_months: &[12.0, 48.0] },
    VaccineSchedule { family: "Varicella", cvx: &["21", "94"], keywords: &["varicella", "chickenpox"], doses_months: &[12.0, 48.0] },
    VaccineSchedule { family: "Hepatitis A", cvx: &["83", "85", "84"], keywords: &["hep a", "hepatitis a"], doses_months: &[12.0, 18.0] },
];

#[derive(Debug)]
pub struct DueItem {
    pub family: String,
    pub dose_number: i32,
    pub due_on: Date,
    /// "overdue" | "due" | "upcoming"
    pub status: &'static str,
}

const DAYS_PER_MONTH: f64 = 30.4375;
/// Grace window after the recommended date before a dose reads as "overdue".
const OVERDUE_GRACE_DAYS: i64 = 28;
/// The routine childhood/adolescent schedule applies through age 18. Adult
/// immunization schedules aren't modeled here, so forecasting the childhood
/// schedule for a grown-up just flags every long-past dose as "overdue" (noise).
/// Callers gate on this age.
const MAX_FORECAST_AGE_YEARS: f64 = 19.0;

/// Whether the routine childhood-schedule forecast is meaningful for someone
/// born on `dob` as of `today` — i.e. they're not yet an adult. Used to gate
/// both the forecast list and the chart's "N due" badge.
pub fn forecast_applies(dob: Date, today: Date) -> bool {
    let age_years = (today.to_julian_day() - dob.to_julian_day()) as f64 / 365.25;
    age_years < MAX_FORECAST_AGE_YEARS
}

/// All vaccine families an immunization row maps to, independent of the source's
/// code (CAIR keys by nothing, EHI by NDC, FHIR by CVX — so a raw `(code, date)`
/// key can't match the same shot across sources; PEMR-48). Matches by CVX code,
/// then by display-name keyword, mirroring [`VaccineSchedule`]. A combination
/// vaccine (DTaP-IPV/Hib) returns every family it counts toward, so the dedup
/// key is the family *set* — a source that lists a combo as one row (EHI) dedups
/// against a source that lists it per-disease-group (CAIR).
pub fn vaccine_families(code: Option<&str>, name: &str) -> Vec<&'static str> {
    SCHEDULE
        .iter()
        .filter(|s| matches_code_or_name(s, code, name))
        .map(|s| s.family)
        .collect()
}

fn matches_code_or_name(s: &VaccineSchedule, code: Option<&str>, name: &str) -> bool {
    if let Some(code) = code {
        let code = code.trim();
        if s.cvx.iter().any(|c| c.eq_ignore_ascii_case(code)) {
            return true;
        }
    }
    s.keywords.iter().any(|k| name.to_ascii_lowercase().contains(k))
}

fn matches(s: &VaccineSchedule, imm: &Immunization) -> bool {
    matches_code_or_name(s, imm.code.as_deref(), &imm.vaccine)
}

fn add_months(dob: Date, months: f64) -> Date {
    let days = (months * DAYS_PER_MONTH).round() as i32;
    Date::from_julian_day(dob.to_julian_day() + days).unwrap_or(dob)
}

/// One forecast item per vaccine family = its NEXT not-yet-received dose (or
/// none if the family's series is complete). `today` is injected so the caller
/// controls the clock.
pub fn forecast(dob: Date, imms: &[Immunization], today: Date) -> Vec<DueItem> {
    // Adults are past the routine childhood schedule — don't forecast, else every
    // childhood dose with no record reads as "overdue" for a grown-up.
    if !forecast_applies(dob, today) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for s in SCHEDULE {
        let received = imms
            .iter()
            .filter(|im| im.status == "completed" && matches(s, im))
            .count() as i32;
        let next_idx = received as usize;
        if next_idx >= s.doses_months.len() {
            continue; // series complete (per this simplified schedule)
        }
        let due_on = add_months(dob, s.doses_months[next_idx]);
        let gap = today.to_julian_day() as i64 - due_on.to_julian_day() as i64;
        let status = if gap > OVERDUE_GRACE_DAYS {
            "overdue"
        } else if gap >= 0 {
            "due"
        } else {
            "upcoming"
        };
        out.push(DueItem {
            family: s.family.to_string(),
            dose_number: received + 1,
            due_on,
            status,
        });
    }
    // overdue first, then due, then upcoming; within a tier, soonest due_on.
    let rank = |s: &str| match s {
        "overdue" => 0,
        "due" => 1,
        _ => 2,
    };
    out.sort_by(|a, b| {
        rank(a.status)
            .cmp(&rank(b.status))
            .then(a.due_on.cmp(&b.due_on))
    });
    out
}

/// Today's date (UTC) — the app clock for forecasting.
pub fn today() -> Date {
    OffsetDateTime::now_utc().date()
}

/// Recommended well-child visit ages (months), AAP/Bright Futures routine.
const WELL_CHILD: &[(f64, &str)] = &[
    (1.0, "1-month"),
    (2.0, "2-month"),
    (4.0, "4-month"),
    (6.0, "6-month"),
    (9.0, "9-month"),
    (12.0, "12-month"),
    (15.0, "15-month"),
    (18.0, "18-month"),
    (24.0, "24-month"),
    (30.0, "30-month"),
    (36.0, "3-year"),
    (48.0, "4-year"),
];

pub struct WellVisit {
    pub label: String,
    pub recommended_on: Date,
    pub past: bool,
}

/// Age-based well-child visit guidance: the recommended visits in a window
/// around today (last ~2 months → next ~13 months). This is cadence guidance,
/// not matched against completed appointments — so a "past" one may already be
/// done. The view notes that.
pub fn well_child(dob: Date, today: Date) -> Vec<WellVisit> {
    let lo = today.to_julian_day() - 60;
    let hi = today.to_julian_day() + 400;
    WELL_CHILD
        .iter()
        .filter_map(|(months, label)| {
            let on = add_months(dob, *months);
            let jd = on.to_julian_day();
            if jd >= lo && jd <= hi {
                Some(WellVisit {
                    label: label.to_string(),
                    recommended_on: on,
                    past: jd < today.to_julian_day(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn adults_get_no_forecast() {
        // A grown-up (b. 1988) is past the routine childhood schedule: no forecast,
        // so the chart's "N due" badge is 0 rather than flagging childhood doses.
        let dob = date!(1988 - 01 - 28);
        let today = date!(2026 - 07 - 01);
        assert!(!forecast_applies(dob, today));
        assert!(forecast(dob, &[], today).is_empty());
    }

    #[test]
    fn children_still_forecast() {
        // A ~2.5yo with no recorded vaccines still gets overdue/due items.
        let dob = date!(2024 - 01 - 01);
        let today = date!(2026 - 07 - 01);
        assert!(forecast_applies(dob, today));
        assert!(!forecast(dob, &[], today).is_empty());
    }

    #[test]
    fn cutoff_is_nineteenth_birthday() {
        let dob = date!(2007 - 07 - 01);
        assert!(forecast_applies(dob, date!(2026 - 06 - 30))); // just under 19
        assert!(!forecast_applies(dob, date!(2026 - 07 - 02))); // just over 19
    }

    #[test]
    fn families_match_by_cvx_and_keyword() {
        // Same clinical concept, different source vocabularies (PEMR-48): CVX
        // code from FHIR, NDC code from EHI, name-only from CAIR.
        let cvx = vaccine_families(Some("08"), "HEPATITIS B");
        assert_eq!(cvx, vec!["Hepatitis B"]);
        let ndc = vaccine_families(Some("0006-4093-02"), "HEPATITIS B, PED/ADOL");
        assert_eq!(ndc, vec!["Hepatitis B"]);
        let named = vaccine_families(None, "Hepatitis B");
        assert_eq!(named, vec!["Hepatitis B"]);
    }

    #[test]
    fn families_of_combination_vaccine_covers_each_disease_group() {
        // "DTAP/HIB/IPV" (EHI) counts toward DTaP, Hib, AND Polio — so a source
        // that lists the combo per-disease-group dedups onto the single row.
        let fams = vaccine_families(Some("49281-510-05"), "DTAP/HIB/IPV");
        assert_eq!(fams.len(), 3);
        assert!(fams.contains(&"DTaP"));
        assert!(fams.contains(&"Hib"));
        assert!(fams.contains(&"Polio (IPV)"));
    }

    #[test]
    fn unknown_vaccine_has_no_family() {
        // Influenza/COVID aren't in the routine childhood schedule — no family,
        // so the dedup falls back to the exact-name match instead.
        assert!(vaccine_families(None, "INFLUENZA, SPLIT TRI PF").is_empty());
        assert!(vaccine_families(None, "Some novel mRNA vaccine").is_empty());
    }
}
