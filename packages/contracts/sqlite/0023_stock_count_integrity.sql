-- Holler Edge SQLite — close the completed-count INSERT hole, and give a
-- COUNT_ADJUSTMENT structured provenance. Contracts 0.5.5, ADR-018 addendum.
--
-- ============================================================================
-- PART 1 — A COMPLETED COUNT COULD STILL GROW NEW LINES
-- ============================================================================
--
-- 0016 claimed `stock_count` is "mutable while OPEN, immutable once COMPLETED"
-- and enforced it with `BEFORE UPDATE` and `BEFORE DELETE` triggers on
-- `stock_count_line`. **There was no `BEFORE INSERT` trigger.**
--
-- So editing a line of a completed count was correctly rejected, while
-- INSERTING A BRAND-NEW LINE into that same completed count sailed through
-- with no error. The evidence behind append-only COUNT_ADJUSTMENT ledger rows
-- could be added to after the fact — which is the side door the trigger existed
-- to close, reached from a direction nobody tested.
--
-- Found by T3, which removed its own module-level `status == 'OPEN'` check to
-- see whether the schema trigger was sufficient on its own. It was not. That
-- check is therefore the ONLY guard on the insert path, not the
-- belt-and-braces its comment claimed.
--
-- **The lesson, recorded because it is more general than the bug:** the
-- falsification that landed 0016 tested UPDATE and DELETE — the paths its
-- author had in mind — and passed. A guard falsified along the routes you
-- thought of is a guard tested against your own imagination. Enumerate the
-- verbs the table actually accepts (INSERT, UPDATE, DELETE) and try each.
CREATE TRIGGER stock_count_line_cannot_be_added_once_completed
BEFORE INSERT ON stock_count_line
WHEN (SELECT status FROM stock_count WHERE id = NEW.stock_count_id) = 'COMPLETED'
BEGIN
    SELECT RAISE(ABORT,
        'stock_count_line cannot be inserted into a COMPLETED count: the count is the evidence behind append-only COUNT_ADJUSTMENT entries, so it cannot grow new lines after the fact. Take a new count (ADR-018, contracts 0.5.5)');
END;

-- ============================================================================
-- PART 2 — A COUNT_ADJUSTMENT SHOULD NOT BE LINKED BY A STRING
-- ============================================================================
--
-- T3 had to link a COUNT_ADJUSTMENT ledger row back to the count that produced
-- it through `note` — the string `"stock_count:{id}"` — because 0016 gave the
-- ledger no column for it. It flagged that as pragmatic rather than fixing it,
-- correctly, since the schema was not its to change.
--
-- Provenance carried in a free-text field is provenance nothing can check. A
-- typo, a reformat, or a well-meant edit to a human-readable note silently
-- severs the link between an adjustment and its evidence — and the ledger is
-- append-only, so the severed row is permanent. Every other provenance field
-- on this table (`recipe_id`, `modifier_delta_id`, `source_order_id`) is a
-- typed column with no FK; this one is a string.
--
-- Landing it now rather than at 0.6.0 is the stopping rule's second clause: the
-- interim writes rows whose only link is a string, and rewriting them later
-- means rewriting append-only history.
--
-- No FK, like the rest of the provenance group: the ledger stays readable when
-- the count is archived, and a deleted count orphans nothing.
ALTER TABLE stock_ledger_entry ADD COLUMN source_stock_count_id TEXT;

CREATE INDEX idx_stock_ledger_entry_source_count
    ON stock_ledger_entry(source_stock_count_id);
