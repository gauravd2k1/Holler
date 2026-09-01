-- contracts 0.6.3 (ADR-021) — stock_ledger_entry.line_total_paise
--
-- WHY. `unit_cost_paise` is a per-BASE-UNIT RATE, and it is rounded to whole
-- paise once per receipt in `edge/database/src/procurement/convert.rs`. Weighted
-- average cost was then derived by summing `quantity x rate`, so it inherited a
-- rounding the ledger had already committed to and could never recover.
--
-- The error is +/- 0.5 paise on a per-gram figure, so its RELATIVE size scales
-- inversely with price: 9.5 -> 10 is +5.3%, 4.5 -> 5 is +11.1%, 2.5 -> 3 is
-- +20%. It is one-directional per item and worst exactly where an outlet buys
-- most of its weight — cheap staples. Storing the money the invoice actually
-- said removes the intermediate rounding instead of rescaling around it, and
-- lets `procurement::cost` divide exactly once, at the end.
--
-- WHAT SETS IT. Receipts only. A receipt has an invoiced total, so the total is
-- the fact and the rate is derived from it. Wastage, count adjustments, variance
-- and outbound movements have NO invoiced total — they are valued AT the average
-- — so writing a `quantity x rate` product for them would fabricate a precision
-- that does not exist and then feed it back into the average that produced it.
-- Those rows leave this column NULL, which is why the CHECK below is
-- DIRECTIONAL rather than a strict pairing.
--
-- THE REBUILD MUST CARRY THE TRIGGERS AND INDEXES BACK. `DROP TABLE` takes them
-- with it, silently, and nothing in the suite tries to UPDATE a ledger row
-- expecting to fail — so a rebuild that forgets them leaves the ledger mutable
-- and unbounded while every test still passes. 0029 having done this correctly
-- is not evidence that 0030 does: `edge/database/src/migrations.rs` asserts
-- after this migration that the insert-only guard actually FIRES, and that the
-- durable `stock_ledger_sequence` counter still leads `MAX(entry_seq)`.
--
-- (The wording above is deliberate. The claim lint attributes a phrase to the
-- nearest table by LINE DISTANCE, so spelling the property out here would
-- attach it to the transient _rebuild table defined below and fail. 0029 words
-- it the same way for the same reason. That fragility is a filed backlog item,
-- not a thing this file should pretend is precise.)

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

    -- The derived per-base-unit RATE. Still written, still displayed, still the
    -- figure a purchase return and an outbound transfer are valued at — but no
    -- longer an input to the weighted average. `packages/contracts` pins the
    -- relationship between the two with a drift test rather than a trigger,
    -- because the honest expression only runs one way (total -> rate) and only
    -- on a receipt.
    unit_cost_paise     INTEGER,

    -- The EXACT money this row is worth, unrounded, as invoiced. NULL on every
    -- origin that has no invoiced total.
    line_total_paise    INTEGER,

    source_stock_count_id TEXT,
    source_grn_id       TEXT REFERENCES goods_receipt_note(id),
    source_purchase_return_id TEXT REFERENCES purchase_return(id),
    source_stock_transfer_out_id TEXT REFERENCES stock_transfer_out(id),

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

    -- DIRECTIONAL, not a strict pairing. A total never appears without its
    -- rate; a rate may stand alone. A strict pairing would reject every
    -- wastage, count and variance row — the ones valued at the average with no
    -- invoice behind them — which is the majority of the ledger.
    CONSTRAINT stock_ledger_cost_pairs_with_total
        CHECK (line_total_paise IS NULL OR unit_cost_paise IS NOT NULL),

    UNIQUE (outlet_id, entry_seq)
);

-- Columns named explicitly, never SELECT *: a rebuild that relies on column
-- order is one ALTER away from silently transposing two values of the same
-- type, and every provenance column here is TEXT.
--
-- THE BACKFILL RECONSTRUCTS, IT DOES NOT RECOVER. Pre-0.6.3 rows never stored
-- the invoiced total, so the only figure available is `quantity x rate` — and
-- the rate is exactly the number this migration exists because it was rounded.
-- These values are therefore AS ACCURATE AS THE OLD PATH AND NO MORE. They are
-- not recovered truth, and the CHECK above asserts that the two columns pair,
-- never that the total is the money anyone was actually billed.
--
-- ROUNDING IS HALF AWAY FROM ZERO, matching every other rounding on this path
-- (ADR-018 §5). Integer division in SQLite truncates TOWARD ZERO, which would
-- round a positive row down and a NEGATIVE row up — a silent asymmetry across
-- the sign, on a table whose outbound rows are all negative. The expression
-- below adds half a unit in the direction of the row's own sign before
-- truncating, so both signs round away from zero:
--
--     positive:  (q*c + 500000) / 1000000
--     negative:  (q*c - 500000) / 1000000
--
-- Only rows with a rate are touched; everything else stays NULL and satisfies
-- the directional CHECK.
INSERT INTO stock_ledger_entry_rebuild (
    id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
    entry_type, origin, quantity_applied_micro, recipe_id, recipe_version,
    recipe_name, source_order_id, source_order_item_id, reason_code, note,
    occurred_at, business_date, created_by_user_id, modifier_delta_id,
    modifier_name, modifier_delta_version, unit_cost_paise, line_total_paise,
    source_stock_count_id, source_grn_id, source_purchase_return_id,
    source_stock_transfer_out_id
)
SELECT
    id, outlet_id, entry_seq, inventory_item_id, inventory_item_name, dimension,
    entry_type, origin, quantity_applied_micro, recipe_id, recipe_version,
    recipe_name, source_order_id, source_order_item_id, reason_code, note,
    occurred_at, business_date, created_by_user_id, modifier_delta_id,
    modifier_name, modifier_delta_version, unit_cost_paise,
    CASE
        WHEN unit_cost_paise IS NULL THEN NULL
        WHEN quantity_applied_micro * unit_cost_paise >= 0
            THEN (quantity_applied_micro * unit_cost_paise + 500000) / 1000000
        ELSE (quantity_applied_micro * unit_cost_paise - 500000) / 1000000
    END,
    source_stock_count_id, source_grn_id, source_purchase_return_id,
    source_stock_transfer_out_id
FROM stock_ledger_entry;

DROP TABLE stock_ledger_entry;

ALTER TABLE stock_ledger_entry_rebuild RENAME TO stock_ledger_entry;

-- Indexes, restored verbatim from 0016, 0023 and 0029.
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
