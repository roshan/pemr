-- Events (called "incidents" internally) gain an optional end date so a
-- real-world episode with duration — a hospital stay, a trip — is a first-class
-- span, not just a point. occurred_at stays the start; ended_at null = a
-- point-in-time event (every existing row), so this is backward-compatible.
alter table incidents
    add column ended_at        date,
    add column ended_precision text not null default 'day';
