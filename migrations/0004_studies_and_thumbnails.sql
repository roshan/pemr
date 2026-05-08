-- DICOM exports group multiple files (image views + the radiologist's report)
-- under a single StudyInstanceUID. We store that UID directly on records so
-- the incident detail page can group them visually. No separate `studies`
-- table for now — lift to one later if we ever want study-level metadata
-- stored once instead of denormalized.

alter table records
    add column study_instance_uid    text,
    add column thumbnail_path        text,
    add column thumbnail_content_type text;

create index records_study_instance_uid_idx
    on records (study_instance_uid)
    where study_instance_uid is not null;
