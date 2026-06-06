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

## Schema rules

- Every clinically-meaningful entity table carries `subject_id uuid not null references subjects(id)`. Non-negotiable. Do not add a clinical entity without this.
- **Source provenance is a record-level concept, not an incident-level one.** An incident is a real-world event (a fall, an ER visit); its records may live across multiple EMRs. Per-incident "sources touching this incident" are derived by joining `incidents → incident_records → records → sources`. Do **not** add a `source_id` column to `incidents` (it was removed in migration `0003_incident_provenance_cleanup.sql` for exactly this reason). The same applies to anything else that's a real-world event rather than a digital artifact.
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

- **Reads everywhere; writes on the clinical resources.** GET across every resource. POST (create) on the clinical resources — `sources`, `providers`, `subject_identifiers`, `subject_providers`, `subject_relationships`, `appointments`, `allergies`, `medications`, `conditions`, `immunizations`, `observations`, `care_reminders`. This is a deliberate reversal of the original read-only posture, made for a named caller: an **upload agent** that parses records (PDFs, portal exports) into structured rows. There are **no PUT/PATCH/DELETE** — the only "edit" path is re-POSTing (see upsert below). Adding those verbs, or writes for non-clinical resources (subjects/incidents/records), is a fresh scope decision — talk first.
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

## Component rules (Tailwind + maud)

**Tailwind class strings live in exactly one place: `src/views/components.rs`.**

Per-page views (`views/dashboard.rs`, `views/incident.rs`, etc.) compose pages from primitives in `components.rs` and **must not** sprinkle inline `class="..."` attributes. The exception is layout-only utilities (`flex`, `grid`, `gap-*`, `mb-*`, `mt-*`, `space-y-*`, `w-*`) used to arrange existing primitives — those are fine because they don't carry a "look."

When a view needs a new visual idiom (a new badge color, a new button variant, a card-with-stripe), **add a function to `components.rs` first**, then use it.

`tailwind.css` defines a small set of semantic color tokens (`--color-canvas`, `--color-ink`, `--color-muted`, `--color-line`, `--color-brand`, `--color-subject-bg/fg`, `--color-kind-bg/fg`, `--color-source-bg/fg`, `--color-danger`). Prefer these over raw Tailwind palette names so a re-skin is one-file. They expand to Tailwind utility classes automatically (`bg-canvas`, `text-ink`, `border-line`, …).

**To regenerate CSS**: `mise run css` (one-shot) or `mise run css-watch` (live). The Tailwind CLI scans every `.rs` file under `src/` for class strings, so any class you put in a maud `class="..."` literal will end up in the bundle.

**Gotcha — arbitrary-value classes inside Rust source.** Tailwind v4's source scanner does *not* reliably pick up classes with arbitrary values (`text-[0.7rem]`, `grid-cols-[repeat(auto-fill,minmax(20rem,1fr))]`, etc.) when they live inside Rust string literals. They silently disappear from the compiled CSS. **Don't write arbitrary-value classes in Rust source.** Two safe patterns:

1. Use a standard token (`text-xs` for `text-[0.75rem]`, `max-w-32` for `max-w-[8rem]`, etc.).
2. If you genuinely need an arbitrary value, name it as a `@utility` block in `tailwind.css` and use the named class in the source. Existing examples: `.card-grid`, `.tile-grid`, `.dicom-meta-grid`, `.viewer-frame`, `.viewer-img`, `.timeline-line-top`, `.timeline-marker-offset`.

**Gotcha — always `mise run deploy` (not raw `docker build`)**. `mise run deploy` depends on `build` which depends on `css`, so the Tailwind CLI runs and `static/vendor/app.css` is up to date before the image is baked. Calling `docker build` directly skips that and ships stale CSS, which manifests as missing styles (huge unstyled `<time>` elements, invisible dots, ungrid-ed cards).

## Deployment shape (kant)

- Subdomain: `emr.roshangeorge.dev`
- Internal port: `127.0.0.1:8100` on kant; container listens on `0.0.0.0:8080`.
- DB: `personal_emr` database, role `personal_emr` in the shared `postgres-main` container.
- Secrets: the `DATABASE_URL` (with the `personal_emr` password baked in) lives at `~/.config/personal-emr/env` on kant, mode 0600. The quadlet pulls it via `EnvironmentFile=/home/ubuntu/.config/personal-emr/env`. This is a one-time bootstrap — `mise run deploy` only checks the file exists; it never writes to it. Rotate by editing in place and `systemctl --user restart personal-emr.service`.
- Files: `~/personal-emr/files/` on kant, bind-mounted to `/data/files:Z,U`. The `U` is non-negotiable: the runtime image is distroless `:nonroot` (UID 65531), and rootless Podman maps that to subuid `165531` on the host — *not* the `ubuntu` user that owns the bind-mount source. `:U` tells Podman to recursively chown the source to the container user's mapped UID at mount time. Without it any write to `/data/files` errors with `Permission denied`.
- Image: built amd64 on the laptop, pushed to the registry via the public name `kant.internal.roshangeorge.dev:5000/personal-emr:latest` (laptop has Cloudflare Tunnel + cert), but the **quadlet pulls from `localhost:5000/personal-emr:latest`** because podman on kant only has `localhost:5000` in its insecure-registry list. Both refer to the same on-host registry container — pushing under one name is reachable under the other.
- Backups: nightly `pg_dump` + `tar | zstd` of files dir → `/mnt/r2/backups/personal-emr/`, 14-day retention.

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
