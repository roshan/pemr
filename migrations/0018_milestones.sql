-- personal-emr: developmental milestone tracker (PEMR-35) + the per-subject
-- opt-in feature registry it mounts onto (PEMR-45).
--
-- Everything lives in the existing patient data model — no external datastore.

-- Gestational age at birth (weeks). Nullable: null = unknown → treated as term.
-- DOB already exists (subjects.dob). This single field is all the SILENT
-- corrected-age computation needs (see src/milestone_age.rs).
alter table subjects add column gestational_age_weeks smallint;

-- Per-subject feature enablement (PEMR-45). The catalogue of available features
-- is an IN-CODE registry (src/feature_registry.rs); this table only records which
-- ones a given subject has turned on. Removing a feature deletes its row (hides
-- the surface) but never touches the module's underlying data — disable, never
-- delete. `config` is reserved for future per-feature settings (unused today).
create table subject_features (
    id          uuid primary key,
    subject_id  uuid not null references subjects(id) on delete cascade,
    feature_key text not null,
    config      jsonb,
    enabled_at  timestamptz not null default now(),
    unique (subject_id, feature_key)
);
create index subject_features_subject_idx on subject_features (subject_id);

-- Milestone assessments. USER-ENTERED data, not imported artifacts → no
-- source_id/external_id/source_payload provenance 5-tuple.
--
-- Queryable, denormalized snapshot columns (per the PEMR-35 spec): domain +
-- expected_age come from the vendored dataset (src/milestones.rs) keyed by
-- milestone_key; age_basis_used + chronological_age_days are a POINT-IN-TIME
-- snapshot captured at entry — recomputing them later would give wrong answers if
-- DOB / gestational age is subsequently edited, so they live on the row. A plain
-- SQL query over this table answers all of the spec's fields without joining
-- app-embedded data:
--   child_id                  = subject_id
--   milestone_id              = milestone_key
--   domain                    = domain
--   expected_age              = expected_age_months
--   age_basis_used            = age_basis_used
--   chronological_age_at_entry= chronological_age_days
--   response                  = response
--   date_recorded             = answered_at
create table milestone_responses (
    id                  uuid primary key,
    subject_id          uuid not null references subjects(id),
    milestone_key       text not null,
    domain              text not null,   -- social_emotional | language | cognitive | movement
    expected_age_months smallint not null, -- the CDC checkpoint this milestone belongs to
    response            text not null check (response in ('yes','not_yet','no')),
    -- WHEN the behaviour was first observed (user-entered; drives the progress
    -- view). Null for not_yet/no.
    observed_on         date,
    -- The age basis in effect when this row was recorded (silent correction).
    age_basis_used      text not null check (age_basis_used in ('chronological','corrected')),
    -- Chronological age in DAYS at answered_at — integer to avoid the numeric/
    -- float8 dance; months = days / 30.4375.
    chronological_age_days integer not null,
    note                text,
    -- When the row was last recorded/updated (= the spec's date_recorded),
    -- distinct from observed_on.
    answered_at         timestamptz not null default now(),
    created_at          timestamptz not null default now(),
    -- One current answer per (subject, milestone): a re-mark upserts in place.
    unique (subject_id, milestone_key)
);
create index milestone_responses_subject_idx on milestone_responses (subject_id);
create index milestone_responses_domain_idx on milestone_responses (subject_id, domain);
