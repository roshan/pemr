# personal-emr — data-model expansion plan (DESIGN, not yet built)

> **Status:** design only. No migrations applied, no code written. This is the canonical
> model. **v2** — incorporates a three-model parallel review (Opus + Sonnet reviewers,
> Opus synthesizer). The goal is **getting the model right**; sync workers (MyChart
> `PEMR-5`, Apple Health `PEMR-4`) and forecasting logic are *built on top* of this
> model later and are explicitly out of scope here.

## Review outcome (what changed in v2)

Verified, applied fixes (were real): observations now use **`date + precision` (+ optional
timestamptz)** instead of `timestamptz not null` (the commonest import, well-child vitals,
is date-only); `providers` and `subject_identifiers` got the **full provenance 5-tuple**;
`providers.source_id` (provenance) was **split from `facility_id`** (workplace);
`observations` gained a **nullable `panel_id`** (group a CBC / newborn screen now, not via a
live backfill later) plus `search_tsv`; `immunizations`/`observations` can link to an
**`appointment_id`** (a routine checkup has no incident); `care_reminders.status` dropped the
**derived** `overdue`; `subject_providers` PK is now `(subject_id, provider_id)`.

Rejected after verification: the claim that `where status in (...)` is invalid Postgres
(it isn't — empirically `CREATE INDEX` succeeds on PG18, normalized to `= ANY(ARRAY[...])`).

## Context

Personal EMR for the George family (Roshan, Julie, Astra — a child — and later
grandparents). Data originates in external EMR portals (Epic MyChart at Stanford
Children's and Pacific Pediatrics, lab portals, the CA immunization registry).
**Not** a billing provider: no claims, no RCM, no e-prescribing, no clinical
decision support. Auth is Cloudflare Access at the edge; in-app policy is "every
authenticated viewer sees every subject."

First concrete driver: track Astra's appointments, her clinic (Pacific Pediatrics,
Castro St, 415-565-6810), and her pediatrician (Dr. Daniel Kelly). Real goal: the
**clinical core** that makes this more than a filing cabinet — structured
current-state lists, longitudinal observations, and identity reconciliation.

### What exists today

- `subjects` — patients (id, full_name, given/family, dob, sex_at_birth, blood_type, notes, cf_access_email).
- `sources` — orgs / portals / facilities (id, name, kind, base_url, notes). `kind ∈ {mychart, athena, quest, labcorp, hospital, clinic, insurance, manual, other}`.
- `incidents` — real-world events / episodes (subject_id, title, narrative, occurred_at date + precision, search_tsv). Provenance deliberately removed (0003): an incident spans many EMRs.
- `records` — documents/files (subject_id, kind, title, notes, file payload, DICOM fields, **source provenance**, search_tsv).
- `incident_records` — m:n join (composite PK).
- `api_keys` — read-only API auth.

### Repo conventions the model must respect

- Every **clinical** entity carries `subject_id uuid not null references subjects(id)`. Non-negotiable.
- **Reference data is NOT subject-scoped** (`sources`, and now `providers`).
- Externally-originating digital artifacts carry the provenance 5-tuple
  `source_id, external_id, external_url, source_synced_at, source_payload jsonb` + the
  partial unique index `(source_id, external_id) where both not null` for idempotent re-import.
- Free-text searchable entities get `search_tsv tsvector generated always as (...) stored` + GIN (`english` prose, `simple` tags/kinds).
- Status/kind vocabularies are **`text` + a Rust `const &[&str]`**, NOT Postgres enums.
- IDs are app-generated **uuid v7**; join tables use composite PKs, no surrogate.
- **Dates use `date` + a `*_precision` text** when time-of-day is unknown; true timestamps use `timestamptz`.
- Migrations are append-only `.sql` applied at boot via `sqlx::migrate!`.

## Design principles

1. **Generic where the shape is shared; specific where it differs.** Vitals and lab
   results are the same shape → **one `observations` table**. Allergies, medications,
   conditions, immunizations differ materially → **separate lean tables**.
2. **Uniform provenance + coding hooks = sync builds on top for free.** Every clinical row
   gets the 5-tuple + an optional standard code (`code` + `code_system`: CVX, RxNorm, LOINC,
   ICD-10/SNOMED). Not a terminology server — the code rides alongside the display string.
3. **Three orthogonal axes, kept distinct:** `incidents` = episode/thread (optional);
   `appointments` = calendar event w/ lifecycle; `conditions` = problem-list diagnosis. Plus
   the *facts* (observations, immunizations, meds, allergies) and the *documents* (records).
4. **Lean columns + a `source_payload jsonb` long-tail.** Promote to a real column when a
   real need appears.

---

## Phase 1 — Care-delivery layer (migration `0007`)

### 1a. Extend `sources` with facility contact

```sql
alter table sources add column phone   text;
alter table sources add column address text;   -- ONE free-text line, not street/city/state/zip
```

### 1b. `providers` — shared clinician directory (NO subject_id — reference data, like `sources`)

```sql
create table providers (
    id          uuid primary key,
    full_name   text not null,
    specialty   text,                          -- "Pediatrics" (free text)
    npi         text,                          -- National Provider ID: the global dedup key
    facility_id uuid references sources(id),   -- primary workplace clinic (distinct from provenance)
    phone       text,
    email       text,
    notes       text not null default '',
    -- provenance 5-tuple. source_id = the system this row was synced FROM (the portal),
    -- which is NOT the same as facility_id (where the provider physically works).
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
```

**Convention departure (documented):** no `subject_id` — a provider is shared reference data,
same category as `sources`. NPI is the global dedup key; when NPI is absent (common in
patient-facing MyChart exports) `(source_id, external_id)` carries dedup, and the same human
may legitimately exist as one row per source — accepted limitation.

### 1c. `subject_providers` — care team ("Dr. Kelly is *Astra's* PCP")

```sql
create table subject_providers (
    subject_id  uuid not null references subjects(id)  on delete cascade,
    provider_id uuid not null references providers(id) on delete cascade,
    role        text not null default 'care',   -- pcp | specialist | dentist | therapist | care | other
    active      boolean not null default true,
    since       date,
    notes       text not null default '',
    created_at  timestamptz not null default now(),
    primary key (subject_id, provider_id)        -- one membership per pair; role is an attribute
);
create index subject_providers_provider_idx on subject_providers (provider_id);
```

### 1d. `subject_identifiers` — cross-system identity reconciliation (the sync hook)

```sql
create table subject_identifiers (
    id          uuid primary key,
    subject_id  uuid not null references subjects(id) on delete cascade,
    source_id   uuid not null references sources(id),
    id_type     text not null default 'mrn',   -- mrn | member_id | cair_id | portal_login | other
    value       text not null,
    notes       text not null default '',
    -- provenance: when we last confirmed this identifier, + the raw payload
    source_synced_at timestamptz,
    source_payload   jsonb,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);
create unique index subject_identifiers_source_type_value_uk
    on subject_identifiers (source_id, id_type, value);   -- re-import keys on this (identifiers are stable)
create index subject_identifiers_subject_idx on subject_identifiers (subject_id);
```

### 1e. `appointments` — calendar events

```sql
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
```

`status='rescheduled'` deliberately dropped — rescheduling = editing `starts_at`, staying `scheduled`.

---

## Phase 2 — Clinical core (migration `0008`)

All five tables carry `subject_id not null`, the **full provenance 5-tuple**
(`source_id, external_id, external_url, source_synced_at, source_payload`), the
`(source_id, external_id)` partial unique index, `code`/`code_system`, and `created_at`/
`updated_at`. Shown explicitly as `‹provenance 5-tuple› ‹created_at/updated_at›` for brevity.
The partial unique handles **same-source** re-import; **cross-source** dedup (e.g. a vaccine
reported by both Stanford and CAIR2) is app logic on `code`+date.

```sql
-- positive "no known allergies" assertion (distinct from "no data imported")
alter table subjects add column no_known_allergies boolean not null default false;
```

### 2a. `allergies`

```sql
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
    -- ‹provenance 5-tuple› ‹created_at/updated_at›
    search_tsv  -- english(substance A, reaction B, notes C)
);
```

### 2b. `medications`

```sql
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
    -- ‹provenance 5-tuple› ‹created_at/updated_at›
    search_tsv  -- english(name A, reason B, notes C)
);
```

### 2c. `conditions` (problem list — distinct from incidents)

```sql
create table conditions (
    id              uuid primary key,
    subject_id      uuid not null references subjects(id),
    name            text not null,              -- display
    code            text,                        -- ICD-10 / SNOMED
    code_system     text,
    status          text not null default 'active', -- active | resolved | remission | entered_in_error
    onset_date      date,
    onset_precision text not null default 'day', -- chronic-condition onset is often fuzzy ("sometime 2023")
    resolved_date   date,
    severity        text,
    notes           text not null default '',
    -- ‹provenance 5-tuple› ‹created_at/updated_at›
    search_tsv  -- english(name A, notes B)
);
```

### 2d. `immunizations`

```sql
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
    -- ‹provenance 5-tuple› ‹created_at/updated_at›
    search_tsv  -- english(vaccine A, notes B)
);
```

### 2e. `observations` (vitals + discrete lab results — one table, FHIR `Observation` subset)

```sql
create table observations (
    id                  uuid primary key,
    subject_id          uuid not null references subjects(id),
    category            text not null default 'vital', -- vital | lab | measurement
    code                text,                            -- LOINC (height 8302-2, weight 29463-7, head-circ 9843-4, analytes)
    code_system         text,
    display             text not null,                  -- "Body weight", "Hemoglobin"
    value_num           numeric,                         -- the trendable number (growth charts, lab trends)
    value_text          text,                            -- non-numeric ("positive", "trace")
    unit                text,
    ref_low             numeric,
    ref_high            numeric,
    abnormal_flag       text,                            -- normal | high | low | abnormal
    effective_on        date not null,                  -- trendable anchor; date-only is the common case
    effective_precision text not null default 'day',
    effective_at        timestamptz,                    -- optional real wall-clock time when the source has it (Apple Health)
    panel_id            uuid,                            -- groups analytes from one draw/panel (CBC, newborn screen)
    record_id           uuid references records(id),    -- optional: the source lab-report document
    appointment_id      uuid references appointments(id), -- optional: the visit it was measured at
    incident_id         uuid references incidents(id),  -- optional: the episode
    notes               text not null default '',
    -- ‹provenance 5-tuple› ‹created_at/updated_at›
    search_tsv  tsvector generated always as (
        setweight(to_tsvector('english', coalesce(display, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(notes, '')),   'B')
    ) stored
);
create index observations_subject_code_idx on observations (subject_id, code, effective_on desc);
create index observations_panel_idx        on observations (panel_id) where panel_id is not null;
create index observations_search_idx       on observations using gin (search_tsv);
```

Powers **growth charts** (percentile math is app logic, built on top) and lab trending.
**Blood pressure** = two rows (systolic LOINC 8480-6, diastolic 8462-4), never `"120/80"` in
`value_text`. Growth tracking depends on height/weight/head-circ being **normalized to their
canonical LOINC codes at import**, or the same vital arrives as unjoinable display strings.

---

## Phase 3 — Forward-looking + family graph (migration `0009`) — optional tier

### 3a. `care_reminders` — "what's due"

Forecasting (ACIP schedule, well-child cadence) is **app logic built on top**; the table just
stores due items. `overdue` is **derived** (`due_on < today`), never stored.

```sql
create table care_reminders (
    id            uuid primary key,
    subject_id    uuid not null references subjects(id),
    title         text not null,                 -- "MMR dose 2", "12-month well visit"
    kind          text not null default 'other', -- vaccine | well_visit | screening | dental | med_refill | other
    due_on        date,
    status        text not null default 'due',   -- due | done | dismissed  (overdue = due_on < today, computed)
    recommended_by uuid references providers(id),
    satisfied_by_appointment_id uuid references appointments(id),
    notes         text not null default '',
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now()
);
create index care_reminders_subject_due_idx on care_reminders (subject_id, due_on) where status = 'due';
```

### 3b. `subject_relationships` — family graph / guardianship

```sql
create table subject_relationships (
    subject_id         uuid not null references subjects(id) on delete cascade,
    related_subject_id uuid not null references subjects(id) on delete cascade,
    relationship       text not null,            -- parent | guardian | child | sibling | spouse | emergency_contact | other
    notes              text not null default '',
    created_at         timestamptz not null default now(),
    primary key (subject_id, related_subject_id, relationship)
);
```

(Emergency contacts who are not subjects are deferred — add a small `contacts` table only if needed.)

---

## Integrity & import notes (decisions, not schema)

- **Delete policy (intentional asymmetry):** clinical fact tables reference `subjects(id)` with
  the default `NO ACTION`, so a subject with clinical history **cannot** be hard-deleted (good —
  no silent loss of a child's labs). The join/reference tables (`subject_providers`,
  `subject_identifiers`, `subject_relationships`) `ON DELETE CASCADE`.
- **Cross-source dedup keys (app layer):** vaccines = `(subject, CVX, occurred_at ± window)`;
  observations = `(subject, LOINC, effective_on)`. The DB partial-unique only dedups *re-import
  from the same source*.
- **Subject merge (future):** when sync creates a duplicate subject before `subject_identifiers`
  is populated, we'll need a `subjects.merged_into uuid` tombstone to redirect FKs. Not built now;
  noted so every clinical FK is mergeable later.
- **Vocabularies → Rust consts:** each new status/kind/role/category set lands in a
  `const &[&str]` in `models.rs` (allergy/med/condition/immunization/appointment/reminder status,
  + categories, `subject_provider` roles, `subject_identifier` id_types, relationship kinds).

## Deliberately EXCLUDED (decision, not oversight)

Billing / claims / EOBs / RCM, e-prescribing, order entry, clinical decision support, a
terminology server, note versioning/amendments. **Insurance/coverage** beyond a `member_id` in
`subject_identifiers` (a whole module — deferred). **Home acute tracking** (fever/symptom/med-given
log) — would hang off an incident; deferred. Street/city/state/zip decomposition. A separate FHIR
`Encounter` vs `Appointment` split.

## Migration phasing

- **0007** is independently shippable and answers the literal ask (appointments, Dr. Kelly, Pacific Pediatrics) + lays the reconciliation hook.
- **0008** is the clinical core and the import landing zone; `PEMR-3` and `PEMR-14` collapse into it.
- **0009** is additive polish.
- `PEMR-4` writes `observations`; `PEMR-5` writes everything and relies on `subject_identifiers` + the provenance/code hooks. Neither requires reshaping — the test of "getting the model right."

## Resolved decisions (post-review)

1. **One `observations` table** for vitals+labs, **+ nullable `panel_id` now** (not deferred). No specimen/panel tables.
2. **Always-current lists (allergies/meds/conditions) stay subject-level.** The event-like facts (immunizations, observations) get optional `appointment_id` **and** `incident_id`.
3. **Insurance member IDs live in `subject_identifiers`** (`id_type='member_id'`); no coverage table yet.
4. **`care_reminders` ships in 0009** (optional tier) with `status = due|done|dismissed`; overdue derived.
5. **`providers.npi` partial-unique kept**; `(source_id, external_id)` carries dedup when NPI is null.
6. **No structural pediatric gap** beyond NKDA (added) + the LOINC-normalization import assumption (documented). Growth percentiles, developmental milestones, newborn screening, and school-form export are app-layer or ride `observations`.
