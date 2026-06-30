-- Fix rows written by the initial import before the 'infinity' bug was caught.
-- time::OffsetDateTime can't represent PostgreSQL's infinity timestamp, so
-- sqlx panics when decoding any such row.
update sync_jobs
   set next_run_at = now() + interval '10 years'
 where next_run_at = 'infinity'::timestamptz;
