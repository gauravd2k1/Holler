-- Milestone 0 skeleton migration: minimal menu/order tables so the backend
-- and edge database shapes exist for early wiring. These are placeholders —
-- the authoritative, versioned schema is defined in packages/contracts/ and
-- frozen at Milestone 0.5 (ADR-008). Do not add business logic against this
-- migration before Milestone 0.5 lands.

CREATE TABLE menu_items (
    id          UUID PRIMARY KEY,
    outlet_id   UUID NOT NULL REFERENCES outlets(id),
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE orders (
    id          UUID PRIMARY KEY,
    outlet_id   UUID NOT NULL REFERENCES outlets(id),
    status      TEXT NOT NULL DEFAULT 'DRAFT',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_menu_items_outlet_id ON menu_items(outlet_id);
CREATE INDEX idx_orders_outlet_id ON orders(outlet_id);
