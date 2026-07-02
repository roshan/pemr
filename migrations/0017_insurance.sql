-- personal-emr: insurance coverage.
-- A family shares one insurance card, so the CARD is shared reference data
-- (like providers/sources: NO subject_id) and each person on it is linked via a
-- join table (like subject_providers). This mirrors FHIR Coverage: the plan is
-- the payor/product, the join is the per-beneficiary coverage.

-- insurance_plans — the card / policy. Reference data, NO subject_id: one row
-- can cover the whole family. Provenance 5-tuple for eventual sync (a payer IS a
-- source; insurance data can arrive from a portal).
create table insurance_plans (
    id              uuid primary key,
    payer_name      text not null,                 -- "Blue Shield of California"
    plan_name       text,                          -- "Silver PPO 2000" (product name)
    plan_type       text,                          -- ppo | hmo | epo | pos | hdhp | medicare | medicaid | tricare | other
    member_id       text,                          -- subscriber / member ID printed on the card
    group_number    text,                          -- employer group number
    subscriber_name text,                          -- policyholder as printed (may not be a tracked subject)
    plan_kind       text not null default 'medical', -- medical | dental | vision | pharmacy | other
    rx_bin          text,                          -- pharmacy routing: BIN
    rx_pcn          text,                          -- pharmacy routing: PCN
    rx_group        text,                          -- pharmacy routing: group
    payer_phone     text,                          -- member-services / claims phone
    effective_date  date,
    expiration_date date,
    notes           text not null default '',
    -- provenance 5-tuple. source_id = the portal/payer system this synced FROM.
    source_id        uuid references sources(id),
    external_id      text,
    external_url     text,
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    search_tsv      tsvector generated always as (
        setweight(to_tsvector('english', coalesce(payer_name, '')),  'A') ||
        setweight(to_tsvector('english', coalesce(plan_name, '')),   'B') ||
        setweight(to_tsvector('simple',  coalesce(member_id, '')),   'B') ||
        setweight(to_tsvector('simple',  coalesce(group_number, '')),'C') ||
        setweight(to_tsvector('english', coalesce(notes, '')),       'C')
    ) stored
);
create index insurance_plans_search_idx on insurance_plans using gin (search_tsv);
create unique index insurance_plans_source_external_uk
    on insurance_plans (source_id, external_id) where source_id is not null and external_id is not null;

-- subject_insurance — coverage link (subject ↔ plan). PK (subject_id, plan_id):
-- one coverage per pair; relationship + per-person member id are attributes.
-- Mirrors subject_providers.
create table subject_insurance (
    subject_id      uuid not null references subjects(id)        on delete cascade,
    plan_id         uuid not null references insurance_plans(id) on delete cascade,
    relationship    text not null default 'self',   -- self | spouse | child | dependent | other
    member_id       text,                           -- per-person / dependent id when it differs from the plan's
    is_primary      boolean not null default true,  -- primary coverage vs secondary
    effective_date  date,
    expiration_date date,
    notes           text not null default '',
    created_at      timestamptz not null default now(),
    primary key (subject_id, plan_id)
);
create index subject_insurance_plan_idx on subject_insurance (plan_id);
