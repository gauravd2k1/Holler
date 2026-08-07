-- Holler Edge SQLite schema — frozen at Milestone 0.5 (ADR-008).
-- Source of truth for cross-boundary shapes; mirrored by postgres/0001_init.sql
-- (cloud), src/types/*.ts (TypeScript+Zod), and go/*.go (Go structs).
-- See ADR-008 amendment for the typed-tables-over-JSONB rationale.
--
-- WAL mode is enabled by the edge/database service at connection time
-- (ADR-003), not in this migration.

CREATE TABLE outlet (
    id                  TEXT PRIMARY KEY,       -- UUIDv7, matches cloud outlets.id
    brand_id            TEXT NOT NULL,
    name                TEXT NOT NULL,
    timezone            TEXT NOT NULL DEFAULT 'Asia/Kolkata',
    config_version      INTEGER NOT NULL DEFAULT 0,  -- last applied authorized catalog/config version (sync.md §50.1)
    created_at          TEXT NOT NULL,           -- ISO8601 UTC
    updated_at          TEXT NOT NULL
);

CREATE TABLE device (
    id                  TEXT PRIMARY KEY,       -- UUIDv7
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    kind                TEXT NOT NULL CHECK (kind IN ('POS','KDS','WAITER','PRINTER_GATEWAY')),
    name                TEXT NOT NULL,
    last_seen_at        TEXT,
    created_at          TEXT NOT NULL
);

CREATE TABLE menu_category (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    name                TEXT NOT NULL,
    sort_order          INTEGER NOT NULL DEFAULT 0,
    config_version      INTEGER NOT NULL         -- catalog version this row reflects (replace-not-merge, §50.1)
);

CREATE TABLE menu_item (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    category_id         TEXT NOT NULL REFERENCES menu_category(id),
    name                TEXT NOT NULL,
    base_price_paise    INTEGER NOT NULL,        -- integer paise, never float (CLAUDE.md)
    is_available        INTEGER NOT NULL DEFAULT 1 CHECK (is_available IN (0,1)), -- item-snooze flag, §19
    config_version      INTEGER NOT NULL
);

CREATE TABLE menu_item_variant (
    id                  TEXT PRIMARY KEY,
    menu_item_id        TEXT NOT NULL REFERENCES menu_item(id),
    name                TEXT NOT NULL,
    price_delta_paise   INTEGER NOT NULL DEFAULT 0,
    config_version      INTEGER NOT NULL
);

CREATE TABLE menu_item_modifier (
    id                  TEXT PRIMARY KEY,
    menu_item_id        TEXT NOT NULL REFERENCES menu_item(id),
    group_name          TEXT NOT NULL,
    option_name         TEXT NOT NULL,
    price_delta_paise   INTEGER NOT NULL DEFAULT 0,
    min_selection       INTEGER NOT NULL DEFAULT 0,
    max_selection       INTEGER NOT NULL DEFAULT 1,
    config_version      INTEGER NOT NULL
);

-- "order" is a reserved word in SQL; quoted throughout.
CREATE TABLE "order" (
    id                  TEXT PRIMARY KEY,       -- UUIDv7 = CanonicalOrder.holler_order_id
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    device_id           TEXT NOT NULL REFERENCES device(id),
    order_type          TEXT NOT NULL CHECK (order_type IN
                          ('DINE_IN','TAKEAWAY','DELIVERY','AGGREGATOR','QR','ROOM_SERVICE','CATERING')),
    status              TEXT NOT NULL DEFAULT 'DRAFT',  -- see docs/domain/ORDER_STATE_MACHINE.md
    table_id            TEXT,                    -- nullable; dine-in only
    subtotal_paise      INTEGER NOT NULL DEFAULT 0,
    discount_paise      INTEGER NOT NULL DEFAULT 0,
    tax_paise           INTEGER NOT NULL DEFAULT 0,
    total_paise         INTEGER NOT NULL DEFAULT 0,
    version             INTEGER NOT NULL DEFAULT 1,     -- optimistic concurrency, sync envelope field
    sync_status         TEXT NOT NULL DEFAULT 'PENDING' CHECK (sync_status IN ('PENDING','SYNCED','FAILED')),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE order_item (
    id                  TEXT PRIMARY KEY,
    order_id            TEXT NOT NULL REFERENCES "order"(id),
    menu_item_id        TEXT NOT NULL REFERENCES menu_item(id),
    variant_id          TEXT REFERENCES menu_item_variant(id),
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_paise    INTEGER NOT NULL,        -- snapshot at order time; never recomputed from live menu
    line_total_paise    INTEGER NOT NULL,
    notes               TEXT,
    created_at          TEXT NOT NULL
);

-- One row per station ticket, not per order — see docs/spec/kitchen.md.
CREATE TABLE kot (
    id                    TEXT PRIMARY KEY,
    order_id              TEXT NOT NULL REFERENCES "order"(id),
    station                TEXT NOT NULL,          -- MAIN_KITCHEN | TANDOOR | BAR | DESSERT | ...
    sequence               INTEGER NOT NULL,       -- e.g. 1 for #132, 2 for #132-A (addition), 3 for #132-C (cancellation)
    status                 TEXT NOT NULL DEFAULT 'NEW' CHECK (status IN
                            ('NEW','ACKNOWLEDGED','PREPARING','READY','SERVED','CANCELLED')),
    items_json            TEXT NOT NULL,           -- denormalized ticket snapshot: [{order_item_id,name,qty,modifiers[],notes}]
    created_by_device_id  TEXT NOT NULL REFERENCES device(id),
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

-- Transactional outbox (ADR-007) — written in the same local transaction as
-- the order/kot rows it describes.
CREATE TABLE local_outbox (
    id                  TEXT PRIMARY KEY,       -- UUIDv7
    aggregate_type      TEXT NOT NULL,           -- 'order' | 'kot' | ...
    aggregate_id        TEXT NOT NULL,
    event_type          TEXT NOT NULL,           -- OrderCreated | ItemAdded | KOTCreated | OrderReady | ...
    payload_json        TEXT NOT NULL,           -- matches src/types/events.ts / go/events.go
    created_at          TEXT NOT NULL,
    published_at        TEXT,                    -- NULL until confirmed sent
    attempt_count       INTEGER NOT NULL DEFAULT 0
);

-- Per-outlet sync cursor bookkeeping for both directions (sync.md §50.1).
CREATE TABLE sync_state (
    outlet_id                     TEXT PRIMARY KEY REFERENCES outlet(id),
    last_pushed_outbox_id         TEXT,           -- high-water mark, edge → cloud
    last_applied_config_version   INTEGER NOT NULL DEFAULT 0, -- high-water mark, cloud → edge
    last_sync_attempt_at          TEXT,
    last_sync_success_at          TEXT,
    is_online                     INTEGER NOT NULL DEFAULT 0 CHECK (is_online IN (0,1))
);

CREATE INDEX idx_device_outlet_id ON device(outlet_id);
CREATE INDEX idx_menu_category_outlet_id ON menu_category(outlet_id);
CREATE INDEX idx_menu_item_outlet_id ON menu_item(outlet_id);
CREATE INDEX idx_menu_item_category_id ON menu_item(category_id);
CREATE INDEX idx_menu_item_variant_menu_item_id ON menu_item_variant(menu_item_id);
CREATE INDEX idx_menu_item_modifier_menu_item_id ON menu_item_modifier(menu_item_id);
CREATE INDEX idx_order_outlet_id ON "order"(outlet_id);
CREATE INDEX idx_order_sync_status ON "order"(sync_status);
CREATE INDEX idx_order_item_order_id ON order_item(order_id);
CREATE INDEX idx_kot_order_id ON kot(order_id);
CREATE INDEX idx_kot_station_status ON kot(station, status);
CREATE INDEX idx_local_outbox_published_at ON local_outbox(published_at);
