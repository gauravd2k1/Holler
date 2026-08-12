-- Holler Cloud PostgreSQL — device enrollment. Contracts 0.4.0, ADR-017.
--
-- CLOUD-ONLY. Neither table has a SQLite mirror and neither is an
-- AggregateType — the refresh_token precedent (0003). Giving device_credential
-- a sync direction would ship credential material to the very device whose
-- identity it is meant to establish.
--
-- Until Milestone 3's T1/T4 land, `GET /sync/config` — the one route carrying
-- Argon2id password and PIN hashes — is gated on an ordinary human bearer
-- token, so an enrolled edge node and a logged-in browser session are
-- indistinguishable to the backend. These tables are the shape that closes it.

-- The cloud's registry of outlet devices. Until now `"order".device_id` and
-- `kot.changed_by_device_id` were bare UUIDs referencing nothing, because the
-- edge owned the only device table. Enrollment requires the cloud to know
-- which devices exist before it can decide which may pull credentials.
--
-- Deliberately NOT an AggregateType: a device is registered through the
-- enrollment flow, not replicated as config, and it is never replayed from the
-- edge. Adding it to the authority map would give it a sync direction it must
-- not have.
CREATE TABLE device (
    id           UUID PRIMARY KEY,               -- app-generated UUIDv7 (§74)
    outlet_id    UUID NOT NULL REFERENCES outlet(id),
    kind         TEXT NOT NULL CHECK (kind IN ('POS','KDS','WAITER','PRINTER_BRIDGE')),
    name         TEXT NOT NULL,
    -- Set when enrollment completes. A device row may exist unenrolled (an
    -- admin registered it ahead of install); it simply cannot sync until it
    -- holds a credential.
    enrolled_at  TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ,
    last_seen_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Tenant-scoped, never global: two outlets each naming a device "POS1" is
-- normal and must not collide.
CREATE UNIQUE INDEX idx_device_outlet_name ON device(outlet_id, name);
CREATE INDEX idx_device_outlet_id ON device(outlet_id);

-- The per-device enrolled credential.
--
-- token_hash is Argon2id over a high-entropy token minted at enrollment. The
-- PLAINTEXT IS RETURNED ONCE, at enrollment, and never again — there is no
-- route that reads it back, by construction. It must never appear in an
-- audit_event old/new value; the audit helper's redact list gains
-- 'device_token_hash' alongside password_hash, pin_hash and token_hash.
--
-- Rotation appends a new row and stamps revoked_at on the old one rather than
-- updating in place, so a compromised credential leaves a trail rather than
-- being overwritten out of history.
CREATE TABLE device_credential (
    id          UUID PRIMARY KEY,                -- app-generated UUIDv7 (§74)
    device_id   UUID NOT NULL REFERENCES device(id),
    tenant_id   UUID NOT NULL REFERENCES tenant(id),
    outlet_id   UUID NOT NULL REFERENCES outlet(id),
    token_hash  TEXT NOT NULL,
    -- Free-text note for the technician: which machine, which install visit.
    label       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ
);

-- One active credential per device at a time. A rotation revokes before it
-- issues, so two live credentials for one device is a bug, not a state.
CREATE UNIQUE INDEX idx_device_credential_active
    ON device_credential(device_id) WHERE revoked_at IS NULL;
CREATE INDEX idx_device_credential_device ON device_credential(device_id);
CREATE INDEX idx_device_credential_tenant ON device_credential(tenant_id);

-- Backfill: "order".device_id and cash_shift.device_id were written before
-- this table existed and may reference devices with no row here. The foreign
-- key is deliberately NOT added retroactively — doing so would fail on
-- existing dev data and, worse, would make an unenrolled device unable to
-- replay orders it has already taken. Enrollment gates the SYNC PATH, not the
-- historical record.
