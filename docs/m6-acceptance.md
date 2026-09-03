# M6 acceptance evidence

**The record, not the chat.** A milestone does not close until its acceptance
evidence is committed to the repository: M5's criteria 1, 3, 4 and 6 were all
observed on real screens and then reported as unobserved by the next session,
which was holding the commit made *because* of the run that observed them. Every
row below names what was observed, how the precondition was established, who
observed it, and on what date — or says plainly that nobody has observed it yet.

**"C1".."C8" here mean M6's criteria** (`docs/m6-planning.md`), never M5's. M5's
seven are closed in `docs/m5-acceptance.md`; its criterion 7 (weighted average
cost) is a different thing entirely from **M6 C7**.

**Two classes of evidence, never merged.** *Executed* means a command was run and
its output read. *Observed* means a person watched the shipping binaries do it.
**An acceptance criterion is closed only by Observed**: a test harness is not an
acceptance run (`docs/retro.md`, 2026-08-11), and the falsifier must have been
watched failing first (§66).

---

## Status summary

| # | Criterion | State |
|---|---|---|
| C1 | Aggregator order bills and closes with the cloud unreachable | NOT STARTED (Phase C) |
| C2 | Stock-out snoozes on ONDC staging | **PARKED** behind platform sandbox access |
| C3 | A permanently-rejected row blocks itself and not its neighbours | **CODE COMPLETE, AWAITING OBSERVATION** — see below |
| C4 | An offline order reaches the cloud without the operator closing the app | NOT STARTED (A5) |
| C5 | Supplier and pack size created in admin convert on the next receipt | NOT STARTED (Phase B) |
| C6 | A goods receipt is readable back in-product | NOT STARTED (Phase B) |
| C7 | A client-data failure is reported as 4xx with a reason the edge records | **CODE COMPLETE, AWAITING OBSERVATION** — see below |
| C8 | An aggregator order flows through both adapters | NOT STARTED (Phase C) |

---

## M6 C7 — a client-data failure is reported as 4xx with a reason the edge records

**State: CODE COMPLETE, NOT CLOSED.** Every mechanism the criterion names is
built, falsified and green. **Nobody has watched it happen on the shipping
binaries**, so by this project's own rule the criterion stays open.

### The falsifying condition, watched first

> Replay an FK-violating row on the **pre-fix** binary → 500, budget uncharged;
> after → 4xx, reason stored, row surfaced.

**The pre-fix half is Executed and recorded.** On 2026-09-03, against Docker
Postgres, a replayed `order_item` referencing a `menu_item` the cloud does not
hold produced, from the real router and the real repository:

```
2026/09/03 10:05:30 ERROR httpx: unhandled error error="ordering: appending item:
ERROR: insert or update on table \"order_item\" violates foreign key constraint
\"order_item_menu_item_id_fkey\" (SQLSTATE 23503)"
    status = 500 (internal_error), want a 4xx
    code = "internal_error", want "missing_reference"
```

### What is Executed

| Half of the criterion | Evidence | Commit |
|---|---|---|
| **4xx on the wire** | `TestIngest_AppendItem_MissingMenuItemIsClientErrorNotServerError` — red at 500 `internal_error`, green at 422 `missing_reference`. Body carries no SQL, no SQLSTATE, no constraint name | `99875cc` |
| **The same fault on two more ingest routes** | KOT for an unknown order was **404** (worse than 500: the edge treats 404 as transient, so it retried forever and took a global stop with it) → 422. Payment for an unknown order 500 → 422, with a companion test proving a good tender still succeeds | `856616b`, `3f7abaa` |
| **Budget charged** | `a_permanently_refused_row_spends_its_budget_then_becomes_visible` — five attempts, then `blocked_at` set. Falsified by disabling the give-up branch: `the budget is spent: left: 0, right: 1` | `078d4e5` |
| **Transient failures never charged** | `a_transient_failure_counts_forever_and_blocks_never` — counted and surfaced, never blocked. Falsified by spending the budget on transient failures | `078d4e5` |
| **Reason stored** | `sync_outbox_block.last_code` carries the machine-readable code (`missing_reference`), durable across a restart. Asserted field-by-field, not merely non-empty | `c95dc24`, `078d4e5` |
| **Row surfaced** | `list_blocked_outbox_rows` / `list_persistently_failing_outbox_rows`, their TS clients, and `SyncBlockedBanner` on `PosScreen` and `OrderListScreen` | `c8147ef` |
| **Neighbours not stranded (C3)** | `a_refused_row_blocks_its_own_aggregate_and_not_its_neighbours` — falsified twice: report fields with no skip logic, then the classifier call replaced with `if false`, both giving `left: [] right: ["outbox-2"]` | `07d7968` |

Suites executed through `scripts/assert-tests-ran.mjs`, so a run that executed
nothing is a failure rather than a pass: backend `go test ./...` — 15 packages
with tests; `cargo test -p holler-edge-sync` — 62 tests; `pnpm test` (POS) — 230
tests. All green.

### What is NOT observed, and why that matters here

**The banner has never been rendered.** It is typechecked (`tsc --noEmit`), built
(`pnpm build`) and its data path is unit-tested — and this repository has twice
recorded that **build-green is not dev-works for a Tauri frontend**: the KDS
detached-global crash was browser-only, and the POS white screen was
dev-server-only. Neither was visible to any green suite. A banner that renders
blank, or renders behind another fixed overlay, would satisfy every check above
and fail the criterion.

**The end-to-end path has never run in one process.** Each half is proven against
its own harness: the cloud returns 422 in a Go test, the edge records and blocks
in a Rust test against a `tiny_http` stand-in. **No single run has taken a real
order from the till, failed it against the real backend, and shown the operator
the result** — and `docs/backlog.md` still carries "`edge/sync` has no host", so
the worker is only reachable from a test process at all.

### What closing it requires

1. Backend up in its own window via `scripts/dev-up.ps1`, **verified by a NEW
   pid** — not by the port answering (`docs/retro.md`; an old process answers
   identically).
2. A till order whose `menu_item_id` the cloud does not hold — the seeded 2-row
   cloud menu against the edge's 43 makes this the default, not a contrivance.
   **The menu seed drift must stay untouched until then**: seeding the cloud
   makes the 500 disappear and ships both defects looking like a fix.
3. Watch the drain: the order is refused 422, its neighbours still publish, the
   budget spends over five pumps, and the row lands in `sync_outbox_block`.
4. **Read the banner off the screen** and record what it says, with the
   `aggregate_id` and the code it displays.
5. Restart the POS and confirm the banner still says it — that is the half
   "a reason the edge **records**" actually asserts.

---

## M6 C3 — a permanently-rejected row blocks itself and not its neighbours

**State: CODE COMPLETE, NOT CLOSED**, for the same reason and by the same route
as C7. The mechanism, its falsifications and its commits are in the C7 table
above; the observation is step 3 of the same run.

The falsifier this criterion names — *the same fixture on the pre-fix binary
strands the neighbours, neighbour counts recorded both times* — is Executed as a
harness result (`left: [] right: ["outbox-2"]`) and **not** as an observed
outlet run.

---

*Last updated 2026-09-03, during M6 Phase A.*
