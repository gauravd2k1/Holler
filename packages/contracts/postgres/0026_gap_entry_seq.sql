-- Holler Cloud PostgreSQL — stock_deduction_gap.entry_seq. Contracts 0.5.8,
-- ADR-018 replay addendum. Mirror of sqlite/0026_gap_entry_seq.sql.
--
-- WHY THIS SIDE NEEDS THE COLUMN. Under ranged sync the cloud checks
-- contiguity of the received stream against its own high-water mark. It can
-- only do that for a stream whose rows carry the mark, so this column is
-- received, stored, and read here -- it is not edge bookkeeping. The two
-- replay CURSORS are the edge-local half and live in sqlite/0025 alone.
--
-- SAME NOT-NULL TRAP AS SQLITE, DIFFERENT SPELLING. `ADD COLUMN entry_seq
-- BIGINT NOT NULL DEFAULT 0` writes the constant into every existing row and
-- then dies on the second row of any outlet once the UNIQUE key below is
-- added -- invisible against an empty table. Nullable, backfill in sequence,
-- THEN constrain.
ALTER TABLE stock_deduction_gap ADD COLUMN entry_seq BIGINT;

-- Backfill 1..N per outlet, oldest first, `id` breaking ties so a re-run is
-- deterministic and matches what SQLite's ROW_NUMBER produced.
WITH numbered AS (
    SELECT id,
           ROW_NUMBER() OVER (PARTITION BY outlet_id ORDER BY occurred_at, id) AS seq
    FROM stock_deduction_gap
)
UPDATE stock_deduction_gap g
SET entry_seq = numbered.seq
FROM numbered
WHERE g.id = numbered.id;

ALTER TABLE stock_deduction_gap ALTER COLUMN entry_seq SET NOT NULL;

ALTER TABLE stock_deduction_gap
    ADD CONSTRAINT stock_deduction_gap_outlet_entry_seq_key
    UNIQUE (outlet_id, entry_seq);
