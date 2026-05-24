-- API keys for the read-only /api/v1 surface.
--
-- The token itself is shown exactly once at creation; only its sha256 hash
-- is stored. `token_prefix` is the leading few characters of the raw token
-- ("pemr_abcd1234") and is safe to show in the UI to help the user identify
-- which key they're looking at.
--
-- `owner_subject_id` is for accounting / revocation, NOT for data filtering.
-- Per CLAUDE.md, the in-app policy is "every authenticated viewer sees
-- everything"; API keys mirror that. The owner column lets us answer
-- "whose key is this" when revoking.

create table api_keys (
    id                  uuid primary key,
    name                text not null,
    token_hash          text not null,
    token_prefix        text not null,
    owner_subject_id    uuid references subjects(id) on delete set null,
    last_used_at        timestamptz,
    created_at          timestamptz not null default now(),
    revoked_at          timestamptz
);
create unique index api_keys_token_hash_uk on api_keys (token_hash);
create index api_keys_owner_idx on api_keys (owner_subject_id);
create index api_keys_active_idx on api_keys (created_at desc) where revoked_at is null;
