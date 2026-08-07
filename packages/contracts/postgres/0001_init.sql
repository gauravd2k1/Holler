-- Holler Cloud PostgreSQL schema — frozen at Milestone 0.5 (ADR-008).
-- Mirrors packages/contracts/sqlite/0001_init.sql for the tenant/menu/order
-- vertical slice. This supersedes the placeholder migrations under
-- backend/migrations/ as the authoritative schema source; backend/migrations/
-- should be reconciled to match on next backend touch (tracked, not silently
-- diverged).

CREATE EXTENSION IF NOT EXISTS pgcrypto; -- gen_random_uuid()

CREATE TABLE tenant (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE brand (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenant(id),
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE outlet (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    brand_id        UUID NOT NULL REFERENCES brand(id),
    name            TEXT NOT NULL,
    timezone        TEXT NOT NULL DEFAULT 'Asia/Kolkata',
    config_version  INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE menu_category (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    outlet_id       UUID NOT NULL REFERENCES outlet(id),
    name            TEXT NOT NULL,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    config_version  INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE menu_item (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    outlet_id       UUID NOT NULL REFERENCES outlet(id),
    category_id     UUID NOT NULL REFERENCES menu_category(id),
    name            TEXT NOT NULL,
    base_price_paise INTEGER NOT NULL,
    is_available    BOOLEAN NOT NULL DEFAULT true,
    config_version  INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE menu_item_variant (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    menu_item_id        UUID NOT NULL REFERENCES menu_item(id),
    name                TEXT NOT NULL,
    price_delta_paise   INTEGER NOT NULL DEFAULT 0,
    config_version      INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE menu_item_modifier (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    menu_item_id        UUID NOT NULL REFERENCES menu_item(id),
    group_name          TEXT NOT NULL,
    option_name         TEXT NOT NULL,
    price_delta_paise   INTEGER NOT NULL DEFAULT 0,
    min_selection       INTEGER NOT NULL DEFAULT 0,
    max_selection       INTEGER NOT NULL DEFAULT 1,
    config_version      INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE "order" (
    id                  UUID PRIMARY KEY,          -- assigned edge-side (UUIDv7); cloud never generates order ids
    outlet_id           UUID NOT NULL REFERENCES outlet(id),
    device_id           UUID NOT NULL,
    order_type          TEXT NOT NULL CHECK (order_type IN
                          ('DINE_IN','TAKEAWAY','DELIVERY','AGGREGATOR','QR','ROOM_SERVICE','CATERING')),
    status              TEXT NOT NULL DEFAULT 'DRAFT',
    table_id            UUID,
    subtotal_paise      INTEGER NOT NULL DEFAULT 0,
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    tax_paise           INTEGER NOT NULL DEFAULT 0,
    total_paise         INTEGER NOT NULL DEFAULT 0,
    version             INTEGER NOT NULL DEFAULT 1,
    source_payload      JSONB,                     -- raw external payload, audit only (never core relational data)
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    received_at         TIMESTAMPTZ NOT NULL DEFAULT now() -- when cloud received the replayed edge event
);

CREATE TABLE order_item (
    id                  UUID PRIMARY KEY,
    order_id            UUID NOT NULL REFERENCES "order"(id),
    menu_item_id        UUID NOT NULL REFERENCES menu_item(id),
    variant_id          UUID REFERENCES menu_item_variant(id),
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_paise    INTEGER NOT NULL,
    line_total_paise    INTEGER NOT NULL,
    notes               TEXT,
    created_at          TIMESTAMPTZ NOT NULL
);

CREATE TABLE kot (
    id                    UUID PRIMARY KEY,
    order_id              UUID NOT NULL REFERENCES "order"(id),
    station                TEXT NOT NULL,
    sequence               INTEGER NOT NULL,
    status                 TEXT NOT NULL DEFAULT 'NEW' CHECK (status IN
                            ('NEW','ACKNOWLEDGED','PREPARING','READY','SERVED','CANCELLED')),
    items_json            JSONB NOT NULL,
    created_by_device_id  UUID NOT NULL,
    created_at            TIMESTAMPTZ NOT NULL,
    updated_at            TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_brand_tenant_id ON brand(tenant_id);
CREATE INDEX idx_outlet_brand_id ON outlet(brand_id);
CREATE INDEX idx_menu_category_outlet_id ON menu_category(outlet_id);
CREATE INDEX idx_menu_item_outlet_id ON menu_item(outlet_id);
CREATE INDEX idx_menu_item_category_id ON menu_item(category_id);
CREATE INDEX idx_order_outlet_id ON "order"(outlet_id);
CREATE INDEX idx_order_item_order_id ON order_item(order_id);
CREATE INDEX idx_kot_order_id ON kot(order_id);
