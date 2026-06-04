-- personal-emr: Phase 2 — clinical core.
-- Design-of-record: docs/data-model-plan.md §Phase 2, KB:PEMR:data-model.
-- Every table carries subject_id not null, the provenance 5-tuple, the
-- (source_id, external_id) partial-unique (idempotent SAME-source re-import;
-- cross-source dedup is app logic on code+date), code/code_system, and audit.

-- Positive "no known allergies" assertion (distinct from "no data imported").
alter table subjects add column no_known_allergies boolean not null default false;

-- 2a. allergies
create table allergies (
    id          uuid primary key,
    subject_id  uuid not null references subjects(id),
    substance   text not null,                  -- display
    code        text,                            -- RxNorm / UNII
    code_system text,
    category    text,                            -- drug | food | environmental | other
    reaction    text,
    severity    text,                            -- mild | moderate | severe | unknown
    status      text not null default 'active', -- active | inactive | resolved | entered_in_error
    onset_date  date,
    noted_date  date,
    notes       text not null default '',
    source_id        uuid references sources(id),
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now(),
    search_tsv  tsvector generated always as (
        setweight(to_tsvector('english', coalesce(substance, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(reaction, '')),  'B') ||
        setweight(to_tsvector('english', coalesce(notes, '')),     'C')
    ) stored
);
create index allergies_subject_idx on allergies (subject_id);
create index allergies_search_idx  on allergies using gin (search_tsv);
create unique index allergies_source_external_uk
    on allergies (source_id, external_id) where source_id is not null and external_id is not null;

-- 2b. medications
create table medications (
    id            uuid primary key,
    subject_id    uuid not null references subjects(id),
    name          text not null,                -- display
    code          text,                          -- RxNorm
    code_system   text,
    dose          text,                          -- "5 mL", "250 mg" (free text)
    route         text,
    frequency     text,                          -- "twice daily" (free text; PRN folds in here)
    status        text not null default 'active', -- active | completed | stopped | on_hold | entered_in_error
    started_on    date,
    ended_on      date,
    reason        text,
    prescriber_id uuid references providers(id),
    notes         text not null default '',
    source_id        uuid references sources(id),
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),
    search_tsv    tsvector generated always as (
        setweight(to_tsvector('english', coalesce(name, '')),   'A') ||
        setweight(to_tsvector('english', coalesce(reason, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(notes, '')),  'C')
    ) stored
);
create index medications_subject_idx on medications (subject_id);
create index medications_search_idx  on medications using gin (search_tsv);
create unique index medications_source_external_uk
    on medications (source_id, external_id) where source_id is not null and external_id is not null;

-- 2c. conditions (problem list — DISTINCT from incidents)
create table conditions (
    id              uuid primary key,
    subject_id      uuid not null references subjects(id),
    name            text not null,              -- display
    code            text,                        -- ICD-10 / SNOMED
    code_system     text,
    status          text not null default 'active', -- active | resolved | remission | entered_in_error
    onset_date      date,
    onset_precision text not null default 'day', -- chronic-condition onset is often fuzzy
    resolved_date   date,
    severity        text,
    notes           text not null default '',
    source_id        uuid references sources(id),
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    search_tsv      tsvector generated always as (
        setweight(to_tsvector('english', coalesce(name, '')),  'A') ||
        setweight(to_tsvector('english', coalesce(notes, '')), 'B')
    ) stored
);
create index conditions_subject_idx on conditions (subject_id);
create index conditions_search_idx  on conditions using gin (search_tsv);
create unique index conditions_source_external_uk
    on conditions (source_id, external_id) where source_id is not null and external_id is not null;

-- 2d. immunizations
create table immunizations (
    id             uuid primary key,
    subject_id     uuid not null references subjects(id),
    vaccine        text not null,               -- display
    code           text,                         -- CVX
    code_system    text,
    occurred_at    date,                          -- point in time (a child's record may lack the exact date)
    dose_number    integer,
    lot_number     text,
    site           text,                          -- left deltoid...
    route          text,
    status         text not null default 'completed', -- completed | not_given | entered_in_error
    provider_id    uuid references providers(id),
    appointment_id uuid references appointments(id), -- optional: the visit it was given at
    incident_id    uuid references incidents(id),    -- optional: the episode
    notes          text not null default '',
    source_id        uuid references sources(id),
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at     timestamptz not null default now(),
    updated_at     timestamptz not null default now(),
    search_tsv     tsvector generated always as (
        setweight(to_tsvector('english', coalesce(vaccine, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(notes, '')),   'B')
    ) stored
);
create index immunizations_subject_idx on immunizations (subject_id, occurred_at desc nulls last);
create index immunizations_search_idx  on immunizations using gin (search_tsv);
create unique index immunizations_source_external_uk
    on immunizations (source_id, external_id) where source_id is not null and external_id is not null;

-- 2e. observations (vitals + discrete lab results — one table, FHIR Observation subset).
-- Powers growth charts (percentile math is app logic). BP = two rows (systolic
-- LOINC 8480-6, diastolic 8462-4), never "120/80" in value_text. Growth vitals
-- normalize to canonical LOINC at import (height 8302-2, weight 29463-7, head-circ 9843-4).
create table observations (
    id                  uuid primary key,
    subject_id          uuid not null references subjects(id),
    category            text not null default 'vital', -- vital | lab | measurement
    code                text,                            -- LOINC
    code_system         text,
    display             text not null,                  -- "Body weight", "Hemoglobin"
    value_num           numeric,                         -- the trendable number
    value_text          text,                            -- non-numeric ("positive", "trace")
    unit                text,
    ref_low             numeric,
    ref_high            numeric,
    abnormal_flag       text,                            -- normal | high | low | abnormal
    effective_on        date not null,                  -- trendable anchor; date-only is the common case
    effective_precision text not null default 'day',
    effective_at        timestamptz,                    -- optional real wall-clock time (Apple Health)
    panel_id            uuid,                            -- groups analytes from one draw/panel (CBC, newborn screen)
    record_id           uuid references records(id),    -- optional: the source lab-report document
    appointment_id      uuid references appointments(id), -- optional: the visit it was measured at
    incident_id         uuid references incidents(id),  -- optional: the episode
    notes               text not null default '',
    source_id        uuid references sources(id),
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at          timestamptz not null default now(),
    updated_at          timestamptz not null default now(),
    search_tsv  tsvector generated always as (
        setweight(to_tsvector('english', coalesce(display, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(notes, '')),   'B')
    ) stored
);
create index observations_subject_code_idx on observations (subject_id, code, effective_on desc);
create index observations_panel_idx        on observations (panel_id) where panel_id is not null;
create index observations_search_idx       on observations using gin (search_tsv);
create unique index observations_source_external_uk
    on observations (source_id, external_id) where source_id is not null and external_id is not null;
