//! Subject-page modules. Each is a **self-contained feature**: it fetches its own
//! data once and renders itself for the requested [`Mode`] (the on-screen card, or
//! the printable summary section), or returns `None` to opt out. Callers just
//! iterate [`MODULES`] — they have *no* per-feature knowledge, so adding a
//! subfeature is one fn here plus one line in the registry. Mirrors the
//! `sync::TaskDef` function-pointer registry.

use std::future::Future;
use std::pin::Pin;

use maud::{Markup, Render, html};
use sqlx::PgPool;

use crate::models::{
    Allergy, Appointment, CareTeamMember, Condition, Immunization, InsuranceCoverageRow,
    Medication, Subject, VitalRow,
};
use crate::views::components as c;

/// Which surface a module is rendering for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The subject chart — a bordered summary card (truncated, with "View all").
    Card,
    /// The printable one-pager — a flat section with the full list, no links.
    Print,
}

type BoxFut<'a> = Pin<Box<dyn Future<Output = Result<Option<Markup>, sqlx::Error>> + Send + 'a>>;

/// A subject-page module: `(pool, subject, mode) -> optional rendered block`.
pub type Module = for<'a> fn(&'a PgPool, &'a Subject, Mode) -> BoxFut<'a>;

/// Ordered registry — the render order on both the chart and the printout. Add a
/// subfeature = add a line (and a fn below).
pub const MODULES: &[Module] = &[
    problems,
    allergies,
    medications,
    immunizations,
    vitals,
    growth,
    appointments,
    care_team,
    insurance,
];

/// Render every applicable module's block for `mode`, in registry order.
pub async fn render_all(pool: &PgPool, s: &Subject, mode: Mode) -> Result<Vec<Markup>, sqlx::Error> {
    let mut out = Vec::new();
    for m in MODULES {
        if let Some(block) = m(pool, s, mode).await? {
            out.push(block);
        }
    }
    Ok(out)
}

// ── shared render helpers ────────────────────────────────────────────────────
const CARD_MAX: usize = 4;

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n:.1}") }
}

/// Card wrapper: bordered panel, optionally with a "View all →" link.
fn card<T: Render>(title: T, href: Option<String>, body: Markup) -> Markup {
    match href {
        Some(h) => c::summary_panel_linked(title, h, body),
        None => c::summary_panel(title, body),
    }
}

/// Print wrapper: a flat, page-break-friendly section.
fn print_section(title: &str, body: Markup) -> Markup {
    html! { section class="mb-4" { (c::section_heading(title)) (body) } }
}

/// Card body: the first `CARD_MAX` items + an "and N more" affordance.
fn truncated_list(
    items: impl Iterator<Item = Markup>,
    total: usize,
    more_href: Option<String>,
) -> Markup {
    let shown: Vec<Markup> = items.take(CARD_MAX).collect();
    let overflow = total.saturating_sub(shown.len());
    html! {
        (c::panel_list(html! { @for item in shown { (item) } }))
        @if overflow > 0 {
            @if let Some(href) = more_href {
                a href=(href) class="block mt-2 text-xs text-brand hover:underline" {
                    "and " (overflow) " more →"
                }
            } @else {
                p class="mt-2 text-xs text-muted" { "and " (overflow) " more" }
            }
        }
    }
}

/// Print body: the full list.
fn full_list(items: impl Iterator<Item = Markup>) -> Markup {
    html! { (c::panel_list(html! { @for item in items { (item) } })) }
}

// ── modules ─────────────────────────────────────────────────────────────────

fn problems<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Condition>(
            "select * from conditions where subject_id = $1 and status = 'active'
              order by onset_date desc nulls last, created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        let item = |x: &Condition| c::panel_list_item(
            html! { (x.name) },
            html! { @if let Some(d) = x.onset_date { "since " (d) } },
        );
        Ok(Some(match mode {
            Mode::Card => card("Problems", None, if rows.is_empty() {
                c::empty_state("No active problems")
            } else {
                truncated_list(rows.iter().map(item), rows.len(), None)
            }),
            Mode::Print => print_section("Active problems", if rows.is_empty() {
                c::empty_state("None recorded")
            } else {
                full_list(rows.iter().map(item))
            }),
        }))
    })
}

fn allergies<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        // Per-subject opt-in feature (PEMR-47 pattern): the allergy card only
        // shows on subjects with the "allergies" feature enabled. Allergy
        // records are untouched when disabled.
        if !crate::feature_registry::is_enabled(pool, s.id, "allergies").await? {
            return Ok(None);
        }
        let rows = sqlx::query_as::<_, Allergy>(
            "select * from allergies where subject_id = $1 and status <> 'entered_in_error'
              order by created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        let empty = if s.no_known_allergies {
            "No known allergies (asserted)"
        } else if mode == Mode::Print {
            "None recorded"
        } else {
            "No allergies recorded"
        };
        // Print includes the reaction detail; the card keeps it terse.
        let item = |a: &Allergy, with_reaction: bool| c::panel_list_item(
            html! { (a.substance) },
            html! {
                @if let Some(crit) = &a.criticality { (crit) }
                @else if let Some(sev) = &a.severity { (sev) }
                @if with_reaction { @if let Some(r) = &a.reaction { " — " (r) } }
            },
        );
        Ok(Some(match mode {
            Mode::Card => card("Allergies", None, if rows.is_empty() {
                c::empty_state(empty)
            } else {
                truncated_list(rows.iter().map(|a| item(a, false)), rows.len(), None)
            }),
            Mode::Print => print_section("Allergies", if rows.is_empty() {
                c::empty_state(empty)
            } else {
                full_list(rows.iter().map(|a| item(a, true)))
            }),
        }))
    })
}

fn medications<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        // Active first (recency within each group), then finished. The card is
        // `CARD_MAX`-truncated either way, so finished courses stay reachable
        // only when few; full history lives on the DB.
        let rows = sqlx::query_as::<_, Medication>(
            "select * from medications where subject_id = $1 and status <> 'entered_in_error'
              order by (status = 'active') desc, coalesce(started_on, created_at::date) desc, created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        // Row body: name + status accent + dates. The dose/frequency line is its
        // own wrapped row — a long frequency must never push the card edge
        // (`panel_list_item` details are whitespace-nowrap, which is the bug
        // this fixes).
        let item = |m: &Medication| {
            let active = m.status == "active";
            let dates = match (m.started_on, m.ended_on) {
                (Some(s), Some(e)) => format!("{s} \u{2192} {e}"),
                (Some(s), None) => format!("since {s}"),
                (None, Some(e)) => format!("ended {e}"),
                (None, None) => String::new(),
            };
            c::panel_list_item(
                html! {
                    div class="min-w-0" {
                        div class="flex items-center gap-2" {
                            span class={ (if active { "text-ink font-medium" } else { "text-muted line-through" }) } {
                                (m.name)
                            }
                            @if active {
                                (c::badge_ok("active"))
                            } @else {
                                (c::badge_neutral("done"))
                            }
                        }
                        @if m.dose.is_some() || m.frequency.is_some() {
                            div class="text-xs text-muted mt-0.5 break-words" {
                                @if let Some(dose) = &m.dose { (dose) }
                                @if let Some(freq) = &m.frequency {
                                    @if m.dose.is_some() { " \u{00b7} " }
                                    (freq)
                                }
                            }
                        }
                    }
                },
                html! { span class="text-xs text-muted whitespace-nowrap" { (dates) } },
            )
        };
        Ok(Some(match mode {
            Mode::Card => card("Medications", None, if rows.is_empty() {
                c::empty_state("No active medications")
            } else {
                truncated_list(rows.iter().map(item), rows.len(), None)
            }),
            Mode::Print => print_section("Medications", if rows.is_empty() {
                c::empty_state("None recorded")
            } else {
                full_list(rows.iter().map(item))
            }),
        }))
    })
}

fn immunizations<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Immunization>(
            "select * from immunizations where subject_id = $1
              order by occurred_at desc nulls last, created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        Ok(Some(match mode {
            Mode::Card => {
                let due = match s.dob {
                    Some(dob) => crate::peds::forecast(dob, &rows, crate::peds::today())
                        .iter()
                        .filter(|d| d.status != "upcoming")
                        .count(),
                    None => 0,
                };
                let href = format!("/subjects/{}/immunizations", s.id);
                card(
                    html! { "Immunizations" @if due > 0 { " " (c::badge_warn(format!("{due} due"))) } },
                    Some(href.clone()),
                    if rows.is_empty() {
                        c::empty_state("No immunizations recorded")
                    } else {
                        truncated_list(rows.iter().map(|im| c::panel_list_item(
                            html! { (im.vaccine) },
                            html! { @if let Some(d) = im.occurred_at { (d) } },
                        )), rows.len(), Some(href))
                    },
                )
            }
            Mode::Print => print_section("Immunizations", if rows.is_empty() {
                c::empty_state("None recorded")
            } else {
                full_list(rows.iter().map(|im| c::panel_list_item(
                    html! { (im.vaccine) @if let Some(n) = im.dose_number { " " (c::muted(format!("dose {n}"))) } },
                    html! { @if let Some(d) = im.occurred_at { (d) } @else { "date unknown" } },
                )))
            }),
        }))
    })
}

fn vitals<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, VitalRow>(
            "select display, value_num::float8 as value_num, value_text, unit, effective_on, abnormal_flag
               from observations where subject_id = $1
              order by effective_on desc, created_at desc limit 8",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        let val = |v: &VitalRow| v.value_num.map(fmt_num)
            .or_else(|| v.value_text.clone())
            .unwrap_or_else(|| "—".into());
        Ok(Some(match mode {
            Mode::Card => {
                let total: i64 = sqlx::query_scalar(
                    "select count(*) from observations where subject_id = $1",
                )
                .bind(s.id)
                .fetch_one(pool)
                .await?;
                let href = format!("/subjects/{}/vitals", s.id);
                card("Recent vitals & labs", Some(href.clone()), if rows.is_empty() {
                    c::empty_state("No vitals or labs recorded")
                } else {
                    truncated_list(rows.iter().map(|v| c::panel_list_item(
                        html! {
                            (v.display)
                            @if let Some(f) = &v.abnormal_flag { @if f != "normal" { " " (c::badge_warn(f)) } }
                        },
                        html! { (val(v)) @if let Some(u) = &v.unit { " " (u) } " · " (v.effective_on) },
                    )), total as usize, Some(href))
                })
            }
            Mode::Print => print_section("Recent vitals & growth", if rows.is_empty() {
                c::empty_state("None recorded")
            } else {
                full_list(rows.iter().map(|v| c::panel_list_item(
                    html! { (v.display) },
                    html! { (val(v)) @if let Some(u) = &v.unit { " " (u) } " · " (v.effective_on) },
                )))
            }),
        }))
    })
}

fn growth<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        // Chart is a card-only affordance; the printout folds growth into the
        // "Recent vitals & growth" list above.
        if mode == Mode::Print {
            return Ok(None);
        }
        // Per-subject opt-in feature (PEMR-47): the mini chart card only shows
        // on subjects with the "growth" feature enabled (default: Astra).
        if !crate::feature_registry::is_enabled(pool, s.id, "growth").await? {
            return Ok(None);
        }
        let series = crate::handlers::subjects::growth_series(pool, s).await?;
        if series.iter().all(|g| g.points.is_empty()) {
            return Ok(None);
        }
        Ok(Some(crate::views::growth::mini_card(s.id, &series)))
    })
}

fn appointments<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Appointment>(
            "select * from appointments where subject_id = $1 and starts_at >= now()
              order by starts_at asc limit 6",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        let item = |ap: &Appointment| c::panel_list_item(
            html! { (ap.title) },
            html! { (ap.starts_at.date()) },
        );
        Ok(Some(match mode {
            Mode::Card => {
                let href = format!("/subjects/{}/appointments", s.id);
                card("Upcoming appointments", Some(href.clone()), if rows.is_empty() {
                    c::empty_state("None scheduled")
                } else {
                    truncated_list(rows.iter().map(item), rows.len(), Some(href))
                })
            }
            Mode::Print => print_section("Upcoming appointments", if rows.is_empty() {
                c::empty_state("None scheduled")
            } else {
                full_list(rows.iter().map(item))
            }),
        }))
    })
}

fn care_team<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, CareTeamMember>(
            "select sp.role, p.full_name, p.specialty
               from subject_providers sp join providers p on p.id = sp.provider_id
              where sp.subject_id = $1 and sp.active
              order by p.full_name",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        let item = |m: &CareTeamMember| c::panel_list_item(
            html! { (m.full_name) @if let Some(sp) = &m.specialty { " " (c::muted(sp)) } },
            html! { (m.role) },
        );
        Ok(Some(match mode {
            Mode::Card => {
                let href = format!("/subjects/{}/care-team", s.id);
                card("Care team", Some(href.clone()), if rows.is_empty() {
                    c::empty_state("No care team recorded")
                } else {
                    truncated_list(rows.iter().map(item), rows.len(), Some(href))
                })
            }
            Mode::Print => print_section("Care team", if rows.is_empty() {
                c::empty_state("None recorded")
            } else {
                full_list(rows.iter().map(item))
            }),
        }))
    })
}

fn insurance<'a>(pool: &'a PgPool, s: &'a Subject, mode: Mode) -> BoxFut<'a> {
    Box::pin(async move {
        // Insurance is on the chart, not the printable clinical handout.
        if mode == Mode::Print {
            return Ok(None);
        }
        let rows = sqlx::query_as::<_, InsuranceCoverageRow>(
            "select p.payer_name, p.plan_name, p.plan_kind,
                    si.relationship, coalesce(si.member_id, p.member_id) as member_id
               from subject_insurance si join insurance_plans p on p.id = si.plan_id
              where si.subject_id = $1
              order by si.is_primary desc, p.payer_name",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        Ok(Some(card("Insurance", Some("/insurance".to_string()), if rows.is_empty() {
            c::empty_state("No insurance recorded")
        } else {
            truncated_list(rows.iter().map(|ins| c::panel_list_item(
                html! { (ins.payer_name) @if let Some(pn) = &ins.plan_name { " " (c::muted(pn)) } },
                html! {
                    @if let Some(m) = &ins.member_id { (m) " · " }
                    (ins.plan_kind) " · " (ins.relationship)
                },
            )), rows.len(), Some("/insurance".to_string()))
        })))
    })
}

/// Compact counts for the home-dashboard snapshot (no full row loads). Kept here
/// so the counts track the modules' own filters (active problems/meds, etc.).
pub struct SnapshotCounts {
    pub problems: usize,
    pub medications: usize,
    pub allergies: usize,
    pub immunizations: usize,
    pub vitals: usize,
    pub appointments: usize,
    pub vaccines_due: usize,
}

impl SnapshotCounts {
    /// Anything worth surfacing on the home dashboard?
    pub fn any(&self) -> bool {
        self.problems
            + self.medications
            + self.allergies
            + self.immunizations
            + self.vitals
            + self.appointments
            > 0
            || self.vaccines_due > 0
    }
}

pub async fn snapshot_counts(pool: &PgPool, s: &Subject) -> Result<SnapshotCounts, sqlx::Error> {
    async fn n(pool: &PgPool, sql: &str, id: uuid::Uuid) -> Result<usize, sqlx::Error> {
        let c: i64 = sqlx::query_scalar(sql).bind(id).fetch_one(pool).await?;
        Ok(c as usize)
    }
    let imms = sqlx::query_as::<_, Immunization>(
        "select * from immunizations where subject_id = $1",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let vaccines_due = match s.dob {
        Some(dob) => crate::peds::forecast(dob, &imms, crate::peds::today())
            .iter()
            .filter(|d| d.status != "upcoming")
            .count(),
        None => 0,
    };
    Ok(SnapshotCounts {
        problems: n(pool, "select count(*) from conditions where subject_id=$1 and status='active'", s.id).await?,
        medications: n(pool, "select count(*) from medications where subject_id=$1 and status='active'", s.id).await?,
        allergies: n(pool, "select count(*) from allergies where subject_id=$1 and status<>'entered_in_error'", s.id).await?,
        immunizations: imms.len(),
        vitals: n(pool, "select count(*) from observations where subject_id=$1", s.id).await?,
        appointments: n(pool, "select count(*) from appointments where subject_id=$1 and starts_at>=now()", s.id).await?,
        vaccines_due,
    })
}
