-- cdph_shc_url is useless: CDPH portal links expire in 24 h, so storing them
-- for weekly re-fetch doesn't work. The vaccine import now accepts URLs
-- pasted directly from the CDPH email and fetches them immediately.
alter table subjects drop column if exists cdph_shc_url;
