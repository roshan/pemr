-- personal-emr: link conditions to the real-world event they describe (PEMR-50).
--
-- An incident is the model's real-world-event concept (a fall, an ER visit). A
-- diagnosis can be *about* an incident — e.g. EHI PAT_ENC_DX carries "Closed
-- nondisplaced fracture of right clavicle" for the 2026-05-07 Fall-from-bed: the
-- event + imaging live on the incident, but no condition row names the fracture.
-- immunizations/observations already have an optional `incident_id` ("the
-- episode"); conditions gain the same column so import can attach a dx to an
-- existing incident (matching on date) instead of creating a second event row.
alter table conditions add column incident_id uuid references incidents(id);

create index conditions_incident_idx on conditions (incident_id);
