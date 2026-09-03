-- 0031 — sync_outbox_block: the general outbox's block-and-budget record.
-- Contracts 0.6.4, ADR-023 (M6 A3).
--
-- EDGE-LOCAL. SQLite only, no PostgreSQL mirror, never an AggregateType, no
-- sync direction — the invoice_sequence / stock_balance_snapshot /
-- sync_replay_block precedent. A record of what THIS outlet failed to send is
-- the outlet's own; a cloud copy would be a second authority on the edge's
-- progress, and the cloud already derives what it did or did not receive from
-- what it actually stored.
--
-- WHY A SIBLING TABLE AND NOT sync_replay_block. The ranged table is keyed
-- (outlet_id, stream, entry_seq) where entry_seq is a mark from a per-stream
-- counter, NOT NULL, CHECK >= 1. A general-outbox row has no such mark: it is
-- identified by local_outbox.id, a TEXT ULID. Fitting it into the existing
-- table would need entry_seq nullable (forbidden — it is in the key), or a
-- synthetic mark indistinguishable from a real one, or record_id meaning the
-- identity for one stream and a description for the others. That is two
-- shapes in one table. Same shape, different key, so it is a different table.
--
-- WHY NOT PROCUREMENT'S APPROACH. There is now a third mechanism for one
-- concept, and that is drift; ADR-023 records the reasoning and
-- docs/backlog.md carries the convergence with a trigger. In short:
-- procurement keeps its budget in local_outbox.attempt_count and reports
-- blocked entries in memory (BlockedProcurementEntry), which is durable
-- enough to STOP retrying but not durable enough to SHOW a human what was
-- abandoned and why — the count survives, the reason does not. M6 C7 needs
-- the reason to be readable after a restart, so the general outbox gets a
-- durable row.

CREATE TABLE sync_outbox_block (
    outlet_id        TEXT NOT NULL REFERENCES outlet(id),

    -- The outbox row that could not be sent. This is the identity; there is
    -- no ordinal to key on, which is the whole reason this table exists
    -- separately from sync_replay_block.
    outbox_id        TEXT NOT NULL REFERENCES local_outbox(id),

    -- What the row was ABOUT, denormalised on purpose. A2 blocks per
    -- aggregate, so this is what an operator must be shown, and a human
    -- chasing an abandoned row must not depend on the outbox row still being
    -- joinable to explain it.
    aggregate_type   TEXT NOT NULL,
    aggregate_id     TEXT NOT NULL,

    attempts         INTEGER NOT NULL DEFAULT 1 CHECK (attempts >= 1),

    -- The cloud's last word: an HTTP status, or NULL with a local reason when
    -- the row never reached the wire.
    last_status      INTEGER,

    -- The MACHINE-READABLE code from the cloud's error envelope
    -- (httpx.ErrorBody.code — "missing_reference" and the rest). Nullable for
    -- the same reason as last_status: a row that failed locally has no code.
    --
    -- This is the column M6 C7's "a reason the edge records" is closed on.
    -- last_error below is prose and is for a human; closing a criterion on
    -- prose would mean the stored reason changes whenever someone edits a
    -- message string. The enum these values come from is not yet in the
    -- frozen contract — it lands at 0.7.0 with the rest of the error codes —
    -- so this column is deliberately TEXT and deliberately unconstrained
    -- until then.
    last_code        TEXT,

    -- Human-facing prose. Never the only record of why.
    last_error       TEXT NOT NULL,

    first_attempt_at TEXT NOT NULL,
    last_attempt_at  TEXT NOT NULL,

    -- NULL while the row is still being retried within its budget. Set when
    -- the budget is spent — the moment this becomes something to show a
    -- human, and the moment the drain stops retrying it.
    blocked_at       TEXT,

    PRIMARY KEY (outlet_id, outbox_id)
);

-- The outstanding-work query: everything this outlet has given up on.
CREATE INDEX idx_sync_outbox_block_outstanding
    ON sync_outbox_block(outlet_id, blocked_at);
