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
    /// LMS parameters — the CDC's Box-Cox model for the value distribution at
    /// this age. Used to compute an exact percentile for a measured value.
    pub l: f64,
    pub m: f64,
    pub s: f64,
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
                l: c[2].trim().parse().ok()?,
                m: c[3].trim().parse().ok()?,
                s: c[4].trim().parse().ok()?,
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

/// Exact CDC percentile (0–100) for a measured `value` at `age_months`, by the
/// official LMS method: z = ((X/M)^L − 1)/(L·S), percentile = Φ(z)·100, with
/// L, M, S linearly interpolated between the bracketing table rows. Returns
/// None outside the table's age range (or on an empty curve).
pub fn percentile(curve: &[RefPoint], age_months: f64, value: f64) -> Option<f64> {
    let (first, last) = (curve.first()?, curve.last()?);
    if age_months < first.age_months || age_months > last.age_months || value <= 0.0 {
        return None;
    }
    let hi = curve.iter().position(|r| r.age_months >= age_months)?;
    let (a, b) = (&curve[hi.saturating_sub(1)], &curve[hi]);
    let t = if b.age_months > a.age_months {
        (age_months - a.age_months) / (b.age_months - a.age_months)
    } else {
        0.0
    };
    let lerp = |x: f64, y: f64| x + (y - x) * t;
    let (l, m, s) = (lerp(a.l, b.l), lerp(a.m, b.m), lerp(a.s, b.s));
    let z = if l.abs() > 1e-9 {
        ((value / m).powf(l) - 1.0) / (l * s)
    } else {
        (value / m).ln() / s
    };
    Some(100.0 * 0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2)))
}

/// Abramowitz & Stegun 7.1.26 — max error ~1.5e-7, far below display precision.
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = ((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t
        - 0.284_496_736)
        * t
        + 0.254_829_592;
    sign * (1.0 - poly * t * (-x * x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CSV rows carry precomputed percentile columns, so the LMS math can
    /// be checked against the table itself: a value equal to the row's P5 /
    /// P50 / P95 must come back as that percentile.
    #[test]
    fn lms_percentile_matches_table_columns() {
        for sex in [1u8, 2] {
            for measure in [Measure::Weight, Measure::Length, Measure::HeadCirc] {
                let curve = curve(measure, sex);
                assert!(!curve.is_empty());
                for r in curve.iter().step_by(7) {
                    let p50 = percentile(&curve, r.age_months, r.p50).unwrap();
                    let p5 = percentile(&curve, r.age_months, r.p5).unwrap();
                    let p95 = percentile(&curve, r.age_months, r.p95).unwrap();
                    assert!((p50 - 50.0).abs() < 0.5, "P50 came back {p50}");
                    assert!((p5 - 5.0).abs() < 0.5, "P5 came back {p5}");
                    assert!((p95 - 95.0).abs() < 0.5, "P95 came back {p95}");
                }
            }
        }
    }

    #[test]
    fn percentile_none_outside_range() {
        let curve = curve(Measure::Weight, 1);
        assert!(percentile(&curve, 40.0, 12.0).is_none());
        assert!(percentile(&curve, -1.0, 3.0).is_none());
        assert!(percentile(&[], 6.0, 7.0).is_none());
    }
}
