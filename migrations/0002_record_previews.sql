-- For records whose primary file isn't directly browser-renderable (notably
-- DICOM), we keep a separately-stored "preview" — typically a PNG rendered
-- from the DICOM's pixel data. The detail-page viewer prefers the preview;
-- the download link still serves the original `file_path`.

alter table records
    add column preview_path text,
    add column preview_content_type text;
