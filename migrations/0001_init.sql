-- personal-emr: initial schema.
-- Conventions: every clinical entity carries `subject_id` (whose body) and
-- source provenance columns. See CLAUDE.md.

create extension if not exists "uuid-ossp";

-- the people the records are about. Seeded below with the initial three.
create table subjects (
    id              uuid primary key,
    full_name       text not null,
    given_name      text not null,
    family_name     text not null,
    dob             date,
    sex_at_birth    text,
    blood_type      text,
    notes           text not null default '',
    cf_access_email text,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now()
);
create unique index subjects_cf_access_email_uk
    on subjects (cf_access_email)
    where cf_access_email is not null;

-- where a record/incident came from. Manual entries can leave source_id null.
create table sources (
    id          uuid primary key,
    name        text not null,
    kind        text not null,
    base_url    text,
    notes       text not null default '',
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

create table incidents (
    id                  uuid primary key,
    subject_id          uuid not null references subjects(id),
    title               text not null,
    narrative           text not null default '',
    occurred_at         date,
    occurred_precision  text not null default 'day',
    source_id           uuid references sources(id),
    external_id         text,
    external_url        text,
    source_synced_at    timestamptz,
    source_payload      jsonb,
    created_at          timestamptz not null default now(),
    updated_at          timestamptz not null default now(),
    search_tsv          tsvector generated always as (
        setweight(to_tsvector('english', coalesce(title, '')),     'A') ||
        setweight(to_tsvector('english', coalesce(narrative, '')), 'B')
    ) stored
);
create index incidents_search_idx   on incidents using gin (search_tsv);
create index incidents_occurred_idx on incidents (occurred_at desc nulls last);
create index incidents_subject_idx  on incidents (subject_id, occurred_at desc nulls last);
create unique index incidents_source_external_uk
    on incidents (source_id, external_id)
    where source_id is not null and external_id is not null;

create table records (
    id                  uuid primary key,
    subject_id          uuid not null references subjects(id),
    kind                text not null,
    title               text not null,
    notes               text not null default '',
    occurred_at         date,
    occurred_precision  text not null default 'day',
    -- file payload (nullable; a "note" record may have no file)
    file_path           text,
    content_type        text,
    byte_size           bigint,
    sha256              text,
    -- provenance
    source_id           uuid references sources(id),
    external_id         text,
    external_url        text,
    source_synced_at    timestamptz,
    source_payload      jsonb,
    created_at          timestamptz not null default now(),
    updated_at          timestamptz not null default now(),
    search_tsv          tsvector generated always as (
        setweight(to_tsvector('english', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(notes, '')), 'B') ||
        setweight(to_tsvector('simple',  coalesce(kind,  '')), 'C')
    ) stored
);
create index records_search_idx   on records using gin (search_tsv);
create index records_kind_idx     on records (kind);
create index records_occurred_idx on records (occurred_at desc nulls last);
create index records_subject_idx  on records (subject_id, occurred_at desc nulls last);
create unique index records_source_external_uk
    on records (source_id, external_id)
    where source_id is not null and external_id is not null;

-- many-to-many between incidents and records.
create table incident_records (
    incident_id uuid not null references incidents(id) on delete cascade,
    record_id   uuid not null references records(id)   on delete cascade,
    note        text not null default '',
    created_at  timestamptz not null default now(),
    primary key (incident_id, record_id)
);
create index incident_records_record_idx on incident_records (record_id);

-- seed: Roshan, Julie, Astra. uuids are deterministic so re-running is a no-op.
insert into subjects (id, full_name, given_name, family_name, cf_access_email, notes)
values
  ('01970000-0000-7000-8000-000000000001'::uuid, 'Roshan George',  'Roshan', 'George',  'roshan@technologybrother.com', ''),
  ('01970000-0000-7000-8000-000000000002'::uuid, 'Julie Yu Kang',  'Julie',  'Yu Kang', null, ''),
  ('01970000-0000-7000-8000-000000000003'::uuid, 'Astra Meridian', 'Astra',  'Meridian', null, '')
on conflict (id) do nothing;
