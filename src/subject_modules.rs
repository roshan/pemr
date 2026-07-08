//! Subject-page modules. Each is a **self-contained card**: it fetches its own
//! data and renders itself, or returns `None` to opt out (not applicable to this
//! subject). The subject chart just iterates [`MODULES`] — it has *no* per-feature
//! knowledge, so adding a subfeature is one fn here plus one line in the registry.
//! Mirrors the `sync::TaskDef` function-pointer registry already used in the app.

use std::future::Future;
use std::pin::Pin;

use maud::{Markup, html};
use sqlx::PgPool;

use crate::models::{
    Allergy, Appointment, CareTeamMember, Condition, Immunization, InsuranceCoverageRow,
    Medication, Subject, VitalRow,
};
use crate::views::components as c;

type BoxFut<'a> = Pin<Box<dyn Future<Output = Result<Option<Markup>, sqlx::Error>> + Send + 'a>>;

/// A subject-page module: `(pool, subject) -> optional card`.
pub type Module = for<'a> fn(&'a PgPool, &'a Subject) -> BoxFut<'a>;

/// Ordered registry = the render order on the chart. Add a subfeature = add a line.
pub const MODULES: &[Module] = &[
    problems,
    medications,
    allergies,
    vitals,
    growth,
    immunizations,
    appointments,
    care_team,
    insurance,
];

/// Render every applicable module's card, in registry order.
pub async fn render_all(pool: &PgPool, s: &Subject) -> Result<Vec<Markup>, sqlx::Error> {
    let mut cards = Vec::new();
    for m in MODULES {
        if let Some(card) = m(pool, s).await? {
            cards.push(card);
        }
    }
    Ok(cards)
}

// ── shared render helpers (were private in views::subject) ──────────────────
const CARD_MAX: usize = 4;

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }
}

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

// ── modules ─────────────────────────────────────────────────────────────────

fn problems<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Condition>(
            "select * from conditions where subject_id = $1 and status = 'active'
              order by onset_date desc nulls last, created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        Ok(Some(c::summary_panel("Problems", if rows.is_empty() {
            c::empty_state("No active problems")
        } else {
            truncated_list(rows.iter().map(|x| c::panel_list_item(
                html! { (x.name) },
                html! { @if let Some(d) = x.onset_date { "since " (d) } },
            )), rows.len(), None)
        })))
    })
}

fn medications<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Medication>(
            "select * from medications where subject_id = $1 and status = 'active'
              order by created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        Ok(Some(c::summary_panel("Medications", if rows.is_empty() {
            c::empty_state("No active medications")
        } else {
            truncated_list(rows.iter().map(|m| c::panel_list_item(
                html! { (m.name) },
                html! {
                    @if let Some(dose) = &m.dose { (dose) }
                    @if let Some(freq) = &m.frequency { " · " (freq) }
                },
            )), rows.len(), None)
        })))
    })
}

fn allergies<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Allergy>(
            "select * from allergies where subject_id = $1 and status <> 'entered_in_error'
              order by created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        Ok(Some(c::summary_panel("Allergies", if rows.is_empty() {
            if s.no_known_allergies {
                c::empty_state("No known allergies (asserted)")
            } else {
                c::empty_state("No allergies recorded")
            }
        } else {
            truncated_list(rows.iter().map(|a| c::panel_list_item(
                html! { (a.substance) },
                html! {
                    @if let Some(crit) = &a.criticality { (crit) }
                    @else if let Some(sev) = &a.severity { (sev) }
                },
            )), rows.len(), None)
        })))
    })
}

fn vitals<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, VitalRow>(
            "select display, value_num::float8 as value_num, value_text, unit, effective_on, abnormal_flag
               from observations where subject_id = $1
              order by effective_on desc, created_at desc limit 8",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        Ok(Some(c::summary_panel("Recent vitals & labs", if rows.is_empty() {
            c::empty_state("No vitals or labs recorded")
        } else {
            truncated_list(rows.iter().map(|v| {
                let val = v.value_num.map(fmt_num)
                    .or_else(|| v.value_text.clone())
                    .unwrap_or_else(|| "—".into());
                c::panel_list_item(
                    html! {
                        (v.display)
                        @if let Some(f) = &v.abnormal_flag {
                            @if f != "normal" { " " (c::badge_warn(f)) }
                        }
                    },
                    html! { (val) @if let Some(u) = &v.unit { " " (u) } " · " (v.effective_on) },
                )
            }), rows.len(), None)
        })))
    })
}

fn growth<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
        let series = crate::handlers::subjects::growth_series(pool, s).await?;
        // Opt out when there's nothing to plot — growth only applies to subjects
        // with measurements (typically infants/children).
        if series.iter().all(|g| g.points.is_empty()) {
            return Ok(None);
        }
        Ok(Some(crate::views::growth::mini_card(s.id, &series)))
    })
}

fn immunizations<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Immunization>(
            "select * from immunizations where subject_id = $1
              order by occurred_at desc nulls last, created_at desc",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        let due = match s.dob {
            Some(dob) => crate::peds::forecast(dob, &rows, crate::peds::today())
                .iter()
                .filter(|d| d.status != "upcoming")
                .count(),
            None => 0,
        };
        let href = format!("/subjects/{}/immunizations", s.id);
        Ok(Some(c::summary_panel_linked(
            html! { "Immunizations" @if due > 0 { " " (c::badge_warn(format!("{due} due"))) } },
            href.clone(),
            if rows.is_empty() {
                c::empty_state("No immunizations recorded")
            } else {
                truncated_list(rows.iter().map(|im| c::panel_list_item(
                    html! { (im.vaccine) },
                    html! { @if let Some(d) = im.occurred_at { (d) } },
                )), rows.len(), Some(href))
            },
        )))
    })
}

fn appointments<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
        let rows = sqlx::query_as::<_, Appointment>(
            "select * from appointments where subject_id = $1 and starts_at >= now()
              order by starts_at asc limit 6",
        )
        .bind(s.id)
        .fetch_all(pool)
        .await?;
        let href = format!("/subjects/{}/appointments", s.id);
        Ok(Some(c::summary_panel_linked("Upcoming appointments", href.clone(),
            if rows.is_empty() {
                c::empty_state("None scheduled")
            } else {
                truncated_list(rows.iter().map(|ap| c::panel_list_item(
                    html! { (ap.title) },
                    html! { (ap.starts_at.date()) },
                )), rows.len(), Some(href))
            },
        )))
    })
}

fn care_team<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
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
        let href = format!("/subjects/{}/care-team", s.id);
        Ok(Some(c::summary_panel_linked("Care team", href.clone(),
            if rows.is_empty() {
                c::empty_state("No care team recorded")
            } else {
                truncated_list(rows.iter().map(|m| c::panel_list_item(
                    html! { (m.full_name) @if let Some(sp) = &m.specialty { " " (c::muted(sp)) } },
                    html! { (m.role) },
                )), rows.len(), Some(href))
            },
        )))
    })
}

fn insurance<'a>(pool: &'a PgPool, s: &'a Subject) -> BoxFut<'a> {
    Box::pin(async move {
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
        Ok(Some(c::summary_panel_linked("Insurance", "/insurance",
            if rows.is_empty() {
                c::empty_state("No insurance recorded")
            } else {
                truncated_list(rows.iter().map(|ins| c::panel_list_item(
                    html! { (ins.payer_name) @if let Some(pn) = &ins.plan_name { " " (c::muted(pn)) } },
                    html! {
                        @if let Some(m) = &ins.member_id { (m) " · " }
                        (ins.plan_kind) " · " (ins.relationship)
                    },
                )), rows.len(), Some("/insurance".to_string()))
            },
        )))
    })
}
