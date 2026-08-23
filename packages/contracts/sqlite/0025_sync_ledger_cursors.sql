-- Holler Edge SQLite — ranged replay cursors. Contracts 0.5.8, ADR-018
-- replay addendum.
--
-- SINGLE-STORE ON PURPOSE. Everything in THIS file is edge-local: SQLite
-- only, no PostgreSQL mirror, no AggregateType, no sync direction, ever.
-- Declared in SINGLE_STORE_MIGRATIONS (edge/database/src/migrations.rs) with
-- that reason. `stock_deduction_gap.entry_seq` is deliberately NOT here --
-- that column goes in BOTH stores, so it ships as 0026 in each, where the
-- asymmetry guard can see a missing mirror and fail. Folding the two changes
-- into one declared-single-store file would have silently exempted the gap
-- column from ever needing its PostgreSQL twin.
--
-- ============================================================================
-- PART 1 — TWO CURSORS, NEVER ONE
-- ============================================================================
--
-- Ranged sync replaces the outbox for the two high-volume stream tables
-- (ADR-018's transport rule): send `entry_seq > cursor` in order, advance the
-- cursor on ack. `stock_ledger_entry` and `stock_deduction_gap` are two
-- INDEPENDENT streams over two INDEPENDENT counters, so they get two
-- independent cursors. A shared cursor would make either stream's progress
-- silently skip the other's unsent rows -- both counters mint 1, 2, 3... and
-- one mark cannot mean two positions.
--
-- WHY THE CURSOR IS EDGE-LOCAL. It records how far THIS outlet has replayed.
-- The cloud has its own high-water mark derived from what it actually stored;
-- mirroring the edge's cursor would make the cloud a second authority on the
-- edge's replay progress, the mistake mirroring `invoice_sequence` would make
-- about invoice numbers (§33).
--
-- 1-BASED, AND THAT IS LOAD-BEARING. `next_stock_ledger_sequence_value`
-- issues 1 to the first entry (repo.rs: `VALUES (?1, 1, ?2)` on first insert,
-- `last_value + 1` thereafter). DEFAULT 0 therefore means "nothing acked" and
-- `entry_seq > 0` selects the whole stream. A 0-based sequence would make the
-- first entry unselectable forever -- silently, once per outlet, at the only
-- moment nobody is watching.
--
-- Safe as a plain ADD COLUMN: `sync_state` is keyed on `outlet_id` alone and
-- carries no uniqueness over these columns, so the constant default collides
-- with nothing (contrast 0026, where DEFAULT 0 under a UNIQUE key is exactly
-- the trap that forces a rebuild).
ALTER TABLE sync_state
    ADD COLUMN last_acked_ledger_entry_seq INTEGER NOT NULL DEFAULT 0;

ALTER TABLE sync_state
    ADD COLUMN last_acked_gap_entry_seq INTEGER NOT NULL DEFAULT 0;

-- ============================================================================
-- PART 2 — THE GAP STREAM'S OWN DURABLE COUNTER
-- ============================================================================
--
-- The `stock_ledger_sequence` shape exactly (0021), for the other stream, and
-- for the same reason: a counter derived as MAX(entry_seq) + 1 restarts after
-- ADR-018 §9 archival removes rows, and a reused mark makes the cloud's
-- contiguity check produce both false positives and false negatives.
--
-- SEPARATE from stock_ledger_sequence, not a second row in it. Two streams
-- that advance at wildly different rates -- 15,000 ledger rows a day against
-- a handful of gaps -- share nothing but their shape. One counter across both
-- would put permanent holes in each stream's sequence, and a hole is exactly
-- what the cloud reads as a lost row.
--
-- Taken in the SAME TRANSACTION as the gap insert, so a rollback takes the
-- mark with it.
CREATE TABLE stock_deduction_gap_sequence (
    outlet_id   TEXT PRIMARY KEY REFERENCES outlet(id),
    last_value  INTEGER NOT NULL DEFAULT 0 CHECK (last_value >= 0),
    updated_at  TEXT NOT NULL
);

-- ============================================================================
-- PART 3 — A BOUND ON RETRYING ONE ENTRY, SO THE STREAM CANNOT WEDGE
-- ============================================================================
--
-- THE SAME OUTAGE AS AN UNBOUNDED CONTIGUITY REJECT, FROM THE OTHER END. The
-- cloud accepts a hole and records it rather than halting replay. The mirror
-- image is the edge: if the cloud permanently rejects entry 7 -- malformed, a
-- validation failure, anything not transient -- and the edge retries 7
-- forever, then 8..N never leave the outlet. One bad row becomes an outage
-- again, just authored at the other end.
--
-- So the retry is BOUNDED PER ENTRY, not per stream. After N attempts the
-- entry is recorded here, the cursor moves past it, and the rest of the
-- stream replays. The skipped mark then arrives at the cloud as a hole in
-- `ledger_replay_gap`, so the same fact is visible from both ends and neither
-- end is quietly holding it alone.
--
-- HALTING SYNC IS SURVIVABLE -- no core outlet path depends on the uplink
-- (ADR-013). Halting it SILENTLY is not, and that is why this is a table
-- rather than a log line: `blocked_at IS NOT NULL` is a row a human can be
-- shown.
--
-- EDGE-LOCAL, like everything else in this file: it records what THIS outlet
-- could not send.
CREATE TABLE sync_replay_block (
    outlet_id       TEXT NOT NULL REFERENCES outlet(id),

    -- Which ranged stream. Both count from 1 over their own counter, so an
    -- entry_seq alone does not identify a row.
    stream          TEXT NOT NULL CHECK (stream IN ('LEDGER','DEDUCTION_GAP')),
    entry_seq       INTEGER NOT NULL CHECK (entry_seq >= 1),

    -- The row that could not be sent, so a human chasing this has something
    -- to look up rather than an ordinal.
    record_id       TEXT NOT NULL,

    attempts        INTEGER NOT NULL DEFAULT 1 CHECK (attempts >= 1),
    -- The cloud's last word on it: an HTTP status, or NULL with a local
    -- reason when the row never got as far as the wire.
    last_status     INTEGER,
    last_error      TEXT NOT NULL,

    first_attempt_at TEXT NOT NULL,
    last_attempt_at  TEXT NOT NULL,

    -- NULL while the entry is still being retried within its budget. Set when
    -- the budget is spent and the cursor moves past it -- the moment this
    -- becomes something to show a human.
    blocked_at      TEXT,

    PRIMARY KEY (outlet_id, stream, entry_seq)
);

CREATE INDEX idx_sync_replay_block_outstanding
    ON sync_replay_block(outlet_id, blocked_at);
