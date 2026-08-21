-- Holler Cloud PostgreSQL — close the completed-count INSERT hole, and give a
-- COUNT_ADJUSTMENT structured provenance. Contracts 0.5.5, ADR-018 addendum.
-- Mirror of sqlite/0023_stock_count_integrity.sql, whose header carries the
-- full reasoning.
--
-- Summary:
--   * 0016 claimed a completed count is immutable and enforced it with BEFORE
--     UPDATE OR DELETE. There was no INSERT arm, so a brand-new line could be
--     added to a COMPLETED count with no error -- the evidence behind
--     append-only COUNT_ADJUSTMENT rows could grow after the fact. Found by T3
--     removing its own module check to see whether the schema trigger stood
--     alone. It did not.
--   * The lesson is more general than the bug: 0016's falsification tested the
--     verbs its author had in mind and passed. Enumerate the verbs the table
--     accepts and try each.
--   * COUNT_ADJUSTMENT rows linked back to their count through a `note` string,
--     because the ledger had no column. Provenance in free text is provenance
--     nothing can check, and the ledger is append-only, so a severed link is
--     permanent.

CREATE OR REPLACE FUNCTION stock_count_line_insert_blocked_once_completed()
RETURNS TRIGGER AS $$
BEGIN
    IF (SELECT status FROM stock_count WHERE id = NEW.stock_count_id) = 'COMPLETED' THEN
        RAISE EXCEPTION 'stock_count_line cannot be inserted into a COMPLETED count: the count is the evidence behind append-only COUNT_ADJUSTMENT entries and cannot grow new lines after the fact. Take a new count (ADR-018, contracts 0.5.5)';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER stock_count_line_cannot_be_added_once_completed
BEFORE INSERT ON stock_count_line
FOR EACH ROW EXECUTE FUNCTION stock_count_line_insert_blocked_once_completed();

-- No FK, like the rest of the provenance group: the ledger stays readable when
-- the count is archived, and a deleted count orphans nothing.
ALTER TABLE stock_ledger_entry ADD COLUMN source_stock_count_id UUID;

CREATE INDEX idx_stock_ledger_entry_source_count
    ON stock_ledger_entry(source_stock_count_id);
