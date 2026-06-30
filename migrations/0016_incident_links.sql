create table incident_links (
    incident_id        uuid not null references incidents(id) on delete cascade,
    linked_incident_id uuid not null references incidents(id) on delete cascade,
    primary key (incident_id, linked_incident_id),
    check (incident_id <> linked_incident_id)
);

-- Reverse-direction lookups: "which incidents point TO this one?"
create index incident_links_reverse_idx on incident_links (linked_incident_id);
