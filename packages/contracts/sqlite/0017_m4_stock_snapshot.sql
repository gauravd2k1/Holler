-- Holler Edge SQLite — stock_balance_snapshot. Contracts 0.5.0, ADR-018 §9.
--
-- EDGE-LOCAL. SQLite only. NO PostgreSQL mirror, NO AggregateType, NO sync
-- direction — the invoice_sequence / print_job / refresh_token precedent, and
-- the absence of a postgres/0017 file is the visible marker of it.
--
-- The cloud MAY re-derive its own stock view by summing the ingested ledger.
-- It may NEVER mirror this table. Mirroring a derived local projection would
-- make the cloud a second authority on stock, which is the same mistake
-- mirroring invoice_sequence would have made about invoice numbers (§33).
--
-- ============================================================================
-- WHY THIS TABLE EXISTS: THE LEDGER IS THE FASTEST-GROWING TABLE IN HOLLER
-- ============================================================================
--
-- One entry per ingredient per line per sale. At ~8 lines per order and ~6
-- ingredients per line, a 300-order day is ~15,000 rows — roughly 5M rows a
-- year, on the ADR-013 target: bare Windows 10, 4GB RAM, SPINNING DISK. An
-- invoice table grows at one row per bill. This grows two orders of magnitude
-- faster.
--
-- Current stock = LATEST SEALED SNAPSHOT + ENTRIES SINCE. A stock read is
-- therefore bounded to one business day's entries forever, however old the
-- ledger gets.
--
-- THERE IS NO MATERIALISED CURRENT-STOCK TABLE. An earlier draft proposed one;
-- this makes it redundant and removes an entire class of projection-drift
-- defect. Current stock is a bounded QUERY, not a row somebody must remember
-- to update.
--
-- ============================================================================
-- SEALING NEVER DEPENDS ON AN OPERATOR
-- ============================================================================
--
-- The bounded-read guarantee holds only while days actually get sealed. An
-- outlet that skips day-end close for a month, or a POS that dies at 11pm,
-- would silently degrade every stock read to a full-ledger scan — and the
-- degradation is invisible until the box is slow.
--
-- A GUARANTEE THAT DEPENDS ON A HUMAN PERFORMING A DAILY ACTION IS NOT A
-- GUARANTEE. That is the ADR-013 lesson restated: design intent is not
-- verified fact.
--
-- So sealing is NOT an effect of day-end close:
--   * IDEMPOTENT — sealing an already-sealed day is a no-op, not an error and
--     not a second row.
--   * LAZILY CAUGHT UP — on database open, every unsealed prior business day
--     is sealed, in order, BEFORE the first stock read is served. Day-end
--     close may trigger it; nothing depends on day-end close having happened.
--
-- Its §66 invariant, like every other, is deliberately broken and watched to
-- fail before it is trusted: skip three business days, reopen, assert three
-- snapshots exist and the balance equals a full-ledger sum.
--
-- ============================================================================
-- ARCHIVAL IS STRUCTURAL, NOT TIME-BASED — AND M4 DELETES NOTHING
-- ============================================================================
--
-- A stock_ledger_entry becomes eligible for archival only when BOTH hold:
--   (a) its outbox replay is ACKED by the cloud, and
--   (b) a SEALED snapshot covers its business_date for its item.
--
-- Not "older than 90 days". A row whose replay never landed is the one row
-- that must not disappear, and age tells you nothing about that.
--
-- M4 computes and REPORTS eligibility and deletes nothing. Whether to delete,
-- and at what threshold, is decided later against a measured row count and
-- read latency from the 4GB box — the first opportunity for which is the T0
-- clean-VM run.
CREATE TABLE stock_balance_snapshot (
    outlet_id               TEXT NOT NULL REFERENCES outlet(id),

    -- Snapshotted id, no FK, consistent with the ledger it summarises.
    inventory_item_id       TEXT NOT NULL,

    -- Outlet-local business day per 0013's definition. The reason that
    -- definition had to be settled in this same contract version: sealing a
    -- BALANCE on a UTC boundary would seal mid-service for any outlet trading
    -- past midnight, and every subsequent read derives from it. A mis-sealed
    -- snapshot is not cosmetic.
    business_date           TEXT NOT NULL,

    closing_quantity_micro  INTEGER NOT NULL,   -- signed; may be negative (Rule 1)
    dimension               TEXT NOT NULL CHECK (dimension IN ('MASS','VOLUME','COUNT')),

    -- THE HIGH-WATER MARK, and the reason a stock read is correct rather than
    -- merely fast. The read is:
    --
    --     closing_quantity_micro
    --   + SUM(quantity_applied_micro) FROM stock_ledger_entry
    --     WHERE outlet_id = ? AND inventory_item_id = ?
    --       AND entry_seq > through_entry_seq
    --
    -- NOT "AND business_date > business_date". An entry that arrives after its
    -- day is sealed but carries that day's business_date is absent from the
    -- seal (it did not exist yet) and would be excluded by a date predicate
    -- (too old) — vanishing from the balance permanently and silently, since
    -- a seal is never UPDATEd. A count spanning midnight, cloud-side
    -- re-derivation in replay order, and any back-dated adjustment all produce
    -- exactly that row.
    --
    -- Selecting by "not covered by the mark" makes a late arrival self-heal
    -- into the next read rather than disappear. This replaces the last_entry_id
    -- an earlier draft carried: an id identifies the last row seen but does not
    -- order it against a row that arrives later.
    through_entry_seq       INTEGER NOT NULL,

    sealed_at               TEXT NOT NULL,      -- ISO8601 UTC

    -- Three NOT NULL columns. No nullable column is in a primary key, per the
    -- contract rubric.
    PRIMARY KEY (outlet_id, inventory_item_id, business_date)
);

-- Serves "the latest sealed snapshot for this item", which is the first half
-- of every stock read.
CREATE INDEX idx_stock_balance_snapshot_latest
    ON stock_balance_snapshot(outlet_id, inventory_item_id, business_date DESC);

-- A SNAPSHOT IS SEALED AT BIRTH, SO IT IS IMMUTABLE FROM BIRTH.
--
-- Unlike stock_count, there is no working-document phase here: sealed_at is
-- NOT NULL, so a row cannot exist unsealed and the trigger needs no WHEN
-- clause. Idempotent catch-up seals by INSERTing and treating a primary-key
-- collision as the no-op it is — it never UPDATEs an existing seal.
--
-- The primary key (outlet_id, inventory_item_id, business_date) is what makes
-- "the latest snapshot" unambiguous: a second seal for the same day is
-- physically impossible, so no read can ever derive from two disagreeing
-- balances. These triggers close the other half — nobody edits or removes a
-- seal after the fact, which would silently change every stock read computed
-- from it, retroactively and invisibly.
CREATE TRIGGER stock_balance_snapshot_is_immutable_no_update
BEFORE UPDATE ON stock_balance_snapshot
BEGIN
    SELECT RAISE(ABORT,
        'stock_balance_snapshot is immutable: a sealed day is never re-sealed or edited. Re-sealing is an INSERT whose primary-key collision is a no-op (ADR-018, contracts 0.5.0)');
END;

CREATE TRIGGER stock_balance_snapshot_is_immutable_no_delete
BEFORE DELETE ON stock_balance_snapshot
BEGIN
    SELECT RAISE(ABORT,
        'stock_balance_snapshot is immutable: deleting a seal silently changes every stock read derived from it (ADR-018, contracts 0.5.0)');
END;
