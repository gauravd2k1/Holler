# ADR-023 — The general outbox gets its own durable block record

- **Status: ACCEPTED**, operator-approved 2026-09-03.
- **Date:** 2026-09-03
- **Milestone:** M6 (Phase A, gap A3 — the retry budget and its surfacing)
- **Contracts:** **0.6.4**, `sqlite/0031_sync_outbox_block.sql`. Additive,
  SQLite-only. No PostgreSQL mirror, no `AggregateType`, no sync direction.
- **Supersedes nothing.** Extends the edge-local precedent of
  `invoice_sequence` (ADR-016), `stock_balance_snapshot` (ADR-018) and
  `sync_replay_block` (ADR-018 0.5.8).

## Context

M6 C7 requires that a client-data failure is *"reported as 4xx with a reason
the edge records"*. A1 put the 4xx and the machine-readable code on the wire.
A2 stopped one refused row from stranding its neighbours. Neither of them
**records** anything a human can read after a restart: the general outbox has
no per-entry budget at all, and a blocked aggregate exists only in the
in-memory `PumpReport` and an `eprintln` whose stderr may not even be attached
(gap A6).

The obvious move was to reuse `sync_replay_block`, which already does exactly
this job for the two ranged streams.

## Decision

**A sibling table, `sync_outbox_block`, with the same shape and a different
key.** Not a widened `sync_replay_block`.

`sync_replay_block` is keyed `(outlet_id, stream, entry_seq)`, where
`entry_seq` is a mark from a per-stream counter — `INTEGER NOT NULL CHECK
(entry_seq >= 1)`. A general-outbox row has no such mark. It is identified by
`local_outbox.id`, a TEXT ULID. Reuse would have required one of:

- `entry_seq` nullable — forbidden outright, it is part of the primary key;
- a synthetic `entry_seq` minted for outbox rows — a second meaning for a
  column documented as a counter mark, indistinguishable from a real one;
- `record_id` carrying the identity when `stream = 'OUTBOX'` and a description
  otherwise — one column meaning two things depending on a sibling column.

All three are the same defect: **two shapes in one table, discriminated by a
column**. The operator's standing rule is that reuse holds only if the new
record fits the existing columns as they stand, and it does not.

`sync_outbox_block` therefore carries the same columns — `attempts`,
`last_status`, `last_error`, `first_attempt_at`, `last_attempt_at`,
`blocked_at` with the same "NULL while within budget" meaning — keyed
`(outlet_id, outbox_id)`, plus `aggregate_type`/`aggregate_id` denormalised
because A2 blocks per aggregate and that is what an operator must be shown.

### `last_code`, and why prose is not a reason

The table carries **`last_code TEXT`** beside `last_status`: the
machine-readable `code` from the cloud's error envelope (`missing_reference`
and the rest).

**M6 C7 is closed on this column, not on `last_error`.** `last_error` is prose
for a human. A criterion closed on prose means the stored reason changes
whenever someone edits a message string, and the thing the criterion asserts
would then be true or false depending on a copy edit. The code is the value the
edge already branches on; it is what "a reason the edge records" has to mean.

The enum those values come from is **not yet in the frozen contract** — it
lands at 0.7.0 with the rest of the error codes, filed in `docs/backlog.md` —
so the column is deliberately unconstrained TEXT until then. It is added now
rather than at 0.7.0 because the alternative is shipping A3 with the criterion
resting on a message string.

## Why not procurement's approach

**There are now three block-and-budget mechanisms for one concept**, and that
is drift. Recorded plainly rather than discovered later:

| Stream | Budget | Blocked record |
|---|---|---|
| Ranged (`LEDGER`, `DEDUCTION_GAP`) | `sync_replay_block.attempts` | `sync_replay_block` row, `blocked_at` set |
| Procurement | `local_outbox.attempt_count` | **in memory only** — `BlockedProcurementEntry` in the `PumpReport` |
| General outbox (this ADR) | `sync_outbox_block.attempts` | `sync_outbox_block` row, `blocked_at` set |

Procurement's approach was considered and rejected **for this gap**, on one
ground: it is durable enough to stop retrying and not durable enough to show a
human what was abandoned and why. `attempt_count` survives a restart; the
reason does not. M6 C7 needs the reason readable after a restart, so counting
in `local_outbox` and reporting in memory does not close it.

That is not an argument that procurement is wrong — its blocked entries are a
buyer-facing event a human acts on the same day, and `grn_gap` already carries
the operational half. It is an argument that the two needs differ today and
that nobody has decided whether they should.

**Convergence is filed in `docs/backlog.md` with a trigger, and is explicitly
NOT A3's job.** Converging three mechanisms while fixing the outage one of them
exists to prevent is how a fix acquires a second defect.

## Consequences

- One more edge-local table the cloud must never learn about. The lint in
  `edge/database/src/migrations.rs` requires the asymmetry to be declared with
  a reason, and it is.
- A blocked row becomes readable after a restart, which is what A3's surfacing
  and M6 C7 rest on.
- `last_code` will need a constraint or a lookup when 0.7.0 lands the error-code
  enum. Filed with that item, not left to be noticed.
- Three mechanisms remain three until the convergence item is scheduled. The
  count is recorded here so the next reader does not have to derive it, and so
  a fourth is harder to add by accident.
