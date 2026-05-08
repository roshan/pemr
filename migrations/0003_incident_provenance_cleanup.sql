-- An incident is a real-world event ("snowboarding fall, Feb 2026"). Its
-- documentation may exist across many EMR systems — Sutter for the ER visit,
-- Stanford for the follow-up MRI, Anthem for the insurance claim. So source
-- provenance is a *record*-level concept, not an *incident*-level one. The
-- "sources touching this incident" view is derived by joining through
-- incident_records → records → sources.

drop index if exists incidents_source_external_uk;

alter table incidents
    drop column source_id,
    drop column external_id,
    drop column external_url,
    drop column source_synced_at,
    drop column source_payload;
