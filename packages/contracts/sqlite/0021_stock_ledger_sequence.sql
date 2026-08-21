-- Holler Edge SQLite — stock_ledger_sequence, and a magnitude bound on every
-- stored quantity. Contracts 0.5.3, ADR-018 addendum.
--
-- ============================================================================
-- PART 1 — A DURABLE ENTRY_SEQ COUNTER
-- ============================================================================
--
-- EDGE-LOCAL. SQLite only, no PostgreSQL mirror, no AggregateType, no sync
-- direction — the `invoice_sequence` precedent exactly, and declared as such in
-- SINGLE_STORE_MIGRATIONS (edge/database/src/migrations.rs).
--
-- WHY. T2 assigned `entry_seq` as `MAX(entry_seq) + 1` over the surviving rows.
-- That is correct today, and it is correct ONLY because `stock_ledger_entry`
-- carries a no-delete trigger (0016).
--
-- **ADR-018 §9's retention design requires removing exactly that trigger.**
-- Archival deletes ledger rows once their replay is acked and a sealed snapshot
-- covers them. Both of those things are committed to this repository and they
-- cannot both be right:
--
--   * MAX+1 over surviving rows RESTARTS the sequence after any archival.
--   * A sealed `stock_balance_snapshot` stores `through_entry_seq = N` and
--     reads "everything not covered by the mark". Reused marks make that read
--     double-count or skip, silently, surfacing as unexplained variance months
--     later — the exact shape of every other defect this milestone removed.
--   * It also breaks the cloud gap detection `entry_seq` was added for:
--     reused marks produce both false positives (a gap that is not one) and
--     false negatives (a real loss hidden by a reused number).
--
-- A derived counter is a value that silently means something different after an
-- unrelated operation. That is the same argument as 0.5.1 and 0.5.2, arriving a
-- third time — here the unrelated operation is archival rather than an edit.
--
-- Taken in the SAME TRANSACTION as the insert, so a rollback takes the mark
-- with it: a hole in the sequence would read as a lost entry to the cloud, and
-- a reused one as a duplicate.
CREATE TABLE stock_ledger_sequence (
    outlet_id   TEXT PRIMARY KEY REFERENCES outlet(id),
    -- Monotonic, never reset. Unlike invoice_sequence there is no period
    -- bucket: an invoice number is a human-facing document reference that
    -- resets by fiscal policy, while this is an internal ordering mark whose
    -- only job is to be unique and increasing forever.
    last_value  INTEGER NOT NULL DEFAULT 0 CHECK (last_value >= 0),
    updated_at  TEXT NOT NULL
);

-- ============================================================================
-- PART 2 — A MAGNITUDE BOUND, SO OVERFLOW IS NOT A RUNTIME CONDITION
-- ============================================================================
--
-- T2's modifier path skipped silently on an i64 overflow of
-- `quantity_micro × line_quantity`. The right fix is not a gap reason for it:
-- it is to make the state unreachable. **Nine quintillion micrograms is bad
-- data, not a runtime condition.**
--
-- The bound is 1e15 micro-units — a thousand tonnes, or a thousand kilolitres,
-- of a single ingredient in one row. Absurd for a restaurant by nine orders of
-- magnitude, and it leaves room for the arithmetic: 1e15 × a four-digit line
-- quantity stays inside i64's 9.2e18, and every stored value stays inside
-- JavaScript's 2^53, which is the tighter of the two limits (see
-- src/types/inventory.ts).
--
-- SQLite cannot ADD CONSTRAINT to an existing table, and rebuilding four tables
-- to add a bound would be a far larger change than the bound is worth. Triggers
-- are equivalent here and are already this file's idiom. PostgreSQL gets real
-- CHECK constraints in its mirror, because it can.
CREATE TRIGGER stock_ledger_entry_quantity_is_bounded
BEFORE INSERT ON stock_ledger_entry
WHEN ABS(NEW.quantity_applied_micro) > 1000000000000000
BEGIN
    SELECT RAISE(ABORT,
        'stock_ledger_entry.quantity_applied_micro exceeds 1e15 micro-units (a thousand tonnes of one ingredient in one row). That is bad data, not a runtime condition -- the bound exists so arithmetic overflow is unreachable rather than handled (ADR-018, contracts 0.5.3)');
END;

CREATE TRIGGER recipe_ingredient_quantity_is_bounded
BEFORE INSERT ON recipe_ingredient
WHEN NEW.quantity_micro > 1000000000000000
BEGIN
    SELECT RAISE(ABORT,
        'recipe_ingredient.quantity_micro exceeds 1e15 micro-units. A recipe does not call for a thousand tonnes of anything (ADR-018, contracts 0.5.3)');
END;

CREATE TRIGGER modifier_ingredient_delta_quantity_is_bounded
BEFORE INSERT ON modifier_ingredient_delta
WHEN ABS(NEW.quantity_micro) > 1000000000000000
BEGIN
    SELECT RAISE(ABORT,
        'modifier_ingredient_delta.quantity_micro exceeds 1e15 micro-units (ADR-018, contracts 0.5.3)');
END;

CREATE TRIGGER stock_count_line_quantity_is_bounded
BEFORE INSERT ON stock_count_line
WHEN ABS(NEW.counted_quantity_micro) > 1000000000000000
   OR ABS(NEW.expected_quantity_micro) > 1000000000000000
BEGIN
    SELECT RAISE(ABORT,
        'stock_count_line quantity exceeds 1e15 micro-units (ADR-018, contracts 0.5.3)');
END;
