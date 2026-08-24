-- personal-emr: enable the per-subject "growth" feature for Astra only (PEMR-47).
--
-- Growth charts (weight/height/head-circumference percentiles) are now a
-- per-subject opt-in module (`src/feature_registry.rs`, feature key "growth";
-- enablement stored in `subject_features`, migration 0018). Before this gate
-- existed they were unconditional on every subject.
--
-- Family decision (2026-08-24): ASTRA ONLY. No other subject gets auto-enrollment
-- — specifically NOT "every subject that has growth data" and NOT "all subjects".
-- Adults keep their charts clean; if growth is wanted elsewhere later it is added
-- per-subject the same way (the "+ Growth charts" picker on that subject's chart).
--
-- Astra's subject id is the deterministic seed UUID from 0001_init (and the one
-- linked on insurance). `on conflict` keeps re-running a no-op.
insert into subject_features (id, subject_id, feature_key)
select '01970000-0000-7000-8000-0000000000a3'::uuid, id, 'growth'
  from subjects
 where id = '01970000-0000-7000-8000-000000000003'::uuid
on conflict (subject_id, feature_key) do nothing;
