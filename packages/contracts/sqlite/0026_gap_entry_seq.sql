-- Holler Edge SQLite — stock_deduction_gap.entry_seq. Contracts 0.5.8,
-- ADR-018 replay addendum.
--
-- MIRRORED IN BOTH STORES (postgres/0026_gap_entry_seq.sql). The cloud
-- RECEIVES this column and needs it: contiguity of the gap stream is checked
-- against it exactly as the ledger stream's is. Declaring this file
-- single-store would be wrong, and is the reason the cursors shipped
-- separately as 0025 -- see that file's header.
--
-- WHY THE COLUMN EXISTS. Under ranged sync the gap stream is replayed by
-- `entry_seq > cursor`, so a row with no mark can never be selected, never be
-- acked, and never be missed. A gap row is a signal that a sale went
-- unaccounted; a signal that cannot reach the cloud is not a signal.
--
-- ============================================================================
-- WHY A REBUILD AND NOT `ADD COLUMN ... NOT NULL DEFAULT 0`
-- ============================================================================
--
-- SQLite's ADD COLUMN requires a default for a NOT NULL column, the default
-- must be constant, and the added column may carry no UNIQUE constraint at
-- all -- so the shortcut is necessarily two steps: add the column with a
-- constant, then build the unique index. Every pre-existing row then holds
-- the SAME value, and the index build fails on the second gap row of any
-- outlet. On an EMPTY table both steps succeed and the migration looks
-- perfectly correct; on a real outlet's database it aborts, at open, and the
-- POS does not start. Falsified against a populated table (see
-- gap_entry_seq_backfills_in_sequence_on_a_populated_table).
--
-- The rebuild is the idiom 0024 already established here: this table is small
-- by design (gaps are rare), carries no triggers, and nothing holds an FK
-- pointing at it.
CREATE TABLE stock_deduction_gap_new (
    id                  TEXT PRIMARY KEY,
    outlet_id           TEXT NOT NULL REFERENCES outlet(id),

    -- The per-outlet monotonic replay mark, minted by
    -- stock_deduction_gap_sequence (0025) in the same transaction as the
    -- insert. 1-based: cursors default to 0 meaning "nothing acked".
    entry_seq           INTEGER NOT NULL,

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
    business_date       TEXT NOT NULL,

    -- The ledger's key, for the ledger's reason (0016): the mark is what the
    -- cloud's contiguity check reads, so a duplicate would make it ambiguous.
    UNIQUE (outlet_id, entry_seq)
);

-- BACKFILL IN SEQUENCE, per outlet, oldest first -- not a constant. Existing
-- rows predate the mark and have never been replayed under it; numbering them
-- 1..N in occurrence order is the same stream the edge would have produced
-- had the column always existed. `id` breaks ties so the result is
-- deterministic across a re-run.
INSERT INTO stock_deduction_gap_new
    (id, outlet_id, entry_seq, order_id, order_item_id, menu_item_id,
     menu_item_variant_id, menu_item_name, quantity, reason, occurred_at,
     business_date)
SELECT
    id, outlet_id,
    ROW_NUMBER() OVER (PARTITION BY outlet_id ORDER BY occurred_at, id),
    order_id, order_item_id, menu_item_id, menu_item_variant_id,
    menu_item_name, quantity, reason, occurred_at, business_date
FROM stock_deduction_gap;

DROP TABLE stock_deduction_gap;
ALTER TABLE stock_deduction_gap_new RENAME TO stock_deduction_gap;

CREATE INDEX idx_stock_deduction_gap_outlet_date
    ON stock_deduction_gap(outlet_id, business_date);
CREATE INDEX idx_stock_deduction_gap_item
    ON stock_deduction_gap(menu_item_id);

-- SEED THE COUNTER TO WHERE THE BACKFILL LEFT IT, in this same migration.
-- Without this the next minted gap restarts at 1 and collides with a
-- backfilled row on the UNIQUE key -- the failure would land on the first
-- unaccounted sale after an upgrade, inside confirm_order's transaction,
-- which is precisely where ADR-018 Rule 2 forbids a failure.
INSERT INTO stock_deduction_gap_sequence (outlet_id, last_value, updated_at)
SELECT outlet_id, MAX(entry_seq), MAX(occurred_at)
FROM stock_deduction_gap
GROUP BY outlet_id
ON CONFLICT(outlet_id) DO UPDATE SET
    last_value = MAX(stock_deduction_gap_sequence.last_value, excluded.last_value),
    updated_at = excluded.updated_at;
