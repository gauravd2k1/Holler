-- Holler Edge SQLite — repair stock_deduction_gap's reason CHECK. Contracts
-- 0.5.6, ADR-018 addendum.
--
-- WHY. DIMENSION_MISMATCH was added to the TS enum, the Go enum and the
-- OpenAPI spec at 0.5.1, and to NEITHER store's CHECK constraint. Found by
-- T4's config-delivery guard work, verified by inspection of the applied
-- constraint in the dev database.
--
-- The consequence was worse than a missing label: the deduction path writes
-- gap rows inside confirm_order's transaction, and an INSERT rejected by this
-- CHECK propagates as a real database error -- so the first genuine
-- dimension-mismatch at a live outlet would have FAILED THE CONFIRM, the exact
-- outcome Rule 2 exists to forbid. No test caught it because every
-- DIMENSION_MISMATCH test stopped at the resolver, asserting the GapReason
-- value and never inserting it into the table.
--
-- Two lessons, both already named this milestone, both instanced again here:
-- an additive change has a consumer list (the enum grew in three places and
-- not the fourth), and a guard tested along the routes its author thought of
-- proves only its author's imagination (the falsifications tested acceptance
-- and rejection of reasons -- against a list that was itself wrong).
--
-- SQLite cannot alter a table CHECK, so this is the documented rebuild idiom:
-- the gap table is small (gaps are rare by design), carries no triggers and no
-- FKs pointing at it, so the rebuild is cheap and safe.
CREATE TABLE stock_deduction_gap_new (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    order_id            TEXT NOT NULL,
    order_item_id       TEXT NOT NULL,
    menu_item_id        TEXT NOT NULL,
    menu_item_variant_id TEXT,
    menu_item_name      TEXT NOT NULL,
    quantity            INTEGER NOT NULL CHECK (quantity > 0),
    reason              TEXT NOT NULL CHECK (reason IN (
                            'NO_RECIPE','NO_VARIANT','CYCLE',
                            'DEPTH_EXCEEDED','UNKNOWN_UNIT',
                            'DIMENSION_MISMATCH','UNRESOLVABLE_REFERENCE')),
    occurred_at         TEXT NOT NULL,
    business_date       TEXT NOT NULL
);

INSERT INTO stock_deduction_gap_new SELECT * FROM stock_deduction_gap;
DROP TABLE stock_deduction_gap;
ALTER TABLE stock_deduction_gap_new RENAME TO stock_deduction_gap;

CREATE INDEX idx_stock_deduction_gap_outlet_date
    ON stock_deduction_gap(outlet_id, business_date);
CREATE INDEX idx_stock_deduction_gap_item
    ON stock_deduction_gap(menu_item_id);
