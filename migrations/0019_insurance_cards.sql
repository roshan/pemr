-- personal-emr: scanned insurance card images.
--
-- The card image is the thing you actually need at a clinic desk, and it is a
-- *file*, not a column on the policy: a card has two sides, and cards get
-- reissued (new member id, new plan year) while the policy row stays. So this
-- mirrors the records/DICOM pattern -- content-addressed bytes under FILES_DIR
-- plus a webp thumbnail -- rather than stuffing a path onto insurance_plans.
--
-- "Current" is the whole point of the feature (PEMR-51), so it is modelled
-- explicitly: superseded_at is null for the live card, set when a replacement
-- is uploaded. The partial unique index below makes "the current front of this
-- plan" unambiguous at the database level, so retrieval never has to guess.
create table insurance_cards (
    id           uuid primary key,
    plan_id      uuid not null references insurance_plans(id) on delete cascade,
    side         text not null default 'front',   -- front | back
    -- bytes, stored exactly like a record's file (content-addressed, deduped)
    file_path    text not null,
    content_type text,
    byte_size    bigint,
    sha256       text,
    thumbnail_path         text,
    thumbnail_content_type text,
    effective_date date,                          -- when this card version took effect
    superseded_at  timestamptz,                   -- null = this is the current card
    notes        text not null default '',
    created_at   timestamptz not null default now(),
    updated_at   timestamptz not null default now()
);
create index insurance_cards_plan_idx on insurance_cards (plan_id);

-- At most one CURRENT card per (plan, side). Superseded rows are unconstrained,
-- so history accumulates freely.
create unique index insurance_cards_current_side_uk
    on insurance_cards (plan_id, side) where superseded_at is null;
