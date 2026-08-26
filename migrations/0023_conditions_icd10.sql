-- personal-emr: capture a condition's ICD-10 translation alongside the primary
-- code (PEMR-30).
--
-- C-CDA problem observations carry a primary code (usually SNOMED) plus an
-- optional <translation> with the ICD-10 billing code. The importer previously
-- kept only the primary value code, so the ICD-10 was lost. This nullable
-- second-code column mirrors the allergies.reaction_code precedent: keep the
-- standardized primary (queryable/stable) and let the translation ride beside
-- it for display. Null when the source carried no translation.
alter table conditions add column icd10_code text;
