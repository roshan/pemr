//! CDC "Learn the Signs. Act Early." (LTSAE) developmental milestones, 2022
//! revision — vendored as public-domain US-federal data (mirrors the growth-chart
//! precedent in [`crate::growth_ref`]). Source: the CDC LTSAE milestone
//! checklists, cdc.gov/act-early/milestones (US federal, public domain). A
//! milestone is a behaviour ~75% of children exhibit by the given age.
//!
//! The dataset (`peds_data/cdc_ltsae_milestones_2022.tsv`) is embedded at compile
//! time. 159 milestones across the 12 well-visit checkpoints (2mo → 5y) and four
//! domains. No AAP-branded media, no commercial photos/videos — CDC text only.
//!
//! TSV columns: `milestone_key`, `checkpoint_months`, `domain`, `text`.
//! `milestone_key` is a stable id (`{domaincode}-{months:02}-{seq}`) that keys
//! `milestone_responses`; it never changes for this vendored snapshot.

use std::sync::OnceLock;

const DATA: &str = include_str!("peds_data/cdc_ltsae_milestones_2022.tsv");

/// The CDC well-visit checkpoints (ages in months), in order. Every checklist is
/// pinned to one of these.
pub const CHECKPOINTS: &[i32] = &[2, 4, 6, 9, 12, 15, 18, 24, 30, 36, 48, 60];

/// The four milestone domains: `(key, display label)`, in CDC display order.
/// The `key` is what's stored in `milestone_responses.domain`.
pub const DOMAINS: &[(&str, &str)] = &[
    ("social_emotional", "Social/Emotional"),
    ("language", "Language/Communication"),
    ("cognitive", "Cognitive"),
    ("movement", "Movement/Physical Development"),
];

/// Compact domain label for dense layouts (the band view's row gutter), keyed
/// by the same `DOMAINS` key. Unknown keys fall through to a generic label.
pub fn domain_short(domain: &str) -> &'static str {
    match domain {
        "social_emotional" => "Social",
        "language" => "Language",
        "cognitive" => "Cognitive",
        "movement" => "Movement",
        _ => "Other",
    }
}

/// One vendored milestone. All fields borrow from the embedded `'static` TSV.
#[derive(Debug, Clone, Copy)]
pub struct Milestone {
    pub key: &'static str,
    pub checkpoint_months: i32,
    /// One of the `DOMAINS` keys.
    pub domain: &'static str,
    pub text: &'static str,
}

fn all() -> &'static [Milestone] {
    static PARSED: OnceLock<Vec<Milestone>> = OnceLock::new();
    PARSED.get_or_init(|| {
        DATA.lines()
            .skip(1) // header
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let mut c = line.splitn(4, '\t');
                let key = c.next().expect("milestone_key");
                let months = c.next().expect("checkpoint_months");
                let domain = c.next().expect("domain");
                let text = c.next().expect("text");
                Milestone {
                    key,
                    checkpoint_months: months.parse().expect("checkpoint_months is an int"),
                    domain,
                    text,
                }
            })
            .collect()
    })
}

/// Display order index for a domain key (for stable sorting); unknown → last.
fn domain_order(domain: &str) -> usize {
    DOMAINS
        .iter()
        .position(|(k, _)| *k == domain)
        .unwrap_or(DOMAINS.len())
}

/// Every milestone for a checkpoint, grouped by domain in CDC display order.
/// Returns `(domain_key, domain_label, milestones)` for each of the four domains
/// that has at least one milestone at this checkpoint.
pub fn by_checkpoint_grouped(months: i32) -> Vec<(&'static str, &'static str, Vec<Milestone>)> {
    DOMAINS
        .iter()
        .filter_map(|(key, label)| {
            let items: Vec<Milestone> = all()
                .iter()
                .copied()
                .filter(|m| m.checkpoint_months == months && m.domain == *key)
                .collect();
            if items.is_empty() {
                None
            } else {
                Some((*key, *label, items))
            }
        })
        .collect()
}

/// Flat list of a checkpoint's milestones, ordered by domain then sequence.
pub fn by_checkpoint(months: i32) -> Vec<Milestone> {
    let mut items: Vec<Milestone> = all()
        .iter()
        .copied()
        .filter(|m| m.checkpoint_months == months)
        .collect();
    items.sort_by_key(|m| (domain_order(m.domain), m.key));
    items
}

/// Look up a single milestone by its stable key — used when persisting a response
/// to denormalise `domain` + `expected_age` onto the row.
pub fn by_key(key: &str) -> Option<Milestone> {
    all().iter().copied().find(|m| m.key == key)
}

/// Allowed milestone response values (matches the DB CHECK on
/// `milestone_responses.response`).
pub const RESPONSES: &[&str] = &["yes", "not_yet", "no"];

/// Human label for a response value.
pub fn response_label(r: &str) -> &'static str {
    match r {
        "yes" => "Yes",
        "not_yet" => "Not yet",
        "no" => "No",
        _ => "—",
    }
}

/// What the milestone ages mean. The 2022 LTSAE revision moved from the 50th
/// percentile (the average age) to the **75th**: each item is a behaviour 75% or
/// more of children show by that age (Zubler et al., Pediatrics
/// 2022;149(3):e2021052138). There is no 90th-percentile companion — CDC
/// publishes one threshold per milestone; per-item percentile bands are the
/// structure of proprietary instruments (Denver II), not of LTSAE. Don't
/// synthesise one.
pub const PERCENTILE_BASIS: &str = "Each CDC milestone is a behaviour 75% or more of children \
    show by the listed age (2022 revision; previously the 50th percentile).";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_loads_completely() {
        // The published 2022 revision has 159 milestones.
        assert_eq!(all().len(), 159);
        // Every row parsed with a known checkpoint + domain.
        for m in all() {
            assert!(CHECKPOINTS.contains(&m.checkpoint_months), "bad checkpoint {}", m.checkpoint_months);
            assert!(DOMAINS.iter().any(|(k, _)| *k == m.domain), "bad domain {}", m.domain);
            assert!(!m.text.is_empty());
            assert!(!m.key.is_empty());
        }
    }

    #[test]
    fn every_checkpoint_has_all_four_domains() {
        for &mo in CHECKPOINTS {
            let groups = by_checkpoint_grouped(mo);
            assert_eq!(groups.len(), 4, "checkpoint {mo} should have 4 domains");
        }
    }

    #[test]
    fn every_domain_has_a_short_label() {
        for (k, _) in DOMAINS {
            assert_ne!(domain_short(k), "Other", "no short label for {k}");
        }
        assert_eq!(domain_short("nope"), "Other");
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<&str> = all().iter().map(|m| m.key).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "milestone keys must be unique");
    }

    #[test]
    fn by_key_roundtrips_domain_and_checkpoint() {
        let m = by_key("se-02-1").expect("se-02-1 exists");
        assert_eq!(m.checkpoint_months, 2);
        assert_eq!(m.domain, "social_emotional");
        assert!(by_key("does-not-exist").is_none());
    }
}
