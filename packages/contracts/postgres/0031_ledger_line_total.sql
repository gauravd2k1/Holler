-- contracts 0.6.3 (ADR-021) — stock_ledger_entry.line_total_paise
--
-- The PostgreSQL mirror of sqlite/0030. See that file for the full reasoning;
-- the short version is that `unit_cost_paise` is a per-BASE-UNIT rate rounded
-- to whole paise once per receipt, and a weighted average summed from rates
-- inherits a rounding it can never recover. Storing the invoiced total lets the
-- average divide exactly once, at the end.
--
-- Additive here, with no table rebuild: PostgreSQL can ADD COLUMN and ADD
-- CONSTRAINT in place, so unlike the SQLite side there are no triggers or
-- indexes to carry back.

ALTER TABLE stock_ledger_entry ADD COLUMN line_total_paise BIGINT;

COMMENT ON COLUMN stock_ledger_entry.line_total_paise IS
    'The exact money this row is worth, unrounded, as invoiced. Set by receipts only, because only a receipt has an invoiced total; wastage, count adjustments, variance and outbound movements are valued AT the average and leave this NULL. The averaging input (procurement/cost.rs) — unit_cost_paise is a derived display rate and is no longer summed.';

-- THE BACKFILL RECONSTRUCTS, IT DOES NOT RECOVER. Pre-0.6.3 rows never stored
-- the invoiced total, so the only figure available is quantity x rate, and the
-- rate is exactly the number this migration exists because it was rounded.
-- These values are AS ACCURATE AS THE OLD PATH AND NO MORE — not recovered
-- truth. The constraint below asserts the two columns pair, never that the
-- total is money anyone was billed.
--
-- ROUNDING IS HALF AWAY FROM ZERO, matching every other rounding on this path
-- (ADR-018 §5). PostgreSQL integer division truncates TOWARD ZERO, which would
-- round a positive row down and a NEGATIVE row up — a silent asymmetry across
-- the sign, on a table whose outbound rows are all negative. Half a unit is
-- added in the direction of the row's own sign before truncating, so both signs
-- round away from zero. The expression is written to match sqlite/0030's
-- character for character in behaviour, so the two stores cannot drift.
-- `trunc()` on a numeric truncates TOWARD ZERO, which is what makes the
-- sign-directed half-unit produce half-away-from-zero. Plain `/` on numerics is
-- fractional division, and casting that to bigint would round a SECOND time —
-- the exact defect this migration exists to remove, reintroduced in its own
-- backfill. The intermediate is numeric, not bigint, because the product can
-- exceed the bigint range: quantity is bounded at 1e15 micro-units, so the
-- product overflows once the rate passes ~9223 paise per base unit.
UPDATE stock_ledger_entry
   SET line_total_paise = CASE
        WHEN quantity_applied_micro::numeric * unit_cost_paise >= 0
            THEN trunc((quantity_applied_micro::numeric * unit_cost_paise + 500000) / 1000000)
        ELSE trunc((quantity_applied_micro::numeric * unit_cost_paise - 500000) / 1000000)
       END::bigint
 WHERE unit_cost_paise IS NOT NULL;

-- DIRECTIONAL, not a strict pairing. A total never appears without its rate; a
-- rate may stand alone. A strict pairing would reject every wastage, count and
-- variance row — the ones valued at the average with no invoice behind them —
-- which is the majority of the ledger.
ALTER TABLE stock_ledger_entry
    ADD CONSTRAINT stock_ledger_cost_pairs_with_total
    CHECK (line_total_paise IS NULL OR unit_cost_paise IS NOT NULL);
