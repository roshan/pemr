-- Store the CDPH vaccine record download URL per subject.
-- After completing the portal flow at myvaccinerecord.cdph.ca.gov, the user
-- copies the download link; the weekly vaccine sync task fetches this URL to
-- get the current SMART Health Card JWS and upsert immunizations from CAIR.
alter table subjects add column cdph_shc_url text;
