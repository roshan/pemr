//! Age basis for the developmental-milestone tracker (PEMR-38). Computes the age
//! the tracker uses from date of birth + gestational age at birth, and maps it to
//! a CDC checkpoint. The correction is applied **silently** — this module returns
//! the basis only so the UI can show an *optional* small label. It emits no
//! reminder / "remember to adjust" text; the caller just renders the right list.
//!
//! Rule (from PEMR-35):
//!   * gestational age ≥ 37 weeks (or unknown) → chronological age, as-is;
//!   * gestational age < 37 weeks → **corrected age** = chronological age minus
//!     weeks premature (`40 − gestational`), used until **24 months chronological**,
//!     then switch to chronological age.
//!
//! Ages are counted in **completed calendar months** (a child reaches the
//! 12-month checkpoint on their 1st birthday), not `days / 30.4375` — the latter
//! drifts ~0.25 day/month and would map a child's exact birthday to the previous
//! checkpoint. The corrected age is computed by counting from a *virtual* birth
//! date shifted later by the weeks premature.

use time::Date;

/// Births at or above this gestational age are treated as term (no correction).
const CORRECTION_THRESHOLD_WEEKS: i16 = 37;
/// Full-term gestation in weeks — the datum the correction is measured from.
const TERM_WEEKS: i16 = 40;
/// Correction is applied only while chronological age is below this (months).
const CORRECTION_UNTIL_MONTHS: i32 = 24;

/// Which age the tracker is keyed on. Stored on each response row
/// (`milestone_responses.age_basis_used`) as `as_str()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeBasis {
    Chronological,
    Corrected,
}

impl AgeBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            AgeBasis::Chronological => "chronological",
            AgeBasis::Corrected => "corrected",
        }
    }

    /// Small, non-intrusive label for the optional UI chip. Deliberately neutral —
    /// no "remember to adjust" nag.
    pub fn label(self) -> &'static str {
        match self {
            AgeBasis::Chronological => "chronological age",
            AgeBasis::Corrected => "corrected age",
        }
    }
}

/// The computed tracker age for a subject.
#[derive(Debug, Clone, Copy)]
pub struct TrackerAge {
    /// Raw chronological age in completed months (clamped ≥ 0). Part of the
    /// computed result + asserted by tests; deliberately NOT displayed anywhere
    /// (the correction is silent — we never surface the chronological/corrected
    /// gap to the user).
    #[allow(dead_code)]
    pub chronological_months: i32,
    /// The age the tracker actually uses (corrected or chronological), months.
    pub computed_months: i32,
    pub basis: AgeBasis,
    /// The CDC checkpoint (months) whose checklist to show.
    pub checkpoint: i32,
}

/// Completed whole calendar months between two dates (0 if `to` precedes `from`).
fn completed_months(from: Date, to: Date) -> i32 {
    let mut months = (to.year() - from.year()) * 12
        + (u8::from(to.month()) as i32 - u8::from(from.month()) as i32);
    if to.day() < from.day() {
        months -= 1;
    }
    months.max(0)
}

/// The largest checkpoint ≤ `months`, clamped to the checkpoint range. Below the
/// first checkpoint → the first; at/after the last → the last. So a child always
/// maps to the most recent checkpoint they've reached.
pub fn checkpoint_for_age(months: i32) -> i32 {
    let cps = crate::milestones::CHECKPOINTS;
    let first = cps[0];
    let last = cps[cps.len() - 1];
    if months < first {
        return first;
    }
    cps.iter().copied().filter(|&cp| cp <= months).last().unwrap_or(last)
}

/// Compute the tracker age + basis + checkpoint. `gestational_age_weeks` is the
/// weeks of gestation at birth (`None` = unknown → treated as term). `today` is
/// injected so the caller controls the clock (mirrors `peds::forecast`).
///
/// Never panics: a future or same-day DOB yields a 0-month age → the first
/// checkpoint.
pub fn tracker_age(dob: Date, gestational_age_weeks: Option<i16>, today: Date) -> TrackerAge {
    let chronological_months = completed_months(dob, today);
    let preterm = matches!(gestational_age_weeks, Some(ga) if ga < CORRECTION_THRESHOLD_WEEKS);

    let (computed_months, basis) = if preterm && chronological_months < CORRECTION_UNTIL_MONTHS {
        let ga = gestational_age_weeks.unwrap();
        let weeks_premature = (TERM_WEEKS - ga).max(0) as i32;
        // Virtual birth date shifted later by the weeks premature; corrected age =
        // completed months from there.
        let virtual_dob =
            Date::from_julian_day(dob.to_julian_day() + weeks_premature * 7).unwrap_or(dob);
        (completed_months(virtual_dob, today), AgeBasis::Corrected)
    } else {
        (chronological_months, AgeBasis::Chronological)
    };

    TrackerAge {
        chronological_months,
        computed_months,
        basis,
        checkpoint: checkpoint_for_age(computed_months),
    }
}

/// Add `n` whole calendar months to a date, clamping the day to the target
/// month's length (Jan 31 + 1mo → Feb 28/29). Never panics.
fn add_months(d: Date, n: i32) -> Date {
    let total = d.year() as i64 * 12 + (u8::from(d.month()) as i64 - 1) + n as i64;
    let year = total.div_euclid(12) as i32;
    let month = time::Month::try_from(total.rem_euclid(12) as u8 + 1).unwrap_or(d.month());
    let day = d.day();
    // Largest valid day ≤ the source day (handles shorter target months).
    (1..=day)
        .rev()
        .find_map(|dd| Date::from_calendar_date(year, month, dd).ok())
        .unwrap_or(d)
}

/// The calendar date on which the child reaches tracker-age `months` — the same
/// basis `tracker_age` reports (corrected below 24 months for a preterm birth,
/// else chronological). Used to bound the observed-on date to a checkpoint's age
/// window.
fn date_at_tracker_age(dob: Date, gestational_age_weeks: Option<i16>, months: i32) -> Date {
    let preterm = matches!(gestational_age_weeks, Some(ga) if ga < CORRECTION_THRESHOLD_WEEKS);
    let base = if preterm && months < CORRECTION_UNTIL_MONTHS {
        let weeks_premature = (TERM_WEEKS - gestational_age_weeks.unwrap()).max(0) as i32;
        Date::from_julian_day(dob.to_julian_day() + weeks_premature * 7).unwrap_or(dob)
    } else {
        dob
    };
    add_months(base, months)
}

/// The latest calendar date on which the child is still within `checkpoint`'s age
/// window — i.e. the day before they reach the next checkpoint's age — capped at
/// `today` (never a future date). For the final checkpoint (open-ended) it's just
/// `today`. This is the sensible default observed-on date when marking a
/// milestone "yes": for the current period it lands on today; for an earlier
/// period it lands on the end of that period.
pub fn latest_date_in_period(
    dob: Date,
    gestational_age_weeks: Option<i16>,
    checkpoint: i32,
    today: Date,
) -> Date {
    let next = crate::milestones::CHECKPOINTS
        .iter()
        .copied()
        .find(|&c| c > checkpoint);
    let end = match next {
        Some(n) => {
            let start_of_next = date_at_tracker_age(dob, gestational_age_weeks, n);
            Date::from_julian_day(start_of_next.to_julian_day() - 1).unwrap_or(start_of_next)
        }
        None => today,
    };
    end.min(today)
}

/// Format a completed-month age as a short human string ("9 months",
/// "2 years 1 month", "3 years").
pub fn fmt_months(months: i32) -> String {
    let m = months.max(0);
    let (y, r) = (m / 12, m % 12);
    match (y, r) {
        (0, r) => format!("{r} month{}", if r == 1 { "" } else { "s" }),
        (y, 0) => format!("{y} year{}", if y == 1 { "" } else { "s" }),
        (y, r) => format!(
            "{y} year{} {r} month{}",
            if y == 1 { "" } else { "s" },
            if r == 1 { "" } else { "s" }
        ),
    }
}

/// Compact age label for dense layouts (the band view's column headers), where
/// `fmt_months` prose ("2 years 6 months") is far too wide: months up to 2
/// years, then years (`2y`, `2y6m`, `5y`).
pub fn fmt_months_short(months: i32) -> String {
    let m = months.max(0);
    if m < 24 {
        return format!("{m}m");
    }
    let (y, r) = (m / 12, m % 12);
    if r == 0 { format!("{y}y") } else { format!("{y}y{r}m") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn term_baby_uses_chronological() {
        // 40-week term, ~7 months old → 6-month checklist, chronological.
        let dob = date!(2025 - 12 - 01);
        let today = date!(2026 - 07 - 10); // 7 completed months
        let a = tracker_age(dob, Some(40), today);
        assert_eq!(a.basis, AgeBasis::Chronological);
        assert_eq!(a.chronological_months, 7);
        assert_eq!(a.checkpoint, 6);
        assert_eq!(a.computed_months, a.chronological_months);
    }

    #[test]
    fn first_birthday_maps_to_12mo_checkpoint() {
        // The exact 1st birthday must be the 12-month checklist, not 9-month
        // (regression guard for the days/30.4375 drift).
        let dob = date!(2025 - 01 - 01);
        let today = date!(2026 - 01 - 01);
        let a = tracker_age(dob, None, today);
        assert_eq!(a.chronological_months, 12);
        assert_eq!(a.checkpoint, 12);
        assert_eq!(a.basis, AgeBasis::Chronological);
    }

    #[test]
    fn term_boundary_37_weeks_is_not_corrected() {
        let dob = date!(2025 - 01 - 01);
        let today = date!(2025 - 07 - 01); // 6 months
        assert_eq!(tracker_age(dob, Some(37), today).basis, AgeBasis::Chronological);
        assert_eq!(tracker_age(dob, Some(36), today).basis, AgeBasis::Corrected);
    }

    #[test]
    fn preemie_corrected_below_24mo_switchover() {
        // 32-week preemie (8 weeks premature).
        let dob = date!(2024 - 07 - 01);
        let before = date!(2026 - 06 - 10); // 23 chronological months
        let a = tracker_age(dob, Some(32), before);
        assert_eq!(a.basis, AgeBasis::Corrected);
        assert_eq!(a.chronological_months, 23);
        assert!(a.computed_months < a.chronological_months);
        assert_eq!(a.checkpoint, 18);
    }

    #[test]
    fn preemie_switches_to_chronological_at_24mo() {
        let dob = date!(2024 - 07 - 01);
        let after = date!(2026 - 07 - 05); // 24 chronological months
        let a = tracker_age(dob, Some(32), after);
        assert_eq!(a.basis, AgeBasis::Chronological);
        assert_eq!(a.chronological_months, 24);
        assert_eq!(a.checkpoint, 24);
    }

    #[test]
    fn checkpoint_mapping_exact_and_between() {
        assert_eq!(checkpoint_for_age(15), 15); // exact
        assert_eq!(checkpoint_for_age(16), 15); // between 15 and 18 → 15
        assert_eq!(checkpoint_for_age(17), 15);
        assert_eq!(checkpoint_for_age(0), 2); // before first → first
        assert_eq!(checkpoint_for_age(1), 2);
        assert_eq!(checkpoint_for_age(60), 60); // last
        assert_eq!(checkpoint_for_age(84), 60); // after last → last
    }

    #[test]
    fn day_of_birth_is_first_checkpoint_no_panic() {
        let dob = date!(2026 - 07 - 10);
        let a = tracker_age(dob, Some(30), dob);
        assert_eq!(a.chronological_months, 0);
        assert_eq!(a.computed_months, 0);
        assert_eq!(a.checkpoint, 2);
    }

    #[test]
    fn future_dob_does_not_panic() {
        let dob = date!(2027 - 01 - 01);
        let today = date!(2026 - 07 - 10);
        let a = tracker_age(dob, None, today);
        assert_eq!(a.chronological_months, 0);
        assert_eq!(a.checkpoint, 2);
    }

    #[test]
    fn very_premature_corrected_age_clamps_nonnegative() {
        // 26-week preemie, ~1 month chronological → correction (14 wks) would go
        // negative; clamp to 0, first checkpoint, no panic.
        let dob = date!(2026 - 06 - 10);
        let today = date!(2026 - 07 - 10);
        let a = tracker_age(dob, Some(26), today);
        assert_eq!(a.basis, AgeBasis::Corrected);
        assert_eq!(a.computed_months, 0);
        assert_eq!(a.checkpoint, 2);
    }

    #[test]
    fn latest_date_in_period_bounds_to_period_end_or_today() {
        // Term child born 2025-01-01; "today" is well past the 2-month period.
        let dob = date!(2025 - 01 - 01);
        let today = date!(2026 - 07 - 10);
        // 2-month period ends the day before the 4-month checkpoint (2025-05-01).
        let d = latest_date_in_period(dob, None, 2, today);
        assert_eq!(d, date!(2025 - 04 - 30));
        // The current period (child is ~18 months) caps at today, not a future date.
        let cur = latest_date_in_period(dob, None, 18, today);
        assert_eq!(cur, today);
        // The final checkpoint is open-ended → today.
        assert_eq!(latest_date_in_period(dob, None, 60, today), today);
    }

    #[test]
    fn latest_date_in_period_uses_corrected_basis_for_preterm() {
        // 32-week preemie (8 weeks premature). The 6-month CORRECTED period ends
        // the day before corrected age 9mo, i.e. later in calendar time than a
        // term child's 9-month date.
        let dob = date!(2024 - 07 - 01);
        let today = date!(2026 - 07 - 10);
        let preemie = latest_date_in_period(dob, Some(32), 6, today);
        let term = latest_date_in_period(dob, None, 6, today);
        assert!(preemie > term, "corrected period end should be later than chronological");
    }

    #[test]
    fn fmt_months_reads_naturally() {
        assert_eq!(fmt_months(9), "9 months");
        assert_eq!(fmt_months(1), "1 month");
        assert_eq!(fmt_months(12), "1 year");
        assert_eq!(fmt_months(25), "2 years 1 month");
        assert_eq!(fmt_months(36), "3 years");
    }

    #[test]
    fn fmt_months_short_stays_narrow() {
        assert_eq!(fmt_months_short(2), "2m");
        assert_eq!(fmt_months_short(18), "18m");
        assert_eq!(fmt_months_short(24), "2y");
        assert_eq!(fmt_months_short(30), "2y6m");
        assert_eq!(fmt_months_short(60), "5y");
    }
}
