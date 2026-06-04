-- personal-emr: Phase 1 — care-delivery layer.
-- Design-of-record: docs/data-model-plan.md §Phase 1, KB:PEMR:data-model.
-- Delivers the literal "track appointments / Dr. Kelly / Pacific Pediatrics"
-- ask and lays the cross-system identity-reconciliation hook for sync.

-- 1a. A clinic IS a source: give sources facility contact fields.
alter table sources add column phone   text;
alter table sources add column address text;   -- ONE free-text line, not street/city/state/zip

-- 1b. providers — shared clinician directory. Reference data, NO subject_id
-- (same category as sources). NPI is the global dedup key; facility_id (where
-- the provider works) is DISTINCT from source_id (the system this row synced FROM).
create table providers (
    id          uuid primary key,
    full_name   text not null,
    specialty   text,                          -- "Pediatrics" (free text)
    npi         text,                          -- National Provider ID: the global dedup key
    facility_id uuid references sources(id),   -- primary workplace clinic (distinct from provenance)
    phone       text,
    email       text,
    notes       text not null default '',
    -- provenance 5-tuple. source_id = the portal this row was synced FROM,
    -- which is NOT facility_id (where the provider physically works).
    source_id        uuid references sources(id),
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now(),
    search_tsv  tsvector generated always as (
        setweight(to_tsvector('english', coalesce(full_name, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(specialty, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(notes, '')),     'C')
    ) stored
);
create index providers_search_idx on providers using gin (search_tsv);
create unique index providers_npi_uk on providers (npi) where npi is not null;
create unique index providers_source_external_uk
    on providers (source_id, external_id) where source_id is not null and external_id is not null;

-- 1c. subject_providers — care team ("Dr. Kelly is Astra's PCP").
-- PK (subject_id, provider_id): one membership per pair; role is an attribute.
create table subject_providers (
    subject_id  uuid not null references subjects(id)  on delete cascade,
    provider_id uuid not null references providers(id) on delete cascade,
    role        text not null default 'care',   -- pcp | specialist | dentist | therapist | care | other
    active      boolean not null default true,
    since       date,
    notes       text not null default '',
    created_at  timestamptz not null default now(),
    primary key (subject_id, provider_id)
);
create index subject_providers_provider_idx on subject_providers (provider_id);

-- 1d. subject_identifiers — cross-system identity reconciliation (THE sync hook).
-- Without it, sync duplicates subjects and misattributes records. Re-import keys
-- on (source_id, id_type, value) — identifiers are stable.
create table subject_identifiers (
    id          uuid primary key,
    subject_id  uuid not null references subjects(id) on delete cascade,
    source_id   uuid not null references sources(id),
    id_type     text not null default 'mrn',   -- mrn | member_id | cair_id | portal_login | other
    value       text not null,
    notes       text not null default '',
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);
create unique index subject_identifiers_source_type_value_uk
    on subject_identifiers (source_id, id_type, value);
create index subject_identifiers_subject_idx on subject_identifiers (subject_id);

-- 1e. appointments — calendar events with a status lifecycle.
create table appointments (
    id          uuid primary key,
    subject_id  uuid not null references subjects(id),
    provider_id uuid references providers(id),
    source_id   uuid references sources(id),
    incident_id uuid references incidents(id),   -- optional: the episode this visit belongs to
    starts_at   timestamptz not null,            -- when all_day=true, local-midnight; only the date is meaningful
    ends_at     timestamptz,
    all_day     boolean not null default false,  -- date-only / unknown-time (backfilled or "labs that day")
    status      text not null default 'scheduled', -- scheduled | completed | cancelled | no_show
    title       text not null,                    -- "18-month well-child visit"
    location    text,                             -- telehealth/room override; else facility address
    notes       text not null default '',
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now(),
    search_tsv  tsvector generated always as (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(notes, '')), 'B')
    ) stored
);
create index appointments_subject_idx  on appointments (subject_id, starts_at desc);
create index appointments_status_idx   on appointments (status);
create index appointments_provider_idx on appointments (provider_id) where provider_id is not null;
create index appointments_search_idx   on appointments using gin (search_tsv);
create unique index appointments_source_external_uk
    on appointments (source_id, external_id) where source_id is not null and external_id is not null;
