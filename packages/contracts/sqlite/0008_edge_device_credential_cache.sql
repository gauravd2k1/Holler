-- Holler Edge SQLite — cached device credentials. Contracts 0.4.3,
-- ADR-017 amendment.
--
-- CONFIG, cloud→edge. The ADR-011 pattern applied to devices: the edge already
-- caches Argon2id password and PIN hashes so a cashier can log in with the
-- uplink down, and this is the same mechanism for the same reason.
--
-- WHY THIS EXISTS. The first KDS enrollment implementation verified every new
-- LAN connection by calling the cloud. Its verification gate ruled that a
-- blocker: a browser reload, a tablet waking, or a router blip during a WAN
-- outage left the kitchen screen unable to re-authenticate and receiving no
-- tickets. CLAUDE.md's premise is that core operations run without internet and
-- that the KDS is LAN-first; ticket visibility is a core operation. The
-- mechanism had traded an unauthenticated-but-available KDS for an
-- authenticated-but-unavailable-offline one, which is worse at exactly the
-- moment local-first is meant to protect.
--
-- A verify-online-then-cache-with-a-TTL design was considered and REJECTED:
-- it leaves a cold-start hole where a screen that has never connected while
-- online cannot join at all, which is precisely the offline-first failure this
-- architecture exists to prevent.
--
-- CONTAINMENT. The PLAINTEXT token never leaves the cloud — only this hash
-- does, only over TLS on GET /sync/config, and only to an already-enrolled
-- node. This file is encrypted at rest (ADR-011): never copy it or its backups
-- anywhere unencrypted. token_hash must never appear in an audit_event value
-- or a log line; `credential_hash` and `device_token_hash` are both in
-- AUDIT_REDACTED_FIELDS. The column is named credential_hash, not token_hash,
-- because it holds a VERIFIER you check a presented token against — never a
-- bearer token you could replay.
CREATE TABLE device_credential_cache (
    credential_id       TEXT PRIMARY KEY,       -- UUIDv7, minted by the cloud
    device_id           TEXT NOT NULL,
    tenant_id           TEXT NOT NULL,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    -- Argon2id encoded string over the token secret. Verified locally against
    -- a presented token, exactly as app_user.password_hash already is.
    credential_hash     TEXT NOT NULL,
    device_kind         TEXT NOT NULL CHECK (device_kind IN
                          ('POS','KDS','WAITER','PRINTER_BRIDGE')),
    -- A revoked or expired credential STILL SYNCS and is still stored. The edge
    -- must be able to learn that a credential is dead, and it cannot learn that
    -- from a row's absence while the uplink is down — absence is
    -- indistinguishable from "not yet synced". Rejection is decided by these
    -- columns, never by whether a row exists.
    revoked_at          TEXT,
    expires_at          TEXT,
    config_version      INTEGER NOT NULL
);

CREATE INDEX idx_device_credential_cache_device ON device_credential_cache(device_id);
CREATE INDEX idx_device_credential_cache_outlet ON device_credential_cache(outlet_id);
