-- Holler Cloud PostgreSQL — a magnitude bound on every stored quantity.
-- Contracts 0.5.3, ADR-018 addendum.
--
-- THERE IS NO POSTGRESQL COUNTERPART TO sqlite/0021's stock_ledger_sequence,
-- deliberately. That counter is EDGE-LOCAL — the invoice_sequence precedent —
-- and it is declared in SINGLE_STORE_MIGRATIONS
-- (edge/database/src/migrations.rs) with its reason. Mirroring it would make
-- the cloud a second minter of ordering marks for a stream the edge owns, and
-- the mark is what the cloud's own gap detection relies on being
-- edge-authored. This file therefore mirrors only PART 2 of that migration.
--
-- WHY THE BOUND. T2's modifier deduction path skipped silently on an i64
-- overflow of quantity_micro x line_quantity. The right fix is not a gap reason
-- for the overflow: it is to make the state unreachable. Nine quintillion
-- micrograms is bad data, not a runtime condition, and a branch that can never
-- be taken needs no error code.
--
-- 1e15 micro-units is a thousand tonnes (or kilolitres) of a single ingredient
-- in one row: absurd by nine orders of magnitude, and it leaves headroom for
-- the arithmetic. 1e15 x a four-digit line quantity stays inside int64's
-- 9.2e18, and every stored value stays inside JavaScript's 2^53, which is the
-- tighter limit of the two.
--
-- PostgreSQL gets real CHECK constraints; the SQLite mirror uses triggers only
-- because SQLite cannot ADD CONSTRAINT to an existing table.
ALTER TABLE stock_ledger_entry
    ADD CONSTRAINT stock_ledger_entry_quantity_is_bounded
    CHECK (abs(quantity_applied_micro) <= 1000000000000000);

ALTER TABLE recipe_ingredient
    ADD CONSTRAINT recipe_ingredient_quantity_is_bounded
    CHECK (quantity_micro <= 1000000000000000);

ALTER TABLE modifier_ingredient_delta
    ADD CONSTRAINT modifier_ingredient_delta_quantity_is_bounded
    CHECK (abs(quantity_micro) <= 1000000000000000);

ALTER TABLE stock_count_line
    ADD CONSTRAINT stock_count_line_quantity_is_bounded
    CHECK (abs(counted_quantity_micro) <= 1000000000000000
       AND abs(expected_quantity_micro) <= 1000000000000000);
