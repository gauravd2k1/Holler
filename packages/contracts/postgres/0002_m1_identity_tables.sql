-- Holler Cloud PostgreSQL — Milestone 1 identity, RBAC and table contracts.
-- Additive to 0001_init.sql; nothing in 0001 is altered. See ADR-011.
--
-- IDs are application-generated UUIDv7 (§74) — no gen_random_uuid() defaults,
-- matching the 0001 order/kot tables. The cloud never mints entity ids.

CREATE TABLE app_user (
    id             UUID PRIMARY KEY,
    tenant_id      UUID NOT NULL REFERENCES tenant(id),
    email          TEXT NOT NULL,
    full_name      TEXT NOT NULL,
    password_hash  TEXT NOT NULL,          -- Argon2id encoded string; never logged, never placed in audit_event
    pin_hash       TEXT,                   -- Argon2id; manager-approval PIN (docs/spec/security-rbac.md)
    is_active      BOOLEAN NOT NULL DEFAULT TRUE,
    config_version INTEGER NOT NULL DEFAULT 0,  -- catalog/config version for cloud→edge push (§50.1)
    created_at     TIMESTAMPTZ NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL,
    UNIQUE (tenant_id, email)
);

-- The 15 roles of docs/spec/security-rbac.md, seeded per tenant.
CREATE TABLE role (
    id          UUID PRIMARY KEY,
    tenant_id   UUID NOT NULL REFERENCES tenant(id),
    code        TEXT NOT NULL,             -- OUTLET_MANAGER | CASHIER | CHEF | ...
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    UNIQUE (tenant_id, code)
);

CREATE TABLE role_permission (
    role_id     UUID NOT NULL REFERENCES role(id) ON DELETE CASCADE,
    permission  TEXT NOT NULL,             -- 'order.create', 'bill.discount', ... (Permission enum in src/types/identity.ts)
    PRIMARY KEY (role_id, permission)
);

-- Role assignment is scoped: outlet_id NULL means tenant-wide (e.g. Organisation
-- Owner). A NULL column cannot participate in a primary key, so the identity is
-- a surrogate id and uniqueness is enforced by two partial indexes below.
CREATE TABLE user_role (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    role_id     UUID NOT NULL REFERENCES role(id),
    outlet_id   UUID REFERENCES outlet(id),
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX uq_user_role_outlet_scoped
    ON user_role (user_id, role_id, outlet_id)
    WHERE outlet_id IS NOT NULL;

CREATE UNIQUE INDEX uq_user_role_tenant_wide
    ON user_role (user_id, role_id)
    WHERE outlet_id IS NULL;

-- Pure configuration: the physical table's definition. Cloud is the source of
-- truth and it syncs cloud→edge, versioned and replaced (§50.1). Operational
-- state lives in table_session, never here (ADR-011).
CREATE TABLE restaurant_table (
    id             UUID PRIMARY KEY,
    outlet_id      UUID NOT NULL REFERENCES outlet(id),
    section        TEXT NOT NULL,          -- floor / zone, e.g. 'GROUND', 'TERRACE'
    label          TEXT NOT NULL,          -- 'T4', 'G12'
    seat_count     INTEGER NOT NULL CHECK (seat_count > 0),
    is_active      BOOLEAN NOT NULL DEFAULT TRUE,
    config_version INTEGER NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL,
    UNIQUE (outlet_id, label)
);

-- Operational aggregate: one seating of one table. Edge-authoritative,
-- append-only replay edge→cloud (§50.1) — the cloud never mutates these rows
-- except by applying a replayed edge event.
CREATE TABLE table_session (
    id               UUID PRIMARY KEY,
    outlet_id        UUID NOT NULL REFERENCES outlet(id),
    table_id         UUID NOT NULL REFERENCES restaurant_table(id),
    state            TEXT NOT NULL CHECK (state IN
                       ('OCCUPIED','ORDERED','KOT_SENT','FOOD_READY',
                        'BILL_REQUESTED','PAYMENT_PENDING','PAID','DIRTY','CLOSED')),
    current_order_id UUID REFERENCES "order"(id),
    guest_count      INTEGER NOT NULL CHECK (guest_count > 0),
    opened_by_user_id UUID REFERENCES app_user(id),
    opened_at        TIMESTAMPTZ NOT NULL,
    closed_at        TIMESTAMPTZ,
    version          INTEGER NOT NULL DEFAULT 1,
    created_at       TIMESTAMPTZ NOT NULL,
    updated_at       TIMESTAMPTZ NOT NULL,
    received_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- At most one open session per physical table.
CREATE UNIQUE INDEX uq_table_session_open
    ON table_session (table_id)
    WHERE closed_at IS NULL;

-- docs/spec/security-rbac.md §Audit — who, what, when, where, device, old, new,
-- reason. old_value/new_value must never contain credential material: the audit
-- helper redacts password_hash and pin_hash before writing (ADR-011).
CREATE TABLE audit_event (
    id            UUID PRIMARY KEY,
    tenant_id     UUID NOT NULL REFERENCES tenant(id),
    outlet_id     UUID REFERENCES outlet(id),
    actor_user_id UUID REFERENCES app_user(id),
    device_id     UUID,
    action        TEXT NOT NULL,
    entity_type   TEXT NOT NULL,
    entity_id     UUID,
    old_value     JSONB,
    new_value     JSONB,
    reason        TEXT,
    occurred_at   TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_app_user_tenant_id ON app_user(tenant_id);
CREATE INDEX idx_role_tenant_id ON role(tenant_id);
CREATE INDEX idx_user_role_user_id ON user_role(user_id);
CREATE INDEX idx_user_role_outlet_id ON user_role(outlet_id);
CREATE INDEX idx_restaurant_table_outlet_id ON restaurant_table(outlet_id);
CREATE INDEX idx_table_session_outlet_id ON table_session(outlet_id);
CREATE INDEX idx_table_session_table_id ON table_session(table_id);
CREATE INDEX idx_audit_event_tenant_occurred ON audit_event(tenant_id, occurred_at DESC);
