-- Holler Cloud PostgreSQL — order line modifier snapshots (contracts 0.2.3).
-- Mirrors sqlite/0003_order_item_modifiers.sql. Additive; nothing earlier is
-- altered. See the ADR-011 addendum.
--
-- The cloud REPLAYS these rows from the edge (§50.1) — it never originates or
-- recomputes them. Part of the `order` operational aggregate, so no new
-- AggregateType entry.
--
-- IDs are application-generated UUIDv7 (§74), assigned edge-side.

CREATE TABLE order_item_modifier (
    id                 UUID PRIMARY KEY,
    order_item_id      UUID NOT NULL REFERENCES order_item(id) ON DELETE CASCADE,
    -- menu_item_modifier.id at order time. Deliberately NOT a foreign key: the
    -- catalog is config, replaced wholesale by config_version (§50.1), and a
    -- completed order's snapshot must not move because the menu changed.
    modifier_id        UUID NOT NULL,
    group_name         TEXT NOT NULL,          -- snapshot
    option_name        TEXT NOT NULL,          -- snapshot
    price_delta_paise  INTEGER NOT NULL,       -- snapshot, integer paise, never float
    created_at         TIMESTAMPTZ NOT NULL
);

-- MONEY INVARIANT — identical to the SQLite side:
--   unit_price_paise = snapshot of menu_item.base_price_paise + variant delta
--   line_total_paise = (unit_price_paise + SUM(price_delta_paise)) * quantity
CREATE INDEX idx_order_item_modifier_order_item_id ON order_item_modifier(order_item_id);
