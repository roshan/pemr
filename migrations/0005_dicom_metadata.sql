-- Two pieces of denormalized DICOM metadata on records.
--
-- `dicom_metadata` is the full bag (jsonb) used to render the structured
-- metadata panel on the record detail page — modality, body part,
-- laterality, study/series descriptions, UIDs, the patient name DICOM
-- claims for the file, etc. Stored as jsonb instead of a wider column
-- list because (a) most of these fields are read-only display strings,
-- and (b) we don't want to migrate the schema every time we extract a
-- new tag.
--
-- `instance_number` is a real column rather than a jsonb field because
-- we ORDER BY it (within a study, AP < Lateral < Oblique).

alter table records
    add column dicom_metadata  jsonb,
    add column instance_number int;
