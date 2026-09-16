# ADR-028 — Order replay is at-least-once and unordered by version, and the line-amendment rule has one owner

**Status:** ACCEPTED
**Date:** 2026-09-16
**Contracts:** v0.8.2 → **v0.8.3** (one new exported constant per wire language; two observable HTTP behaviours changed; no schema, no store, no column)
**Amends:** ADR-009 / §50.1 (edge authority for order transactions), ADR-011 0.2.1 (envelope-wrapped ingest), ADR-014 (`#132-A` line changes after send)

## Context

On 2026-09-16 the running till showed `5 records will not reach the cloud`,
every line `5 attempts · conflict (HTTP 409)`. The cloud's own table showed the
other half:

```
 display_number | status | version | items
 #A1            | DRAFT  |       2 |     5
 #A2            | DRAFT  |       8 |     7
 #A3            | DRAFT  |       6 |     0
```

Three orders the outlet had sent to the kitchen were sitting in the cloud as
DRAFT, one with no lines at all.

**Both defects have one cause: the cloud's command rules assumed exactly-once,
ordered delivery, and the edge's outbox is at-least-once.** A replayed row may
arrive more than once, may arrive long after the aggregate has moved on, and
carries no per-event sequence. Every rule the cloud wrote on the opposite
assumption failed, and failed *permanently*, because a 409 is not retried and
per-aggregate ordering (M6 A2) holds every later row of that order behind it.

### Defect A — the envelope's version cannot sequence anything

`SyncEnvelope.version` is filled from the **live aggregate row at send time**
(`load_aggregate_envelope_fields`, `edge/sync/src/worker.rs`), not stamped when
the event was recorded. Every event still queued for one order therefore ships
the same number: whatever that order has reached by then. There is no
per-event version to send instead — `local_outbox` has no version column and
the frozen `OutboxEvent` envelope has no field for one.

The cloud required a transition's version to be exactly `current + 1`. That
failed in both directions:

- **Synced online, then more lines added.** The transition arrived several
  versions ahead → 409 → permanent → the order's whole queue wedged. This is
  the five blocked rows and the missing lines.
- **Taken entirely offline.** Create and transitions all ship the same version
  → read as an already-applied replay → 200 returned, **nothing applied** →
  the order sits DRAFT in the cloud forever with no error, no blocked row and
  no banner. This is the demo's offline step exactly.

### Defect B — the cloud kept its own line-amendment rule, and defect A hid it

The edge has allowed a line to be added through `DRAFT`, `CONFIRMED`,
`SENT_TO_KITCHEN` and `PREPARING` since `#132-A` — a second Send on the same
table is ordinary restaurant work. The cloud allowed `DRAFT` only, and
`openapi.yaml` described the route the same way.

**Nothing went red for months.** Defect A left every cloud-side order in DRAFT,
so the cloud's stricter rule was never reached. Fixing A is what would have
exposed B — in front of a client, as a wedged queue in the sync banner on the
second Send of the demo's first table.

## Decision

### 1. The state machine is the whole guard for a transition replay

`docs/spec/sync.md` already specifies the order aggregate's conflict policy as
*"state machine + command validation"*, and `openapi.yaml` already documented
the transition routes' 409 as an illegal transition — never as a version gap.
The version check is removed:

- `current.Status == to` is the **idempotency** test. It is the one thing a
  redelivery cannot change.
- `validTransition(current.Status, to)` is the **legality** test.
- The stored version still only moves **forward** (`replayVersion`: take the
  edge's number when it is ahead, otherwise step by one), so anything reading
  it as monotonic is unaffected.

**Two observable behaviours change, and they are the point, not a side effect.**
Confirming an order already `CONFIRMED`, and cancelling one already
`CANCELLED`, now return **200 with the stored row unchanged** instead of 409.
Under at-least-once delivery with no per-event version, a redelivery whose
acknowledgement was lost is indistinguishable from a fresh command — and
refusing it wedges the queue permanently. `confirmed_at` is never shifted by a
redelivery. **A terminal status that is not the one being replayed is still
refused**: cancelling a `CLOSED` order remains an illegal transition.

### 2. The amendable set is declared once in contracts and consumed everywhere

`ORDER_ITEM_AMENDABLE_STATUSES = [DRAFT, CONFIRMED, SENT_TO_KITCHEN,
PREPARING]` — `packages/contracts/src/types/order.ts`, mirrored as
`OrderItemAmendableStatuses` in `go/order.go`, mirrored again as a Rust `const`
in `edge/database/src/repo.rs` (that crate cannot import either), and written
into `openapi.yaml` in a machine-readable form.

**Per §50.1 this is the EDGE's rule written down, not a negotiated middle.**
The edge is the authority for order transactions; the cloud replays what the
outlet did and does not get an amendable set of its own. The cloud's rule is
widened to this set and **reads it** — `contracts.IsOrderItemAmendable` — rather
than restating it.

`scripts/check-order-amendable-drift.mjs` fails the build when any of the four
surfaces disagrees, including on ORDER, and when the cloud stops calling the
contracts helper. It is wired into `ci.yml` and was watched RED against each
surface in turn and against a cloud that restates the rule.

## Consequences

- **A wrong-but-legal replay is now accepted where it used to be refused.** That
  is deliberate. Halting sync is survivable only when a human is told; halting
  it on a row that was correct at the outlet is data stranded for no reason, and
  the outlet is the authority. Genuinely illegal moves — a cancel after CLOSED,
  a line added to a BILLED order — are still 409.
- **The version column on `order` is now a mirror, not a lock.** It was never
  reliable as a lock. Nothing else in the cloud reads it for concurrency; the
  repository's compare-and-set on `version` still guards a concurrent replay
  race within the cloud, which is a different thing and is kept.
- **The four-surface set is a maintenance cost paid deliberately.** A single
  shared declaration is impossible across TypeScript, Go and a Rust crate that
  imports neither, so the check is the joint. Anyone widening the set edits four
  files in one commit or the build fails.
- **openapi.yaml `info.version` is still 0.6.2** and was not touched here. It
  has been stale since well before this change; correcting it is its own commit.

## Falsifiers

- `TestTransition_EnvelopeVersionNeverBlocksOrSwallowsAReplay` reproduces both
  shapes of defect A and was watched fail against a hand-reconstructed pre-fix
  service — the first subtest with `ErrConflict`, the second by leaving the
  order DRAFT while returning no error.
- `TestOrderLifecycle_ReplayedThroughTheSyncPath` drives a complete edge-legal
  lifecycle through the ingest handlers the way the edge actually sends it —
  same-version envelopes, a line appended after the order reached the kitchen,
  and one redelivered row — and asserts the cloud ends in the same state as the
  edge. **It fails against the pre-fix service on both defects.** A
  single-transition test cannot see either, which is the retro finding this ADR
  leaves behind: *sync tests must drive a complete order lifecycle, not one
  transition.*
