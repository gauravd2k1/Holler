-- Holler Cloud PostgreSQL — repair stock_deduction_gap's reason CHECK.
-- Contracts 0.5.6. Mirror of sqlite/0024_fix_gap_reason_check.sql, whose
-- header carries the full reasoning: DIMENSION_MISMATCH reached the TS enum,
-- the Go enum and OpenAPI at 0.5.1 and neither store's CHECK -- so the first
-- genuine mismatch at a live outlet would have failed the confirm, and on this
-- side the ingest route would have 500'd on a valid replayed gap.
ALTER TABLE stock_deduction_gap DROP CONSTRAINT stock_deduction_gap_reason_check;
ALTER TABLE stock_deduction_gap ADD CONSTRAINT stock_deduction_gap_reason_check
    CHECK (reason IN ('NO_RECIPE','NO_VARIANT','CYCLE','DEPTH_EXCEEDED',
                      'UNKNOWN_UNIT','DIMENSION_MISMATCH','UNRESOLVABLE_REFERENCE'));
