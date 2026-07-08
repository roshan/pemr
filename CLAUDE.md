# personal-emr — repo conventions

> **Read this file before adding a feature.** It encodes the constraints we have already paid for. Diverging from any rule here without an explicit conversation is the wrong move.

## Project

Personal EMR for the George family — Roshan, Julie, Astra, and (later) parents. Records *incidents* and *records* (X-rays, labs, doctor's notes, etc.), each tied to a `subject_id`. Most data originates in external EMR portals, so every clinical row also carries source provenance for an eventual sync workflow.

Deployed to **kant** as a Quadlet at `127.0.0.1:8100`, fronted by Cloudflare Tunnel + Cloudflare Access at **`emr.roshangeorge.dev`**. There is **no in-app authentication**: Cloudflare Access decides who's let in, and the in-app policy is "every authenticated viewer sees everything."

The runtime contract on kant (ports, volumes, secrets convention, Tunnel routing) is documented at `kant:~/docs/README.md`. **Any change here that alters that contract must be paired with an edit there**, via the scp/git workflow that file describes.

Durable project knowledge — overview, API reference, anything an agent would want to read *without* cloning the repo — lives in the **`PEMR` Taskmaster KB container** (`KB:PEMR:overview`, `KB:PEMR:api`, …). **Any big change to architecture, auth, deploy shape, schema rules, or the API surface must update the matching KB doc in the same PR** (use the `taskmaster-kb` skill or `taskmaster_kb.py upsert --container PEMR --path <slug>`). Add a new doc when a new concern emerges (e.g. a sync workflow gets its own `KB:PEMR:sync`). Small fixes/refactors do not need a KB update — judgment call, but err toward updating when in doubt.

## Stack — and why

| Choice | Reason |
|---|---|
| **Rust** edition 2024 | Low supply-chain attack surface (lockfile, no install-time scripts beyond `build.rs`); fast at runtime. |
| **axum** + tokio + tower-http | Pragmatic, well-maintained, plays nicely with maud and sqlx. |
| **sqlx** (no macros) | Compile-time SQL checking is great in theory, but requires either a live DB or committed `.sqlx/` offline metadata. We use plain `sqlx::query`/`query_as` so a `cargo build` works on any machine without ceremony. |
| **maud** | Compile-time HTML templates — partials are plain Rust functions returning `Markup`. LLM-friendly because a partial is one contiguous template. |
| **htmx** (vendored) | Server-rendered HTML, no JS toolchain, no bundler, no React. HTMX swaps mean every endpoint can be inspected as plain HTML. |
| **Tailwind v4 standalone CLI** | The official `tailwindcss` single-binary CLI (Go-built; **no npm, no install scripts**). Compiles `tailwind.css` + class names scanned from `src/**/*.rs` into `static/vendor/app.css`. Run via `mise run css` (auto-installed into `bin/`, gitignored). The compiled CSS **is committed** so the Dockerfile doesn't need the CLI. |
| **uuid v7** | Sortable IDs, no separate `created_at` index needed for time-order. |
| **time** (not chrono) | Smaller surface, no `time-old` baggage. |

## Supply-chain rules

1. **No new dependency without an entry in this file** — crate name, why we picked it, anything notable about its `build.rs` if it has one. Add the row in the table below.
2. **Vendored frontend assets must record their SHA-256** in the table below. Bumping a vendored asset means downloading the new file, replacing it, and updating the hash in the same commit.
3. **No `build.rs` shenanigans** beyond `cargo:rerun-if-changed`. If a transitive dep has an unusual `build.rs` (network access, downloads, code generation from non-local sources), call it out before merging.
4. **No CDN-loaded assets at runtime.** All JS and CSS are served from `static/vendor/`.

### Direct Rust dependencies

| Crate | Purpose | Notes |
|---|---|---|
| axum | HTTP server | tokio-rs, mature |
| tokio | async runtime | core ecosystem |
| tower / tower-http | middleware (fs, trace, compression, limit, normalize-path) | `normalize-path` trims trailing slashes app-wide (see `main.rs`); core ecosystem |
| sqlx | Postgres client | runtime queries (no `query!` macros) |
| maud | HTML templates | compile-time, no runtime template compilation |
| serde / serde_json | (de)serialization, jsonb column | core ecosystem |
| uuid | v7 IDs (+ v4 for API token randomness) | features = ["v4","v7","serde"] — v4 is used only by `api_auth::generate_token` to source 32 bytes from the OS CSPRNG |
| time | timestamps | features = ["serde","serde-human-readable","serde-well-known","formatting","parsing","macros"]. `Date` serializes as ISO 8601 (`1988-01-28`) out of the box. `OffsetDateTime` does NOT: `serde-human-readable` alone emits time's native format (`2026-05-08 16:34:17 +00:00:00`), which is not RFC3339 and rejects RFC3339 input. So every `OffsetDateTime`/`Option<OffsetDateTime>` field on the `/api/v1` surface (models + request bodies) is tagged `#[serde(with = "time::serde::rfc3339")]` / `…::rfc3339::option` (needs `serde-well-known`) → the API reads and writes RFC3339. New timestamp fields MUST carry that attribute. |
| sha2 / hex | content-addressed file storage | RustCrypto |
| bytes | streaming buffers | core ecosystem |
| mime_guess | Content-Type from extension | small, well-known |
| thiserror | error enums | small, well-known |
| tracing / tracing-subscriber | structured logs to journald | core ecosystem |
| futures-util | stream adapters | core ecosystem |
| tokio-util | ReaderStream for sendfile-style responses | core ecosystem |
| dicom-object | DICOM parser | pure Rust, no FFI |
| dicom-dictionary-std | DICOM tag constants | pure Rust |
| dicom-pixeldata | DICOM pixel data decoder | features = ["image","jpeg","rayon"]; pure Rust JPEG baseline + lossless |
| image | PNG encoder | features = ["png"] only; everything else off |
| roxmltree | read-only XML DOM for the C-CDA importer | pure Rust, no FFI/build.rs; used by `importer.rs` to parse MyChart C-CDA exports |
| reqwest | async HTTP client for background sync tasks | features = ["rustls-tls","json","gzip"]; no native-tls/openssl. Used by `sync/vaccine.rs` to fetch CDPH SMART Health Card download URLs. |
| flate2 | DEFLATE decompression | pure Rust (miniz_oxide backend); no FFI. Required by SHC JWS payload format (SMART Health Card §4 mandates DEFLATE-compressed FHIR bundle). |
| base64 | base64url decoding | pure Rust; used to decode JWS header/payload segments in `sync/vaccine.rs`. |

### Vendored frontend assets

| File | Source | Version / SHA-256 |
|---|---|---|
| `static/vendor/htmx.min.js` | unpkg | 2.0.4 — `e209dda5c8235479f3166defc7750e1dbcd5a5c1808b7792fc2e6733768fb447` |
| `static/vendor/app.css` | compiled by `bin/tailwindcss` from `tailwind.css` | regenerate via `mise run css`; do not hand-edit |
| `bin/tailwindcss` | tailwindlabs GitHub release (gitignored) | v4.2.4 (macos-arm64) — `932f7045205283f4b26f9a4c3f027958526bf5bcc8577a7e2f18002e1eb5145e` |

### Vendored data

| File | Source | Notes |
|---|---|---|
| `src/peds_data/cdc_{weight,length,headcirc}_for_age_0_36mo.csv` | CDC growth-chart LMS data files (`cdc.gov/growthcharts/data/zscore/{wtageinf,lenageinf,hcageinf}.csv`) | Public domain (US gov). `include_str!`-embedded by `growth_ref.rs` for growth percentile bands. CDC 0–36 mo infant charts (carry precomputed P3–P97 columns). |

## DICOM import

`POST /records/import` accepts a multi-file multipart upload (the form uses `<input webkitdirectory>` so the user picks the parent folder of a Sutter / Lexmark export). For each uploaded file:

1. Bytes 128..132 are checked for the `DICM` magic; non-DICOM files are skipped silently (count returned as `skipped_non_dicom`).
2. The DICOM is parsed with `dicom-object`. Failed parses → skipped.
3. Pixel data is decoded via `dicom-pixeldata` and written to a PNG. If the DICOM has no decodable pixel data (Structured Reports, Presentation States, etc.), the file is skipped and counted as `skipped_no_pixels`.
4. The original `.dcm` and the rendered `.png` are stored content-addressed under `FILES_DIR`; one record is created per renderable image with metadata derived from DICOM tags (StudyDate → `occurred_at`, Modality → `kind`, BodyPart + ViewPosition → `title`, SOPInstanceUID → `external_id` for idempotency on re-import).
5. The detail-page viewer (`views::record::file_viewer`) prefers `preview_path` over `file_path`, so DICOM records render inline as PNG; the original `.dcm` is still served from `/records/:id/file` for download.

Supported transfer syntaxes (via `dicom-pixeldata` + the `jpeg` feature): Implicit/Explicit VR Little Endian, JPEG Baseline 8-bit (`1.2.840.10008.1.2.4.50`), JPEG Lossless First-Order Prediction (`.70`) — which is what Sutter/PACSGEAR exports use. JPEG-2000 (`.90/.91`) is **not** enabled because it would require a C dependency. If a future export uses an unsupported syntax the original `.dcm` still lands in storage, just without a preview.

### Reports vs primary images

A DICOM file is classified as `kind = 'report'` (not `xray`/`ct`/etc.) when any of:

- `SeriesDescription` contains "scan" / "report" / "document" (case-insensitive)
- `SOPClassUID` is Secondary Capture (`1.2.840.10008.5.1.4.1.1.7[.*]`), Structured Report (`1.2.840.10008.5.1.4.1.1.88.*`), or Encapsulated PDF/CDA (`.104.1` / `.104.2`)
- `Modality` is `SR` or `DOC`

Otherwise the kind comes from `Modality` (CR/DX/XR/RG/RF/PX → xray, CT → ct, MR → mri, US → ultrasound).

### Studies (no separate table — yet)

Every DICOM file's `StudyInstanceUID` is stored on the record (`records.study_instance_uid`). Records with the same UID belong to the same imaging event (e.g. an X-ray series + its scanned report). On the incident detail page, linked records are grouped by this UID into "study blocks" — image-kind records render as a thumbnail strip, reports listed below as compact attachments. Records without a UID (manually-uploaded photos, lab PDFs) get their own "Other" group.

If we ever want to attach study-level metadata (radiologist's overall impression, accession number, etc.) we can promote `study_instance_uid` into a real `studies` table. For now the column suffices.

### Thumbnails

Every record whose primary file (or DICOM-rendered preview) is an image gets a `thumbnail_path` pointing at a content-addressed WebP under `FILES_DIR`. Generated at create-time (`POST /records` for direct image uploads) and at import-time (`POST /records/import` for DICOM). The thumbnail is fixed at max 400px on the long side; the original `file_path` and the full-resolution `preview_path` PNG (DICOMs only) remain untouched. The image-kind record list (`models::IMAGE_RECORD_KINDS` = `xray`, `mri`, `ct`, `ultrasound`, `photo`) is what governs whether a record renders as a tile vs. a list row on the incident detail page.

### DICOM rendering pipeline (VOI LUT)

DICOM previews + thumbnails are rendered with **Modality LUT + VOI LUT applied** (`dicom_pixeldata::ConvertOptions::with_voi_lut(VoiLutOption::Default)`). VOI LUT is the radiologist's `WindowCenter` / `WindowWidth` mapping baked into the file; without it, the 16-bit pixel data gets a naive linear rescale and X-rays come out washed-out or too dark compared to what Sutter shows. If we ever surface a "reset window" / "auto-window" UI control, the alternative is `VoiLutOption::Identity` for the raw rescale.

### Structured DICOM metadata

At import every DICOM's extracted tags are serialized to `records.dicom_metadata` (jsonb): study/series UIDs, modality, body part, view position, laterality, study/series description, patient name (per the file), study date, instance number, SOP class/instance UIDs. The record-detail page renders this as a key/value panel; we do **not** dump it into `records.notes` (that field is reserved for free-text user-entered notes). `instance_number` is also denormalized into its own column so we can `ORDER BY` it for AP-before-Lateral tile ordering inside study blocks. The incident detail page uses `dicom_metadata->>'study_description'` as the per-study block heading when present.

### Auto-detection at import (no required form fields)

The `/records/import` form has **no subject / source / link-incident fields**. Both auto-detect at import time:

- **Subject** — DICOM `PatientName` is matched (case-insensitive, family+given) against rows in `subjects`. If exactly one matches, that's the record's subject. Otherwise we fall back to (a) the viewer's `default_subject_id` (their `cf_access_email` match), then (b) the first subject row. Mismatches (file had a `PatientName` but it matched no subject) are counted and surfaced in the redirect URL as `patient_name_mismatches:N`. Subject can always be edited on the record detail page later.
- **Source** — DICOM `InstitutionName` is looked up case-insensitively in `sources`; if no row matches, a new source is created with `kind = 'hospital'` and `notes = "Auto-created from DICOM InstitutionName at import time."` Otherwise `source_id` is left null. (For our Sutter exports this comes through as e.g. `CPMC PACIFIC CAMPUS`.)
- **Incident** — not asked for. Use the per-incident "Import DICOM linked to this incident" link to bake the link in via `?link_incident=<uuid>` query param.

### Folder upload + zip fallback

`/records/import` accepts files in two ways:

1. **Folder picker** — `<input type="file" multiple webkitdirectory directory>`, styled as a big dashed drop zone. The whole `<label>` is clickable to give the user something obvious to aim at.
2. **Zip upload** — a secondary plain `<input type="file" accept=".zip">`. Server-side `is_zip()` checks the first four bytes (`PK\x03\x04`) and `expand_zip()` walks entries via the synchronous `zip` crate, treating each entry as a candidate DICOM. We don't recurse zips-in-zips.

Either path produces the same record set; the user can switch when one is being awkward in their browser.

## Background sync (`src/sync/`)

A small task-runner framework for periodic or on-demand syncs. Lives entirely in the same process — no external scheduler or queue.

- **Framework** (`src/sync/mod.rs`): `TaskDef` (name + schedule_hours + `TaskFn` function pointer), `SyncJob` (DB row struct), `run_loop` (spawned at startup via `tokio::spawn`). The loop uses `tokio::select!` — a 60-second sleep tick checks due tasks; the `mpsc::Receiver<String>` branch handles on-demand triggers from `POST /settings/sync/:name/run`.
- **DB state** (`sync_jobs` table, migration `0012`): one row per task — `name (PK)`, `schedule_hours`, `last_started_at`, `last_finished_at`, `last_status` (`ok|error|running`), `last_message`, `next_run_at`. The framework upserts rows at startup (`ON CONFLICT DO NOTHING`). `next_run_at` is bumped by `schedule_hours` at task-start so overlapping runs don't fire even if the task hangs.
- **Trigger channel**: `AppState.sync_tx: mpsc::Sender<String>` (channel capacity 32). The sync handler sends the task name; the loop receives it and executes immediately.
- **Adding a periodic task**: (1) Write `src/sync/<name>.rs` with `pub async fn run(pool: PgPool) -> Result<String, String>`. (2) Add a `TaskDef` to `ALL_TASKS` in `src/sync/mod.rs`. The framework handles DB registration, scheduling, and status tracking.
- **Manual-only tasks** (like vaccine import) should NOT be in `ALL_TASKS`. Call `sync::record_import(pool, name, status, message)` from the handler to write history into `sync_jobs`.
- **UI**: folded into the unified **Import** page (`GET /settings/import`, `handlers::import` + `views::import`) — the single nav item for getting data in. It's an **import-type picker** (`views::import::IMPORT_TYPES`): a `<select>` htmx-swaps the chosen method's form into `#import-form` via `GET /settings/import/form?import_type=…` (so the page stays one picker + one form, not a growing stack). Types today: `ehi` (EHI zip upload) and `dvr` (CDPH vaccine). Add a method = one `IMPORT_TYPES` entry + one `type_form` arm. Run-history table below. `GET /settings/sync` 302-redirects here; `POST /settings/sync/vaccine-import` (the CDPH form target) still imports and re-renders the unified page with `dvr` selected; `POST /settings/sync/:name/run` triggers a scheduled task on-demand.

### Vaccine records import (`src/sync/vaccine.rs`)

Imports immunization records from the CDPH Digital Vaccine Record portal (myvaccinerecord.cdph.ca.gov). **Not a scheduled task** — CDPH portal links expire after 24 hours, so automation isn't possible; the user triggers the import manually by pasting links from the CDPH email.

- **Workflow**: User completes the CDPH portal OTP flow and gets an email with one link per family member. On Settings → Sync (source = "CDPH Digital Vaccination Record"), **pick the subject**, then paste that person's link(s) — or the page HTML — one at a time. The link is fetched immediately.
- **HTML-table parsing, not SHC/JWS.** The CDPH DVR page is server-rendered HTML with the CAIR history in `<table>`s — there is **no** embedded SMART Health Card / JWS / FHIR bundle. `parse_cdph_html` reads the patient name from the `Name: </span>…` marker and walks each `<tbody>`/`<tr>`, taking the (vaccine, dose, date, age, clinic) `<td>` cells. "Invalid" doses (given too soon per CDC) parse with a null `dose_number` but still count. The bottom "recommended, not in your record" table has <5 columns and is skipped, so recommendations (e.g. a COVID row) are never imported.
- **Subject is REQUIRED and caller-specified — never auto-detected.** CAIR labels minors as "Dependent Minor N", so guessing whose record it is would be a data-integrity hazard. The handler rejects a blank subject; the page's printed name is echoed in the result for a sanity check. (The old name-matching auto-detect was removed 2026-07-06.)
- **Upsert** into `immunizations` on `(source_id, external_id)`; source is "CA Immunization Registry (CAIR)" (auto-created). `external_id = cair_{subject}_{slug(vaccine)}_{date}` — **subject-scoped** (two family members with the same vaccine on the same day can't collide on the global index) and **dose-independent**. Dose is deliberately out of the key: a person can't get the same vaccine twice in a day, and combination vaccines (e.g. DTaP-IPV/Hib) are listed once per disease group with the dose filled under one group and blank under another — keying on dose stored those as duplicates. Dedup keeps whichever listing carries a real dose number (and the upsert `coalesce`s so a blank never wipes a real dose). Re-importing the same person's record is idempotent (verified end-to-end). See the `dedup_immunizations` unit tests.
- **migration `0013`** added `subjects.cdph_shc_url` (later dropped by `0014` — the 24-hour expiry made per-subject URL storage useless).

## Bundle import — FHIR / C-CDA / MyChart (`importer.rs`)

Structured clinical exports (allergies, meds, problems, immunizations, labs, vitals) land via `importer.rs`, **not** the DICOM/records path. Two entry points share the same parse + upsert core:

- **Offline CLI (primary):** `personal-emr import <zip|xml|json|dir> [--subject <uuid|name>] [--source <name>] [--commit]`. The binary detects the `import` subcommand in `main.rs` and runs `import_cli::run` instead of the server — connects straight to `DATABASE_URL`, no running server / API token / Cloudflare. **Dry-run by default** (parses + prints an extraction preview, writes nothing); `--commit` performs the upsert inside one transaction (all-or-nothing). Subject auto-detects from the document's `recordTarget` patient name (matched against `subjects`), overridable with `--subject`; source defaults to `MyChart import`, created if new. This is how family MyChart dumps are bulk-loaded (3 orgs × 3 people), so it stays a CLI, not a web upload.
- **API (`POST /api/v1/import/{fhir,ccda}`):** bearer-auth, one document per request (single C-CDA XML or FHIR Bundle JSON). Still works; uses the same `import_ccda`/`import_fhir`.

**C-CDA parsing is validated against real Epic ("Lucy") exports — not just the spec.** Hard-won facts the parser depends on (do not regress):

- **Entries are matched by `templateId` root**, never by `displayName`. Real Epic exports routinely omit `@displayName` and the original spec-only parser extracted almost nothing (it even mistook the nested *Status="Active"* observation for the problem name). Roots: allergy obs `…22.4.7`, problem obs `…22.4.4`, medication activity `…22.4.16`, immunization `…22.4.52`, result obs `…22.4.2`, vital obs `…22.4.27`.
- **Names come from `<originalText>` / narrative references.** `resolve_display` tries inline `originalText` → a `<reference value="#id"/>` resolved against `narrative_map` (every element with an `ID` attr → its text) → `@displayName` → `<translation>` displayName. Allergen names come from `participant/playingEntity/name`.
- **`el_text` collects only genuine text nodes** (`Node::is_text`) — iterating `descendants()` and calling `.text()` on element nodes too double-counts (it returns the element's first text child).
- **Idempotency / dedup keys on the HL7 `<id root^extension>`** (`entry_id`), stored as `external_id`. The same allergy/problem repeats across all 35 docs in a package but carries the same id, so importing the whole IHE_XDM zip dedups to one row while keeping distinct per-visit labs. Re-importing upserts on `(source_id, external_id)`.
- **IHE_XDM zips:** `load_documents` expands the zip (or walks a directory) and imports **every** `DOC*.XML`, deduped as above (`import_ccda_docs`).
- Captures more than the originals: `code_system` (normalized via `norm_system`), lab `ref_low`/`ref_high` + `abnormal_flag` (from `referenceRange`/`interpretationCode`), `panel_id` (deterministic per result `<organizer>` so a CBC's analytes group), condition `status` (defaults to `active` — `conditions.status` is NOT NULL), and allergies the FHIR way: distinct `criticality` (high|low|unable-to-assess) vs reaction `severity` (SNOMED-coded value set `mild|moderate|severe|life-threatening|fatal` in `models::ALLERGY_SEVERITIES`; off-vocabulary → dropped, not stored verbatim) + a SNOMED-coded manifestation.
- **Problem-list entries that aren't conditions → incidents.** Epic files the *delivery* in the OB problem list, but a delivery/birth is a real-world *event*, not a chronic condition. `is_birth_event(name, code)` (well-known SNOMED delivery codes `289259007`/`11466000`/`177184002`, or a "delivery"/"cesarean" name) routes those to a `CItem::Incident` → the `incidents` table instead. Incidents carry **no** `source_id`/`external_id` (dropped in migration `0003`), so import-created incidents dedup on **content** — `(subject_id, lower(title), occurred_at)` — not on a provenance key. (Both the C-CDA and FHIR `Condition` paths do this.)
- **Conditions dedup by code.** The same chronic problem recurs across visit documents with a *different* HL7 entry-id, so `(source_id, external_id)` alone leaves duplicates. When a condition is coded, the upsert keys on `(subject_id, code, code_system)` and updates in place rather than inserting a second row (uncoded conditions still fall back to the `(source_id, external_id)` upsert). This is a runtime guard, not a DB constraint — no unique index (so it never blocks a migration on dirty data).
- **Fidelity warnings:** `import_ccda_docs`/`preview_ccda_docs` return `Counts.warnings` / `Preview.warnings` — non-empty means low fidelity (a document failed to parse, a section was present but imported nothing, or entries were dropped for a missing name/date). The API import endpoints return this in JSON and the CLI prints it, so an importing agent can react rather than assume a clean load. The CLI is **dry-run by default and needs `DATABASE_URL` only under `--commit`**.

### Epic EHI export (TSV) — `import_ehi` / `preview_ehi`

`personal-emr import <dir>` also handles an **Epic EHI export** (the "EHI Export" / "Requested Records" download — *not* C-CDA/FHIR): `import_cli` detects an `EHITables/` directory and routes to `importer::preview_ehi` / `import_ehi`. Same `CItem` → `upsert_item` core, so dedup/idempotency + provenance are shared. Validated against a real pediatric export (idempotent re-import: counts unchanged, zero dupes).

Hard-won facts (do not regress):
- **Name-denormalized, code-poor.** Epic puts the readable value in `*_ID_*_NAME`/`*_C_NAME` columns and omits the raw code almost everywhere — **no CVX/ICD-10/LOINC/SNOMED/CPT with data**. The only real machine code is **NDC** (immunizations). We map on names, assign canonical growth LOINCs ourselves, carry NDC where present.
- **Flowsheet values live in a separate table.** Vitals/growth = `IP_FLWSHT_MEAS` ⋈ **`V_EHI_FLO_MEAS_VALUE`** on `(FSD_ID, LINE)` (the value companion — despite the `V_EHI_` prefix it is NOT audit; do not filter it out). Whitelist the real growth measures (`WEIGHT/SCALE`, `HEIGHT`, `HEAD CIRCUMFERENCE`, `TEMPERATURE`) out of ~40 template-formula rows, **convert oz→kg / in→cm**, and assign canonical growth LOINC (weight `29463-7`, length `8302-2`, head-circ `9843-4`) so the CDC growth charts (`handlers::subjects::growth`) render.
- **Immunizations** from `IMMUNE` (name + `NDC_NUM_ID_NDC_CODE` + lot/site/route). **Vaccine `ORDER_MED` orders are skipped** (`looks_like_vaccine`) — they duplicate the authoritative `IMMUNE` rows; only real meds land in `medications`.
- **Conditions** from `PROBLEM_LIST` (name + status + onset/resolved); birth events route to incidents via `is_birth_event`.
- **Allergies:** `ALLERGY_FLAG = Y` (with an empty `ALLERGY` table) is a positive **No Known Allergies** assertion → sets `subjects.no_known_allergies`, not an allergy row.
- **Subject is REQUIRED and caller-specified** (`--subject`) — an EHI export names the patient only by internal id, so never auto-detect. Provenance key `ehi_{table}_{row-id}`.
- **v1 scope (surfaced as fidelity warnings):** encounter diagnoses (`PAT_ENC_DX`, incl. acute events like fractures), RTF clinical notes, procedures, and labs (absent in the sampled export) are **not** imported yet.
- `CItem` gained optional richer fields for EHI (the C-CDA path passes `None`): `Immunization` +`dose_number`/`lot_number`/`site`/`route`, `Condition` +`resolved`, `Medication` +`dose`/`route`/`status`/`started`.
- **Web upload (`/settings/import`, `handlers::import`):** the EHI import is ALSO a browser upload — pick subject, drop the zip, **Preview** (dry-run) → **Import** (commit), run in-process against the live DB. This is a deliberate exception to the "bulk imports are CLI-only" convention above, made because we re-import these repeatedly (multiple family members × portals). C-CDA/FHIR uploads stay CLI-only for now. The handler extracts only `EHITables/*.tsv` into a temp dir under `FILES_DIR` (writable in the distroless container; `/tmp` may not be), then reuses `preview_ehi`/`import_ehi`.
- **Gotcha — Windows zip separators.** Epic zips the EHI export on Windows, so zip entry names use **backslashes** (`EHITables\IMMUNE.tsv`). The upload handler normalizes `\`→`/` before matching; a CLI import pointed at an already-extracted directory never hits this, but the raw-zip path must.

## Schema rules

- Every clinically-meaningful entity table carries `subject_id uuid not null references subjects(id)`. Non-negotiable. Do not add a clinical entity without this.
- **Source provenance is a record-level concept, not an incident-level one.** An incident is a real-world event (a fall, an ER visit); its records may live across multiple EMRs. Per-incident "sources touching this incident" are derived by joining `incidents → incident_records → records → sources`. Do **not** add a `source_id` column to `incidents` (it was removed in migration `0003_incident_provenance_cleanup.sql` for exactly this reason). The same applies to anything else that's a real-world event rather than a digital artifact.
- **"Incident" is the internal name; the UI calls it an "Event".** The table, routes (`/incidents`), and API (`/api/v1/incidents`) keep the `incident` name; only user-facing labels say "Event" (renaming the rest would ripple through the API + KB for no gain). Events can span days: `incidents.occurred_at` is the start and `ended_at` (nullable, migration `0011`) the end — null = a point-in-time event (the default for every prior row and every import). Render spans with `render_date_range(...)`. The `Incident` model's new `ended_at`/`ended_precision` fields are `#[sqlx(default)]`, so a SELECT that omits the columns still maps (they come back null/"day") instead of failing FromRow at runtime — handy since incident SELECTs are spread across many handlers, but **prefer adding the columns** to any query whose view shows the date.
- Every entity that **is** a digital artifact (e.g. `records`) and may originate externally carries `source_id`, `external_id`, `external_url`, `source_synced_at timestamptz`, `source_payload jsonb`. The `(source_id, external_id) where both not null` unique index makes idempotent re-imports safe.
- Free-text searchable entities have a `search_tsv tsvector generated always as (...) stored` column with a GIN index. `to_tsvector('english', ...)` for prose; `to_tsvector('simple', ...)` for tags/kinds.
- Migrations are plain `.sql` files in `migrations/` applied in order at boot via `sqlx::migrate!`. Never hand-edit an applied migration; add a new one.

## Multi-subject + auth rules

- **Subject is data, not auth.** Every list view, search, and form respects `subject_id` as a filter and a default; no view should be subject-blind unless the user explicitly asks for "all".
- **The UI is gated by Cloudflare Access**, not by in-app code. The policy is currently "any authenticated viewer sees every subject"; we write zero permission-check code for browser traffic.
- **The viewer middleware** reads `Cf-Access-Authenticated-User-Email` (or `DEV_VIEWER_EMAIL` in local dev), matches it against `subjects.cf_access_email`, and sets a UI default. It never gates access. If that policy ever changes, add a `visibility` column or an ACL table — do not retrofit `subject_id` semantics.
- The app binds `0.0.0.0:8080` inside the container, but the host publishes only `127.0.0.1:8100` — Cloudflare Tunnel is the sole ingress. If that ever changes (LAN exposure, etc.), upgrade the viewer middleware to verify the Cloudflare Access JWT before merging.
- **The `/api/v1/*` surface is a second auth path**: an app-issued bearer token from the `api_keys` table, gated by `api_auth::middleware`. The agent still has to clear Cloudflare Access at the edge (typically via a CF Access service token configured on the `emr.roshangeorge.dev` app), then we additionally verify the bearer token. Tokens are sha256-hashed before storage; the raw token is shown once at creation in `/settings/api-keys`. Keys are unscoped and un-roled — **any valid key can read and write (POST) every subject**, same "every viewer sees everything" policy as the UI (the API gained clinical writes for an upload agent; see the API surface section). The `owner_subject_id` column is for accounting/revocation, NOT a data filter. Do not collapse the two auth paths into one (e.g. by trusting only the bearer token at the app and dropping the CF Access gate) without an explicit design conversation.

## API surface (`/api/v1/*`)

- **Reads everywhere; writes on the clinical resources.** GET across every resource. POST (create) on the clinical resources — `sources`, `providers`, `subject_identifiers`, `subject_providers`, `subject_relationships`, `appointments`, `allergies`, `medications`, `conditions`, `immunizations`, `observations`, `care_reminders`, `insurance_plans`, `subject_insurance`. This is a deliberate reversal of the original read-only posture, made for a named caller: an **upload agent** that parses records (PDFs, portal exports) into structured rows. There are **no PUT/PATCH/DELETE** — the only "edit" path is re-POSTing (see upsert below). Adding those verbs, or writes for non-clinical resources (subjects/incidents/records), is a fresh scope decision — talk first.
- **POST is an idempotent UPSERT.** Keyed on the row's natural dedup key — `(source_id, external_id)` for the provenance tables (`provenance_conflict` in `handlers::api::mod`), `npi` then `(source_id, external_id)` for providers, `(source_id, id_type, value)` for identifiers, the PK for the join tables, and case-insensitive `name` for sources. Supply the key to update-in-place on re-upload; omit it and every POST inserts. `care_reminders` has no key → always inserts. POST returns the resulting row as JSON (200).
- **JSON in, JSON out.** Errors are `{"error": "..."}` with the right HTTP status, served by `handlers::api::ApiError`. Never return HTML from an `/api/v1/*` route. Request bodies use the `ApiJson<T>` extractor so a malformed body is a `{"error":...}` 400, not axum's plain-text rejection. Write errors map through `write_err`: FK violation → 400, unique violation → 409, bad enum/uuid/number → 400 (vocabularies validated against the `const &[&str]` sets via `validate_in`).
- **Mirror the UI's read shape.** Read endpoints exist for subjects, incidents, records (incl. file/preview/thumbnail bytes), sources, full-text search, the clinical resources above, plus `/api/v1` (discovery — lists every endpoint + the write vocabularies) and `/api/v1/me`. File routes stream the original bytes inline with the real Content-Type.
- **Same query params as the UI.** Lists accept `?subject=<uuid>` (except reference data: providers); records/observations also accept `?kind=`/`?code=`. Lists accept `?limit=` (default 100, capped 500) and `?offset=`.
- **`numeric` columns:** `observations.value_num`/`ref_low`/`ref_high` are typed `f64` (no sqlx decimal feature), so those handlers project with `::float8` rather than `select */returning *`. Every other table can use `*`.
- All API handlers extract `ApiKeyContext` so per-key accounting (last_used_at, etc.) is automatic. **Keys are still unscoped + un-roled: any valid key can read AND write every subject** — same policy as the UI. If write access ever needs to be narrower than read, add a scope/role to `api_keys` (don't infer it from `owner_subject_id`).

## HTMX rules

- Every mutating handler returns server-rendered HTML, not JSON. Successful POST/PUT/DELETE returns the new partial that HTMX swaps in; failed POST returns the form re-rendered with inline error markup and a 4xx status (HTMX swaps it via `hx-target` on the form).
- The set of HTMX attributes we use is intentionally small: `hx-get`, `hx-post`, `hx-delete`, `hx-target`, `hx-swap`, `hx-trigger`. If you find yourself reaching for `hx-vals`, `hx-include`, or out-of-band swaps, pause and reconsider — the route shape is probably wrong.
- No JSON-emitting endpoints unless an explicit non-browser caller is named in the PR.
- **One first-party JS exception:** the `/timeline` page carries a small inline `<script>` (`WHEEL_ZOOM_JS` in `views/dashboard.rs`) for **trackpad pan + zoom** — wheel axis + pointer position can't be read in CSS or htmx. It only intercepts the wheel when the pointer is over the timeline strip (`[data-tl-band]` rect ± a margin); otherwise the page scrolls normally. The gesture's dominant axis decides: **horizontal swipe pans** (scroll right moves the window to later dates, 1:1 with the finger; always `preventDefault`ed so a sideways swipe can't trigger browser back/forward nav), **vertical scrolls zoom** about the day under the cursor (up = in, down = out; the cursored date keeps its window fraction so it stays put). It keeps the intended window in a JS `cur` (accumulating sub-day deltas) and **`htmx.ajax`-swaps `#tl-inner` in place** (no reload) to that `from`/`to`, coalesced (one request in flight; the latest window is flushed on `htmx:afterSettle`, which also re-`sync`s `cur` from the rendered window so tab/date-box swaps stay in step). A **zoom-in that would empty the window is refused**: every marker carries its date in `data-d`, and since a zoom-in window is a subset of the current one, if no `data-d` falls inside the proposed `[from,to]` the step is dropped — so you stop at the tightest *populated* window. Pans clamp to the data range; fully zoomed out, a downward scroll lets the page scroll. The empty-state (a `range` preset with no events) has **no `[data-tl-band]`**, so the handler falls back to `#tl-inner`'s rect, keeping zoom-out alive. The window is server-driven: `/timeline` returns the full page normally and just the `#tl-inner` partial on an `HX-Request`; `from`/`to` ISO params beat the `range` preset; the tabs and the from/to date-box form also htmx-swap `#tl-inner`. Clicking a marker `hx-get`s `/timeline/day` into the persistent `#tl-detail` panel, which lives **outside** `#tl-inner` so it survives zoom/pan. The JS only computes the window and calls `htmx.ajax`. **htmx loads with `defer`, so this inline script defers its own wiring to `DOMContentLoaded`** — otherwise `window.htmx` is undefined when it runs at parse time and the listener never attaches (this regressed once). Keep custom JS to this kind of last-resort, no-dependency, no-build snippet; reach for htmx first.

## Component rules (Tailwind + maud)

**Tailwind class strings live in exactly one place: `src/views/components.rs`.**

Per-page views (`views/dashboard.rs`, `views/incident.rs`, etc.) compose pages from primitives in `components.rs` and **must not** sprinkle inline `class="..."` attributes. The exception is layout-only utilities (`flex`, `grid`, `gap-*`, `mb-*`, `mt-*`, `space-y-*`, `w-*`) used to arrange existing primitives — those are fine because they don't carry a "look."

When a view needs a new visual idiom (a new badge color, a new button variant, a card-with-stripe), **add a function to `components.rs` first**, then use it.

`tailwind.css` defines a small set of semantic color tokens (`--color-canvas`, `--color-ink`, `--color-muted`, `--color-line`, `--color-brand`, `--color-subject-bg/fg`, `--color-kind-bg/fg`, `--color-source-bg/fg`, `--color-danger`). Prefer these over raw Tailwind palette names so a re-skin is one-file. They expand to Tailwind utility classes automatically (`bg-canvas`, `text-ink`, `border-line`, …).

**To regenerate CSS**: `mise run css` (one-shot) or `mise run css-watch` (live). The Tailwind CLI scans every `.rs` file under `src/` for class strings, so any class you put in a maud `class="..."` literal will end up in the bundle.

**Gotcha — arbitrary-value classes inside Rust source.** Tailwind v4's source scanner does *not* reliably pick up classes with arbitrary values (`text-[0.7rem]`, `grid-cols-[repeat(auto-fill,minmax(20rem,1fr))]`, etc.) when they live inside Rust string literals. They silently disappear from the compiled CSS. **Don't write arbitrary-value classes in Rust source.** Two safe patterns:

1. Use a standard token (`text-xs` for `text-[0.75rem]`, `max-w-32` for `max-w-[8rem]`, etc.).
2. If you genuinely need an arbitrary value, name it as a `@utility` block in `tailwind.css` and use the named class in the source. Existing examples: `.card-grid`, `.tile-grid`, `.dicom-meta-grid`, `.viewer-frame`, `.viewer-img`, `.timeline-line-top`, `.timeline-marker-offset`.

**Gotcha — regenerate + commit CSS before you deploy.** The image is built by **skybuild** on tb-0-0 from **committed HEAD** (not your working tree — see "Builds (skybuild)" below), so a class you added but didn't compile won't be in the shipped `static/vendor/app.css`. `mise run deploy` guards this for you: it depends on `css` (regenerates the bundle) and then **refuses to build if the tree is dirty**, telling you to commit. So the workflow is `mise run css` → commit → `mise run deploy`. The failure this prevents is stale CSS: missing styles (huge unstyled `<time>` elements, invisible dots, ungrid-ed cards). The local `mise run build` (raw `docker build` of the working tree) is for local container testing only — it is **not** what deploys.

## Deployment shape (kant)

- Subdomain: `emr.roshangeorge.dev`
- Internal port: `127.0.0.1:8100` on kant; container listens on `0.0.0.0:8080`.
- DB: `personal_emr` database, role `personal_emr` in the shared `postgres-main` container.
- Secrets: the `DATABASE_URL` (with the `personal_emr` password baked in) lives at `~/.config/personal-emr/env` on kant, mode 0600. The quadlet pulls it via `EnvironmentFile=/home/ubuntu/.config/personal-emr/env`. This is a one-time bootstrap — `mise run deploy` only checks the file exists; it never writes to it. Rotate by editing in place and `systemctl --user restart personal-emr.service`.
- Files: `~/personal-emr/files/` on kant, bind-mounted to `/data/files:Z,U`. The `U` is non-negotiable: the runtime image is distroless `:nonroot` (UID 65531), and rootless Podman maps that to subuid `165531` on the host — *not* the `ubuntu` user that owns the bind-mount source. `:U` tells Podman to recursively chown the source to the container user's mapped UID at mount time. Without it any write to `/data/files` errors with `Permission denied`.
- Image: built by **skybuild** on tb-0-0 (native amd64) and pushed to the kant registry at `100.64.0.2:5000/personal-emr:latest` (tailnet HTTP). The **quadlet pulls from `localhost:5000/personal-emr:latest`** — `100.64.0.2:5000` and `localhost:5000` are the same on-host registry container on kant, so an image pushed under one name is reachable under the other. (Previously the laptop `docker build`+pushed via `kant.internal.roshangeorge.dev:5000`; skybuild replaced that. The registry, image name, and `:latest` tag the quadlet pulls are all unchanged, so the kant runtime contract didn't move.) skybuild also tags `git-<short_sha>` for commit→image traceability and keeps a copy in the tb-0-0 registry `100.64.0.10:30500/personal-emr`.
- Backups: nightly `pg_dump` + `tar | zstd` of files dir → `/mnt/r2/backups/personal-emr/`, 14-day retention.

## Builds (skybuild)

The container image is built by **skybuild** — the git-push-to-build service on the tb-0-0 cluster — not by a local `docker build`. skybuild builds natively on amd64 (matching kant) and pushes to the registries named in `skybuild.toml`; it **never deploys** (that stays `mise run deploy`'s job). Full guide: `KB:SKYBUILD:guides/integration`.

- **Config: `skybuild.toml` at the repo root.** One `docker` artifact → `deploy/Dockerfile`, context `.`, tags `latest` + `git-{short_sha}`, pushed to the kant registry (`100.64.0.2:5000/personal-emr`) and the tb-0-0 registry (`100.64.0.10:30500/personal-emr`). It's a **push-only mirror** (no `[source]` block) — builds are triggered by `mise run deploy`, not GitHub CI. To turn on webhook/poll CI later, add a `[source]` with `upstream` + `watch` + `refs` per the guide.
- **`mise run deploy` orchestrates it:** regenerate CSS (`css` dep) → **refuse if the tree is dirty** (skybuild builds committed HEAD, so uncommitted work — including freshly-regenerated CSS — must be committed first) → `skybuild build --repo personal-emr` (streams logs, exits non-zero on failure so a bad build never reaches the restart) → scp quadlet + backup units → restart the service on kant (which re-pulls `:latest`). The dirty-tree guard is safe because Tailwind's minified output is **deterministic**: on a tree whose committed CSS is already current, the `css` step regenerates a byte-identical `static/vendor/app.css`, the tree stays clean, and deploy proceeds — the guard only trips when you genuinely forgot to commit (source or CSS).
- **You can build without deploying.** `skybuild build --repo personal-emr` (or `mise run` up to that point) builds + pushes the image but does **not** restart anything on kant — the quadlet only re-pulls on its next restart. Handy for validating a build against the real amd64 builder without going live.
- **Auth: `SKYBUILD_TOKEN` must be in the env** for `mise run deploy`. Fetch it with `kubectl --context tb-0-0 -n skybuild get secret skybuild-secrets -o jsonpath='{.data.auth-token}' | base64 -d`. The server URL defaults to `https://skybuild.internal.roshangeorge.dev` (tailnet).
- **amd64 only.** tb-0-0 has no cross-arch emulation; keep base images amd64 (both current bases already are).
- The local `mise run build` (`docker build` of the working tree) is retained for **local container testing only** — it does not participate in deploy.
- **`deploy/Dockerfile` caches dependencies in a separate layer** (build a stub `main.rs` first so the dep compile is keyed only on `Cargo.toml`/`Cargo.lock`, then copy real `src` and build the app crate). **Do not collapse this back into one layer** — an app-only change would then recompile the whole dependency tree. The `touch src/**.rs` before the real build is **load-bearing, not optional**: BuildKit normalizes `COPY` mtimes, so without it cargo thinks the real source is older than the stub's artifacts and ships the do-nothing **stub binary** (verify a build with `podman run … | grep 'starting personal-emr'`). Measured (native, tb-0-0): full build ≈ 134s; the split saves the dep compile (~37s) but the **`personal-emr` crate itself is the dominant cost (~99s)** — so an app-only rebuild is ≈ 100s, not seconds. Cutting that further needs cargo *incremental* compilation across builds (a BuildKit `--mount=type=cache,target=/app/target`, which the layer cache can't do) or splitting the app into smaller crates — deliberately not done (avoids a builder-side cache to manage).

## Local development

```sh
# Spin up a throwaway Postgres
docker run --rm -d --name pemr-pg -p 5433:5432 -e POSTGRES_PASSWORD=postgres postgres:18

export DATABASE_URL=postgresql://postgres:postgres@127.0.0.1:5433/postgres
export FILES_DIR=/tmp/personal-emr-files
export BIND_ADDR=127.0.0.1:8080
export DEV_VIEWER_EMAIL=roshan@technologybrother.com
mkdir -p $FILES_DIR

cargo run
```

Then open `http://127.0.0.1:8080`. There's no Cloudflare in front locally; the `DEV_VIEWER_EMAIL` env var stands in for the header.

## When you change runtime contract

Anything that affects the deployed shape — port, volume path, env var, image name, Postgres role, dependency on another container — must be reflected in `kant:~/docs/README.md`. Workflow:

```sh
scp kant:docs/README.md /tmp/kant-readme.md
$EDITOR /tmp/kant-readme.md       # add/update the personal-emr row
scp /tmp/kant-readme.md kant:docs/README.md
ssh kant "cd docs && git add README.md && git commit -m 'personal-emr: <what changed>'"
```
