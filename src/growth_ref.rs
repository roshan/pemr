//! Growth-chart reference percentiles (PEMR-24), spliced from two vendored
//! public datasets in `src/peds_data/` (embedded at compile time):
//!
//! - **0–24 mo: WHO Child Growth Standards** (weight / length / head
//!   circumference for age), from the CDC-hosted WHO data files:
//!   `ftp.cdc.gov/pub/Health_Statistics/NCHS/growthcharts/WHO-{Boys,Girls}-*-Percentiles.csv`.
//!   This is the US standard-of-care for under-2s (CDC's own recommendation
//!   since 2010) — the WHO standard describes optimal growth of breastfed
//!   infants, where the CDC infant reference's formula-fed cohort skews
//!   percentiles for breastfed babies.
//! - **24–36 mo: CDC infant charts** (`cdc.gov/growthcharts/data/zscore/
//!   {wtageinf,lenageinf,hcageinf}.csv`), the US reference once WHO hands off
//!   at 24 months. Only rows past 24 mo are used.
//!
//! Both use the same Box-Cox (LMS) model, so one percentile routine serves the
//! spliced curve. The small step in the bands at 24 mo is the real WHO→CDC
//! handoff, not a bug.
//!
//! WHO CSV columns: Month, L, M, S, P2.3, P5, P10, P25, P50, P75, P90, P95, P97.7.
//! CDC CSV columns: Sex(1=M,2=F), Agemos, L, M, S, P3, P5, P10, P25, P50, P75, P90, P95, P97.

const WHO_WEIGHT_BOYS: &str = include_str!("peds_data/who_weight_for_age_0_24mo_boys.csv");
const WHO_WEIGHT_GIRLS: &str = include_str!("peds_data/who_weight_for_age_0_24mo_girls.csv");
const WHO_LENGTH_BOYS: &str = include_str!("peds_data/who_length_for_age_0_24mo_boys.csv");
const WHO_LENGTH_GIRLS: &str = include_str!("peds_data/who_length_for_age_0_24mo_girls.csv");
const WHO_HEADCIRC_BOYS: &str = include_str!("peds_data/who_headcirc_for_age_0_24mo_boys.csv");
const WHO_HEADCIRC_GIRLS: &str = include_str!("peds_data/who_headcirc_for_age_0_24mo_girls.csv");

const CDC_WEIGHT: &str = include_str!("peds_data/cdc_weight_for_age_0_36mo.csv");
const CDC_LENGTH: &str = include_str!("peds_data/cdc_length_for_age_0_36mo.csv");
const CDC_HEADCIRC: &str = include_str!("peds_data/cdc_headcirc_for_age_0_36mo.csv");

/// Age at which the reference switches from WHO standards to CDC charts.
pub const WHO_CDC_SPLICE_MONTHS: f64 = 24.0;

#[derive(Clone, Copy)]
pub enum Measure {
    Weight,
    Length,
    HeadCirc,
}

#[derive(Debug, Clone, Copy)]
pub struct RefPoint {
    pub age_months: f64,
    /// LMS parameters — the Box-Cox model for the value distribution at this
    /// age (WHO ≤ 24 mo, CDC beyond). Used to compute an exact percentile for
    /// a measured value.
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

/// WHO files are per-sex: Month, L, M, S, then percentile columns
/// (P2.3, P5, P10, P25, P50, P75, P90, P95, P97.7).
fn parse_who(csv: &str) -> Vec<RefPoint> {
    csv.lines()
        .skip(1) // header (carries a BOM — skipped)
        .filter_map(|line| {
            let c: Vec<&str> = line.split(',').collect();
            if c.len() < 13 {
                return None;
            }
            Some(RefPoint {
                age_months: c[0].trim().parse().ok()?,
                l: c[1].trim().parse().ok()?,
                m: c[2].trim().parse().ok()?,
                s: c[3].trim().parse().ok()?,
                p5: c[5].trim().parse().ok()?,
                p50: c[8].trim().parse().ok()?,
                p95: c[11].trim().parse().ok()?,
            })
        })
        .collect()
}

/// CDC files carry both sexes: Sex, Agemos, L, M, S, then percentile columns
/// (P3, P5, P10, P25, P50, P75, P90, P95, P97).
fn parse_cdc(csv: &str, sex: u8) -> Vec<RefPoint> {
    csv.lines()
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
        .collect()
}

/// P5/P50/P95 reference curve for a measure + sex — WHO standards for
/// 0–24 mo spliced with CDC infant-chart rows for 24–36 mo — or empty if the
/// sex is unknown.
pub fn curve(measure: Measure, sex: u8) -> Vec<RefPoint> {
    let (who_b, who_g, cdc) = match measure {
        Measure::Weight => (WHO_WEIGHT_BOYS, WHO_WEIGHT_GIRLS, CDC_WEIGHT),
        Measure::Length => (WHO_LENGTH_BOYS, WHO_LENGTH_GIRLS, CDC_LENGTH),
        Measure::HeadCirc => (WHO_HEADCIRC_BOYS, WHO_HEADCIRC_GIRLS, CDC_HEADCIRC),
    };
    let who = match sex {
        1 => who_b,
        2 => who_g,
        _ => return Vec::new(),
    };
    let mut out = parse_who(who);
    out.extend(
        parse_cdc(cdc, sex).into_iter().filter(|r| r.age_months > WHO_CDC_SPLICE_MONTHS),
    );
    out.sort_by(|a, b| a.age_months.partial_cmp(&b.age_months).unwrap());
    out
}

/// Exact percentile (0–100) for a measured `value` at `age_months`, by the
/// official LMS method: z = ((X/M)^L − 1)/(L·S), percentile = Φ(z)·100, with
/// L, M, S linearly interpolated between the bracketing table rows (WHO rows
/// under 24 mo, CDC rows beyond). Returns None outside the table's age range
/// (or on an empty curve).
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

    /// Both datasets carry precomputed percentile columns, so the LMS math can
    /// be checked against the table itself: a value equal to the row's P5 /
    /// P50 / P95 must come back as that percentile — across the WHO rows, the
    /// CDC rows, and the splice boundary.
    #[test]
    fn lms_percentile_matches_table_columns() {
        for sex in [1u8, 2] {
            for measure in [Measure::Weight, Measure::Length, Measure::HeadCirc] {
                let curve = curve(measure, sex);
                assert!(!curve.is_empty());
                for r in curve.iter().step_by(3) {
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

    /// The 0–24 mo segment must be WHO, not CDC: spot-check published WHO
    /// medians (boys weight birth 3.3464 kg — CDC says 3.5302; girls weight
    /// 12 mo 8.9481 kg; boys length 12 mo 75.7488 cm).
    #[test]
    fn curve_uses_who_standards_below_24_months() {
        let bw = curve(Measure::Weight, 1);
        assert!((bw[0].age_months - 0.0).abs() < 1e-9);
        assert!((bw[0].m - 3.3464).abs() < 1e-4, "birth median was {}", bw[0].m);

        let gw = curve(Measure::Weight, 2);
        let at12 = gw.iter().find(|r| (r.age_months - 12.0).abs() < 1e-9).unwrap();
        assert!((at12.m - 8.9481).abs() < 1e-4);

        let bl = curve(Measure::Length, 1);
        let at12 = bl.iter().find(|r| (r.age_months - 12.0).abs() < 1e-9).unwrap();
        assert!((at12.m - 75.7488).abs() < 1e-4);
    }

    /// The splice: monthly WHO rows through 24.0, CDC rows strictly after,
    /// out to ~36 mo (the CDC length table's last row is 35.5), strictly
    /// increasing throughout.
    #[test]
    fn curve_splices_who_then_cdc() {
        for sex in [1u8, 2] {
            for measure in [Measure::Weight, Measure::Length, Measure::HeadCirc] {
                let c = curve(measure, sex);
                assert!((c[0].age_months - 0.0).abs() < 1e-9);
                assert!(c.last().unwrap().age_months >= 35.5);
                let who_rows = c.iter().filter(|r| r.age_months <= WHO_CDC_SPLICE_MONTHS);
                assert_eq!(who_rows.count(), 25, "monthly WHO rows 0..=24");
                for w in c.windows(2) {
                    assert!(w[0].age_months < w[1].age_months);
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
