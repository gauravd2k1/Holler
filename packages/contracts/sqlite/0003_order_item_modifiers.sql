-- Holler Edge SQLite — order line modifier snapshots (contracts 0.2.3).
-- Additive to 0001/0002; nothing earlier is altered. See the ADR-011 addendum.
--
-- WHY THIS EXISTS: OrderItem has carried a `modifiers` array since 0.1.0 and
-- fixtures/order.json round-trips one through both languages, but no table ever
-- held them. Every ItemAdded event therefore replayed an empty modifier list,
-- silently dropping "Large / Cheese Burst / extra paneer" on the way to the
-- cloud. The wire contract promised fidelity the storage could not deliver.
--
-- WHY A TYPED TABLE, NOT A modifiers_json COLUMN: these rows carry money
-- (price_delta_paise) and must survive replay with the same fidelity as the
-- line itself. ADR-008's typed-tables-over-JSONB rule applies to financial
-- line data.
--
-- IDs are application-generated UUIDv7 (§74), edge-assigned.

CREATE TABLE order_item_modifier (
    id                 TEXT PRIMARY KEY,
    order_item_id      TEXT NOT NULL REFERENCES order_item(id) ON DELETE CASCADE,
    -- menu_item_modifier.id as it was at order time. Deliberately NOT a foreign
    -- key: the catalog is config and may be replaced wholesale by a later
    -- config_version (§50.1), and a completed order's snapshot must never move
    -- because the menu changed underneath it.
    modifier_id        TEXT NOT NULL,
    group_name         TEXT NOT NULL,          -- snapshot; the catalog may rename it later
    option_name        TEXT NOT NULL,          -- snapshot
    price_delta_paise  INTEGER NOT NULL,       -- snapshot, integer paise, never float
    created_at         TEXT NOT NULL
);

-- MONEY INVARIANT — one definition, so the edge recompute path and the cloud
-- replay cannot diverge:
--   unit_price_paise = snapshot of menu_item.base_price_paise + variant delta
--   line_total_paise = (unit_price_paise + SUM(price_delta_paise)) * quantity
CREATE INDEX idx_order_item_modifier_order_item_id ON order_item_modifier(order_item_id);
