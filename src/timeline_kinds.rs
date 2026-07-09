//! Registry of timeline event kinds — the one place that knows what a "kind"
//! is. Each entry owns its SQL union arm (which table/columns feed the event
//! stream), its click-through URL, its dot colour, and its user-facing label.
//! Registry order is both the legend order and the dot-colour priority when a
//! bucket mixes kinds. Callers (the timeline handler, the legend, the detail
//! rows) just iterate or look up — adding a timeline-visible entity is one
//! entry here. Mirrors the `subject_modules` / `subject_pages` registries.

use uuid::Uuid;

pub struct TimelineKind {
    /// The kind key as it appears in the event stream (and in SQL).
    pub key: &'static str,
    /// User-facing label ("incident" renders as "Event").
    pub label: &'static str,
    /// Dot/legend colour class. Standard Tailwind literals kept in Rust source
    /// so the CSS scanner picks them up.
    pub color: &'static str,
    /// Table the events come from.
    table: &'static str,
    /// SQL expression for the event date.
    date: &'static str,
    /// SQL expression for the event title.
    title: &'static str,
    /// SQL expression for the end date of a multi-day event; point-in-time
    /// kinds project `null::date` to keep the union column-compatible.
    end: &'static str,
    /// Guard `date` with `is not null` (skip for NOT NULL columns).
    date_nullable: bool,
    /// Where clicking the event goes.
    pub href: fn(id: &str, subject_id: Uuid) -> String,
}

/// Ordered registry: legend order AND bucket-dot priority (the first kind
/// present in a bucket colours its dot).
pub const KINDS: &[TimelineKind] = &[
    TimelineKind {
        key: "incident",
        label: "Event",
        color: "bg-rose-500",
        table: "incidents",
        date: "occurred_at",
        title: "title",
        end: "ended_at",
        date_nullable: true,
        href: |id, _| format!("/incidents/{id}"),
    },
    TimelineKind {
        key: "appointment",
        label: "Appointment",
        color: "bg-sky-500",
        table: "appointments",
        date: "starts_at::date",
        title: "title",
        end: "null::date",
        date_nullable: false,
        href: |id, _| format!("/appointments/{id}/edit"),
    },
    TimelineKind {
        key: "record",
        label: "Record",
        color: "bg-indigo-500",
        table: "records",
        date: "occurred_at",
        title: "title",
        end: "null::date",
        date_nullable: true,
        href: |id, _| format!("/records/{id}"),
    },
    TimelineKind {
        key: "condition",
        label: "Condition",
        color: "bg-amber-500",
        table: "conditions",
        date: "onset_date",
        title: "name",
        end: "null::date",
        date_nullable: true,
        href: |_, sid| format!("/subjects/{sid}"),
    },
    TimelineKind {
        key: "immunization",
        label: "Immunization",
        color: "bg-emerald-500",
        table: "immunizations",
        date: "occurred_at",
        title: "vaccine",
        end: "null::date",
        date_nullable: true,
        href: |_, sid| format!("/subjects/{sid}"),
    },
    TimelineKind {
        key: "observation",
        label: "Observation",
        color: "bg-slate-400",
        table: "observations",
        date: "effective_on",
        title: "display",
        end: "null::date",
        date_nullable: false,
        href: |_, sid| format!("/subjects/{sid}"),
    },
];

pub fn get(key: &str) -> Option<&'static TimelineKind> {
    KINDS.iter().find(|k| k.key == key)
}

/// Dot colour for a kind, with a safe fallback for unknown keys.
pub fn color(key: &str) -> &'static str {
    get(key).map_or("bg-slate-400", |k| k.color)
}

/// User-facing label for a kind, with a safe fallback for unknown keys.
pub fn label(key: &str) -> &'static str {
    get(key).map_or("Event", |k| k.label)
}

/// The union query behind the timeline: one arm per kind, oldest first. `$1`
/// is the optional subject filter. Columns: date, kind, title, id (text),
/// subject_id, end date.
pub fn events_sql() -> String {
    let arms: Vec<String> = KINDS
        .iter()
        .map(|k| {
            let guard = if k.date_nullable {
                format!("{} is not null and ", k.date)
            } else {
                String::new()
            };
            format!(
                "select {} as d, '{}' as kind, {} as t, id::text as i, subject_id as s, {} as e \
                   from {} where {}($1::uuid is null or subject_id = $1)",
                k.date, k.key, k.title, k.end, k.table, guard
            )
        })
        .collect();
    format!("{} order by d asc", arms.join(" union all "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique() {
        for (i, k) in KINDS.iter().enumerate() {
            assert!(
                KINDS.iter().skip(i + 1).all(|o| o.key != k.key),
                "duplicate timeline kind key: {}",
                k.key
            );
        }
    }

    #[test]
    fn union_has_one_arm_per_kind() {
        let sql = events_sql();
        assert_eq!(sql.matches("union all").count(), KINDS.len() - 1);
        for k in KINDS {
            assert!(sql.contains(&format!("'{}'", k.key)));
        }
    }
}
