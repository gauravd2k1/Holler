-- Holler Edge SQLite — Milestone 1 identity, RBAC and table contracts.
-- Additive to 0001_init.sql; nothing in 0001 is altered. See ADR-011.
--
-- IDs are application-generated UUIDv7 (§74), matching 0001.
--
-- ENCRYPTION AT REST: app_user caches Argon2id credential material on the
-- shop-floor device so a cashier can authenticate with the WAN down. The edge
-- database file therefore falls under the edge encryption-at-rest requirement
-- (ADR-003 amendment / ADR-011) — edge/database must open this file encrypted
-- and must never copy it to an unencrypted location.

CREATE TABLE app_user (
    id               TEXT PRIMARY KEY,
    tenant_id        TEXT NOT NULL,
    outlet_id        TEXT NOT NULL REFERENCES outlet(id),
    email            TEXT NOT NULL,
    full_name        TEXT NOT NULL,
    password_hash    TEXT NOT NULL,        -- Argon2id encoded string, verified locally when offline; never logged
    pin_hash         TEXT,                 -- Argon2id manager-approval PIN
    is_active        INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    permissions_json TEXT NOT NULL,        -- flattened permission list for THIS outlet; replaced wholesale per config_version
    config_version   INTEGER NOT NULL,     -- cloud→edge, replace-not-merge (§50.1)
    updated_at       TEXT NOT NULL,
    UNIQUE (outlet_id, email)
);

-- Pure configuration, cloud→edge, replaced per config_version. No operational
-- state columns — a table's live state lives in table_session (ADR-011).
CREATE TABLE restaurant_table (
    id             TEXT PRIMARY KEY,
    outlet_id      TEXT NOT NULL REFERENCES outlet(id),
    section        TEXT NOT NULL,
    label          TEXT NOT NULL,
    seat_count     INTEGER NOT NULL CHECK (seat_count > 0),
    is_active      INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    config_version INTEGER NOT NULL,
    UNIQUE (outlet_id, label)
);

-- Operational aggregate, edge-authoritative, replayed edge→cloud append-only.
CREATE TABLE table_session (
    id                TEXT PRIMARY KEY,
    outlet_id         TEXT NOT NULL REFERENCES outlet(id),
    table_id          TEXT NOT NULL REFERENCES restaurant_table(id),
    state             TEXT NOT NULL CHECK (state IN
                        ('OCCUPIED','ORDERED','KOT_SENT','FOOD_READY',
                         'BILL_REQUESTED','PAYMENT_PENDING','PAID','DIRTY','CLOSED')),
    current_order_id  TEXT REFERENCES "order"(id),
    guest_count       INTEGER NOT NULL CHECK (guest_count > 0),
    opened_by_user_id TEXT REFERENCES app_user(id),
    opened_at         TEXT NOT NULL,
    closed_at         TEXT,
    version           INTEGER NOT NULL DEFAULT 1,
    sync_status       TEXT NOT NULL DEFAULT 'PENDING' CHECK (sync_status IN ('PENDING','SYNCED','FAILED')),
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

CREATE UNIQUE INDEX uq_table_session_open
    ON table_session (table_id)
    WHERE closed_at IS NULL;

-- Local append-only audit; drains to cloud through local_outbox (ADR-007).
-- old_value_json/new_value_json must never contain password_hash or pin_hash —
-- the audit helper redacts them before the row is written (ADR-011).
CREATE TABLE audit_event (
    id             TEXT PRIMARY KEY,
    outlet_id      TEXT NOT NULL REFERENCES outlet(id),
    actor_user_id  TEXT,
    device_id      TEXT NOT NULL REFERENCES device(id),
    action         TEXT NOT NULL,
    entity_type    TEXT NOT NULL,
    entity_id      TEXT,
    old_value_json TEXT,
    new_value_json TEXT,
    reason         TEXT,
    occurred_at    TEXT NOT NULL
);

CREATE INDEX idx_app_user_outlet_id ON app_user(outlet_id);
CREATE INDEX idx_restaurant_table_outlet_id ON restaurant_table(outlet_id);
CREATE INDEX idx_table_session_outlet_id ON table_session(outlet_id);
CREATE INDEX idx_table_session_sync_status ON table_session(sync_status);
CREATE INDEX idx_audit_event_occurred_at ON audit_event(occurred_at);
