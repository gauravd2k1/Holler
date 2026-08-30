-- Holler Edge SQLite — stock_ledger_entry.origin learns procurement.
-- Contracts 0.6.2, ADR-019 addendum.
--
-- ---------------------------------------------------------------------------
-- WHY THIS IS NOT DEFERRED TO M6
-- ---------------------------------------------------------------------------
--
-- Every procurement ledger row written since 0.6.0 says origin = 'MANUAL'.
-- That is FALSE, it is written into a table that permits only inserts, and it
-- cannot be corrected later by an edit — only by appending a second row that
-- contradicts the first.
--
-- The stopping rule is not "does this change touch existing rows" but "does
-- the INTERIM write rows that would need rewriting." It does, at every receipt,
-- return and dispatch, on every outlet, for as long as the deferral lasts. That
-- is the argument that landed UNRESOLVABLE_REFERENCE at once rather than in the
-- next milestone (ADR-018 addendum 0.5.3), and it applies here unchanged.
--
-- 'MANUAL' also actively lies to the one reader that exists: a variance report
-- grouping by origin cannot separate a delivery from a hand adjustment, which
-- is the exact distinction the column was added to preserve ("a CONSUMPTION
-- posted by a recipe and one posted by a modifier delta are the same entry_type
-- and different facts").
--
-- ---------------------------------------------------------------------------
-- WHY A TABLE REBUILD, WHEN 0021 DELIBERATELY AVOIDED ONE
-- ---------------------------------------------------------------------------
--
-- 0021_stock_ledger_sequence.sql chose triggers over a rebuild and said so:
-- "SQLite cannot ADD CONSTRAINT to an existing table, and rebuilding four
-- tables to add a bound would be a far larger change than the bound is worth."
--
-- That reasoning does not reach this change, and the difference is the
-- direction of the constraint. A trigger can only ADD a restriction. This
-- migration must LOOSEN one — three values that the CHECK currently rejects
-- have to become legal — and no trigger can widen a CHECK that is already
-- compiled into the table. So the table is rebuilt, which is the same
-- rebuild-and-backfill discipline 0.5.8 used for entry_seq.
--
-- TWO CHECKS MOVE, NOT ONE. The obvious one is the origin enum. The second is
-- the provenance CHECK below, which pins each origin to the shape of its
-- companion columns; its 'MANUAL','COUNT_ADJUSTMENT','WASTAGE' branch is what
-- would otherwise reject every new member at insert time with a message about
-- recipe_id. Extending only the enum would produce a migration that applies
-- cleanly and then fails on the first receipt.
--
-- THE REBUILD MUST CARRY THE TRIGGERS BACK. DROP TABLE takes its triggers and
-- indexes with it, silently. stock_ledger_entry has three triggers — two
-- insert-only guards and the 1e15 quantity bound — and three indexes. A
-- rebuild that forgets them leaves the ledger mutable and unbounded while every
-- test still passes, because nothing in the suite tries to UPDATE a ledger row
-- expecting to fail. They are all recreated below, verbatim.

PRAGMA foreign_keys = OFF;

CREATE TABLE stock_ledger_entry_rebuild (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),
    entry_seq           INTEGER NOT NULL,
    inventory_item_id   TEXT NOT NULL,
    inventory_item_name TEXT NOT NULL,
    dimension           TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),
    entry_type          TEXT NOT NULL CHECK (entry_type IN (
                            'PURCHASE','CONSUMPTION','WASTAGE',
                            'TRANSFER_IN','TRANSFER_OUT','ADJUSTMENT',
                            'RETURN_TO_VENDOR',
                            'PRODUCTION_CONSUMPTION','PRODUCTION_OUTPUT')),

    -- The three new members are one per provenance column already on the row
    -- (source_grn_id, source_purchase_return_id, source_stock_transfer_out_id),
    -- so origin and provenance cannot disagree about which document produced
    -- the movement.
    origin              TEXT NOT NULL CHECK (origin IN (
                            'RECIPE','MODIFIER_DELTA','MANUAL',
                            'COUNT_ADJUSTMENT','WASTAGE',
                            'GOODS_RECEIPT','PURCHASE_RETURN','STOCK_TRANSFER')),

    quantity_applied_micro INTEGER NOT NULL,
    recipe_id           TEXT,
    recipe_version      INTEGER,
    recipe_name         TEXT,
    source_order_id     TEXT,
    source_order_item_id TEXT,
    reason_code         TEXT,
    note                TEXT,
    occurred_at         TEXT NOT NULL,
    business_date       TEXT NOT NULL,
    created_by_user_id  TEXT,
    modifier_delta_id       TEXT,
    modifier_name           TEXT,
    modifier_delta_version  INTEGER,
    unit_cost_paise     INTEGER,
    source_stock_count_id TEXT,
    source_grn_id       TEXT REFERENCES goods_receipt_note(id),
    source_purchase_return_id TEXT REFERENCES purchase_return(id),
    source_stock_transfer_out_id TEXT REFERENCES stock_transfer_out(id),

    -- The provenance CHECK, extended. The three procurement origins join the
    -- "no recipe, no modifier" branch: a receipt is not attributable to a
    -- recipe or a modifier delta, and claiming otherwise would be the
    -- half-attributed deduction this CHECK exists to prevent.
    CHECK (
        (origin = 'RECIPE'
            AND recipe_id IS NOT NULL
            AND modifier_delta_id IS NULL)
     OR (origin = 'MODIFIER_DELTA'
            AND modifier_delta_id IS NOT NULL
            AND recipe_id IS NULL)
     OR (origin IN ('MANUAL','COUNT_ADJUSTMENT','WASTAGE',
                    'GOODS_RECEIPT','PURCHASE_RETURN','STOCK_TRANSFER')
            AND recipe_id IS NULL
            AND modifier_delta_id IS NULL)
    ),
    UNIQUE (outlet_id, entry_seq)
);

-- Columns named explicitly, never SELECT *: a rebuild that relies on column
-- order is one ALTER away from silently transposing two values of the same
-- type, and every provenance column here is TEXT.
INSERT INTO stock_ledger_entry_rebuild (
    id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
    entry_type, origin, quantity_applied_micro, recipe_id, recipe_version,
    recipe_name, source_order_id, source_order_item_id, reason_code, note,
    occurred_at, business_date, created_by_user_id, modifier_delta_id,
    modifier_name, modifier_delta_version, unit_cost_paise, source_stock_count_id,
    source_grn_id, source_purchase_return_id, source_stock_transfer_out_id
)
SELECT
    id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
    entry_type, origin, quantity_applied_micro, recipe_id, recipe_version,
    recipe_name, source_order_id, source_order_item_id, reason_code, note,
    occurred_at, business_date, created_by_user_id, modifier_delta_id,
    modifier_name, modifier_delta_version, unit_cost_paise, source_stock_count_id,
    source_grn_id, source_purchase_return_id, source_stock_transfer_out_id
FROM stock_ledger_entry;

DROP TABLE stock_ledger_entry;

ALTER TABLE stock_ledger_entry_rebuild RENAME TO stock_ledger_entry;

-- Indexes, restored verbatim from 0016 and 0023.
CREATE INDEX idx_stock_ledger_entry_item_date
    ON stock_ledger_entry(outlet_id, inventory_item_id, business_date);

CREATE INDEX idx_stock_ledger_entry_order
    ON stock_ledger_entry(source_order_id);

CREATE INDEX idx_stock_ledger_entry_source_count
    ON stock_ledger_entry(source_stock_count_id);

-- Triggers, restored verbatim from 0016 and 0021. Without these three the
-- rebuild would silently leave the ledger writable and unbounded.
CREATE TRIGGER stock_ledger_entry_is_append_only_no_update
BEFORE UPDATE ON stock_ledger_entry
BEGIN
    SELECT RAISE(ABORT,
        'stock_ledger_entry is append-only: correct it by appending an ADJUSTMENT entry, never UPDATE (ADR-018, contracts 0.5.0)');
END;

CREATE TRIGGER stock_ledger_entry_is_append_only_no_delete
BEFORE DELETE ON stock_ledger_entry
BEGIN
    SELECT RAISE(ABORT,
        'stock_ledger_entry is append-only: a ledger row is never deleted, append an ADJUSTMENT entry instead (ADR-018, contracts 0.5.0)');
END;

CREATE TRIGGER stock_ledger_entry_quantity_is_bounded
BEFORE INSERT ON stock_ledger_entry
WHEN ABS(NEW.quantity_applied_micro) > 1000000000000000
BEGIN
    SELECT RAISE(ABORT,
        'stock_ledger_entry.quantity_applied_micro exceeds 1e15 micro-units (a thousand tonnes of one ingredient in one row). That is bad data, not a runtime condition -- the bound exists so arithmetic overflow is unreachable rather than handled (ADR-018, contracts 0.5.3)');
END;

PRAGMA foreign_keys = ON;
