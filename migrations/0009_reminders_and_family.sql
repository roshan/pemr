-- personal-emr: Phase 3 — forward-looking + family graph (optional tier).
-- Design-of-record: docs/data-model-plan.md §Phase 3, KB:PEMR:data-model.

-- 3a. care_reminders — "what's due". Forecasting (ACIP schedule, well-child
-- cadence) is app logic built on top; this table just stores due items.
-- overdue is DERIVED (due_on < today), never stored.
create table care_reminders (
    id            uuid primary key,
    subject_id    uuid not null references subjects(id),
    title         text not null,                 -- "MMR dose 2", "12-month well visit"
    kind          text not null default 'other', -- vaccine | well_visit | screening | dental | med_refill | other
    due_on        date,
    status        text not null default 'due',   -- due | done | dismissed
    recommended_by uuid references providers(id),
    satisfied_by_appointment_id uuid references appointments(id),
    notes         text not null default '',
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now()
);
create index care_reminders_subject_due_idx on care_reminders (subject_id, due_on) where status = 'due';

-- 3b. subject_relationships — family graph / guardianship. Non-subject
-- emergency contacts are deferred (add a small contacts table only if needed).
create table subject_relationships (
    subject_id         uuid not null references subjects(id) on delete cascade,
    related_subject_id uuid not null references subjects(id) on delete cascade,
    relationship       text not null,            -- parent | guardian | child | sibling | spouse | emergency_contact | other
    notes              text not null default '',
    created_at         timestamptz not null default now(),
    primary key (subject_id, related_subject_id, relationship)
);
create index subject_relationships_related_idx on subject_relationships (related_subject_id);
