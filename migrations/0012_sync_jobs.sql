create table sync_jobs (
    name             text primary key,
    schedule_hours   integer not null default 168,
    last_started_at  timestamptz,
    last_finished_at timestamptz,
    last_status      text check (last_status in ('ok', 'error', 'running')),
    last_message     text,
    next_run_at      timestamptz not null default now()
);
