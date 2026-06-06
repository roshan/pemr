//! CDC growth-chart reference percentiles (PEMR-24), vendored from the public
//! CDC LMS data files (public domain, US government):
//!   https://www.cdc.gov/growthcharts/data/zscore/{wtageinf,lenageinf,hcageinf}.csv
//! Files live in `src/peds_data/` and are embedded at compile time. These are
//! the CDC 0–36 month infant charts (weight / length / head-circumference);
//! the US standard-of-care is WHO for 0–24 mo, but CDC infant data is close and
//! authoritative — labeled "CDC" in the UI. Each row already carries the
//! precomputed percentile columns, so we plot P5/P50/P95 directly.
//!
//! CSV columns: Sex(1=M,2=F), Agemos, L, M, S, P3, P5, P10, P25, P50, P75, P90, P95, P97.

const WEIGHT: &str = include_str!("peds_data/cdc_weight_for_age_0_36mo.csv");
const LENGTH: &str = include_str!("peds_data/cdc_length_for_age_0_36mo.csv");
const HEADCIRC: &str = include_str!("peds_data/cdc_headcirc_for_age_0_36mo.csv");

#[derive(Clone, Copy)]
pub enum Measure {
    Weight,
    Length,
    HeadCirc,
}

#[derive(Debug, Clone, Copy)]
pub struct RefPoint {
    pub age_months: f64,
    pub p5: f64,
    pub p50: f64,
    pub p95: f64,
}

/// Map a subject's free-text `sex_at_birth` to the CDC sex code (1=M, 2=F).
pub fn sex_code(sex_at_birth: Option<&str>) -> Option<u8> {
    let s = sex_at_birth?.trim().to_ascii_lowercase();
    match s.chars().next()? {
        'm' | '1' => Some(1),
        'f' | '2' => Some(2),
        _ => None,
    }
}

fn parse(csv: &str, sex: u8) -> Vec<RefPoint> {
    let mut out: Vec<RefPoint> = csv
        .lines()
        .skip(1) // header (may carry a BOM — skipped)
        .filter_map(|line| {
            let c: Vec<&str> = line.split(',').collect();
            if c.len() < 13 {
                return None;
            }
            if c[0].trim().parse::<u8>().ok()? != sex {
                return None;
            }
            Some(RefPoint {
                age_months: c[1].trim().parse().ok()?,
                p5: c[6].trim().parse().ok()?,
                p50: c[9].trim().parse().ok()?,
                p95: c[12].trim().parse().ok()?,
            })
        })
        .collect();
    out.sort_by(|a, b| a.age_months.partial_cmp(&b.age_months).unwrap());
    out
}

/// CDC P5/P50/P95 reference curve for a measure + sex (0–36 months), or empty
/// if the sex is unknown.
pub fn curve(measure: Measure, sex: u8) -> Vec<RefPoint> {
    let csv = match measure {
        Measure::Weight => WEIGHT,
        Measure::Length => LENGTH,
        Measure::HeadCirc => HEADCIRC,
    };
    parse(csv, sex)
}
