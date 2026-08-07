-- Holler Cloud PostgreSQL — refresh token rotation state (contracts 0.2.1).
-- Additive to 0001/0002; nothing earlier is altered. See the ADR-011 addendum.
--
-- Cloud-only session state. Refresh tokens never sync to the edge: offline
-- login verifies the cached Argon2id hash in the edge app_user table and
-- issues a local session, so this table deliberately has NO AggregateType
-- entry and never appears in a sync envelope.
--
-- Ids are application-generated UUIDv7 (§74) — no DB-side defaults.

CREATE TABLE refresh_token (
    id             UUID PRIMARY KEY,
    family_id      UUID NOT NULL,             -- rotation chain; reuse revokes the whole family
    user_id        UUID NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    outlet_id      UUID REFERENCES outlet(id),
    token_hash     TEXT NOT NULL,             -- SHA-256 of the opaque token; the token itself is NEVER stored
    issued_at      TIMESTAMPTZ NOT NULL,
    expires_at     TIMESTAMPTZ NOT NULL,
    used_at        TIMESTAMPTZ,               -- set when rotated; a second presentation is reuse
    revoked_at     TIMESTAMPTZ,
    replaced_by_id UUID REFERENCES refresh_token(id),
    created_at     TIMESTAMPTZ NOT NULL
);

-- Rotation contract, so a persistent store is behaviour-identical to the
-- in-process one it replaces:
--   rotate  → set used_at + replaced_by_id and insert the successor in the
--             same family, in ONE transaction.
--   reuse   → presenting a token whose used_at or revoked_at is non-null
--             revokes every row sharing family_id.
--
-- Global (not tenant-scoped) uniqueness on token_hash is deliberate: a token
-- must be unique across all tenants, since scoping it per tenant would let the
-- same secret authenticate twice.
CREATE UNIQUE INDEX uq_refresh_token_hash ON refresh_token (token_hash);

CREATE INDEX idx_refresh_token_family_id ON refresh_token (family_id);
CREATE INDEX idx_refresh_token_user_id ON refresh_token (user_id);
CREATE INDEX idx_refresh_token_live ON refresh_token (expires_at) WHERE revoked_at IS NULL;
