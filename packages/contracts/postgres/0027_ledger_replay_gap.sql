-- Holler Cloud PostgreSQL — ledger_replay_gap. Contracts 0.5.8, ADR-018
-- replay addendum.
--
-- CLOUD-ONLY on purpose: no SQLite mirror, no AggregateType, no sync
-- direction (the `refresh_token`/`device_credential` precedent). Declared in
-- SINGLE_STORE_MIGRATIONS. This table records what the CLOUD observed about a
-- stream it received; the edge cannot author it, and an edge that could would
-- be reporting on its own losses.
--
-- ============================================================================
-- WHY THE HOLE IS A ROW AND NOT A REJECTION
-- ============================================================================
--
-- Contiguity detection exists to make a lost stream row VISIBLE. Enforcing it
-- by rejecting the arriving row makes it invisible instead: replay halts at
-- the hole, every later entry stays at the outlet, and the outage is silent
-- because nothing downstream can tell "nothing happened today" from "replay
-- has been wedged since Tuesday". One bad row must never become an outage.
--
-- So the arriving entry is ACCEPTED and the hole is RECORDED. Detection is
-- the goal; blocking was a side effect nobody wanted.
CREATE TABLE ledger_replay_gap (
    id                  UUID PRIMARY KEY,
    outlet_id           UUID NOT NULL REFERENCES outlet(id),

    -- Which of the two ranged streams this hole is in. Both range over their
    -- own independent 1-based counter, so a bare (from, to) pair is ambiguous
    -- across them -- ledger 41..43 and gap 41..43 are different facts.
    stream              TEXT NOT NULL CHECK (stream IN ('LEDGER','DEDUCTION_GAP')),

    -- The missing span, inclusive: everything from the cloud's high-water
    -- mark + 1 up to the entry_seq that arrived, exclusive of that entry.
    from_entry_seq      BIGINT NOT NULL CHECK (from_entry_seq >= 1),
    to_entry_seq        BIGINT NOT NULL CHECK (to_entry_seq >= from_entry_seq),

    first_observed_at   TIMESTAMPTZ NOT NULL,
    -- Re-observation is expected and is not new information. The edge retries
    -- a batch, the same hole is seen again; without the UNIQUE key below and
    -- this column, one hole becomes N rows and the table degrades into the
    -- log-line outcome it exists to avoid -- unreadable, so unread.
    last_observed_at    TIMESTAMPTZ NOT NULL,
    observation_count   INTEGER NOT NULL DEFAULT 1 CHECK (observation_count >= 1),

    -- A HOLE THAT LATER FILLS IS NOT A LOSS. Late arrival is ordinary: a
    -- batch reordered, a retry that lands after its successor, an outlet
    -- resuming mid-stream. Set when every seq in the span has since been
    -- ingested. A row left claiming a permanent loss that has since healed is
    -- a false alarm, and a table of false alarms is one nobody reads.
    resolved_at         TIMESTAMPTZ,

    UNIQUE (outlet_id, stream, from_entry_seq, to_entry_seq)
);

-- The read that matters: what is still missing, per outlet.
CREATE INDEX idx_ledger_replay_gap_unresolved
    ON ledger_replay_gap(outlet_id, stream)
    WHERE resolved_at IS NULL;
