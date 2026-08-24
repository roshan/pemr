-- personal-emr: enable the per-subject "allergies" feature where data exists (PEMR-47 pattern).
--
-- The allergy card is now a per-subject opt-in module (`src/feature_registry.rs`,
-- feature key "allergies"; enablement in `subject_features`, migration 0018).
-- Before this gate existed the card was unconditional.
--
-- Backfill: enable for every subject that currently has >=1 allergy row
-- (status != entered_in_error), so no one with recorded allergies loses the card.
-- Subjects with no allergy data get a clean skin and can opt in from the chart's
-- "Add feature" picker. This mirrors the growth (0020) don't-regress principle.
insert into subject_features (id, subject_id, feature_key)
select gen_random_uuid(), a.subject_id, 'allergies'
  from (select distinct subject_id from allergies where status <> 'entered_in_error') a
on conflict (subject_id, feature_key) do nothing;
