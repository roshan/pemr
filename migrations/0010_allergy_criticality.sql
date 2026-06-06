-- Allergy reactions, modeled the FHIR AllergyIntolerance way.
--
-- `criticality` (the potential clinical seriousness of a FUTURE reaction on
-- re-exposure: high | low | unable-to-assess) is a distinct axis from a
-- reaction's `severity` (how bad a reaction WAS: mild | moderate | severe).
-- The original C-CDA importer conflated them, storing the HL7 criticality
-- (CRITH/CRITL) in `severity`. Split them out, and carry the reaction
-- manifestation as a coded concept (SNOMED CT) alongside its display text,
-- consistent with how every other coded entity stores code + code_system.
alter table allergies add column criticality          text; -- high | low | unable-to-assess
alter table allergies add column reaction_code         text; -- SNOMED CT manifestation code
alter table allergies add column reaction_code_system  text;
