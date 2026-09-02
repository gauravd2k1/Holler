# M6 planning — aggregators, back office, and the seven sync gaps

**Status: APPROVED 2026-09-02. Execution starts in a FRESH SESSION that reads
this file.** The plan was agreed in the session that closed M5; it is committed
here rather than carried in a transcript, because that is the rule M5 ended on —
*a milestone does not close until its acceptance evidence is committed to the
repository, and the chat is not the record*. This file is the other half of that
rule applied at the start instead of the end: **if the plan survives the session
boundary in the repo, the discipline works.**

Entry state, verified against the repository rather than asserted:

| Fact | Verified |
|---|---|
| Contracts | **v0.6.3**, `packages/contracts/package.json` |
| Migrations | sqlite through **0030**, postgres through **0031** |
| ADRs | through **ADR-021** |
| M5 | **CLOSED 2026-09-02**, seven of seven observed — `docs/m6-planning.md`'s predecessor `docs/m5-acceptance.md` |
| HEAD at approval | `1b51df0`, clean tree, pushed |

**Do not re-run M5 criterion 6.**

---

## What M6 delivers

1. **Aggregator integration** — Swiggy, Zomato, ONDC. Inbound orders, menu and
   availability push, order state round-trip, stock-out snooze.
2. **`apps/admin` back office** — menu and pricing, suppliers and pack sizes,
   purchase orders, staff and permissions, goods-receipt list. The directory
   exists and is **empty**; this is a new application.
3. **The seven sync gaps carried out of M5.** Four are why an outlet cannot yet
   run unattended.

**Excluded absolutely:** reporting depth (M7), multi-outlet and central kitchen
(M8), captain app / CRM / loyalty (M9), batch and expiry alerting,
menu-engineering analytics.

---

## Five corrections the repository forced on the kickoff, all accepted

Recorded because each one changes what gets built, and because a plan that
quietly absorbs its own corrections teaches nothing.

1. **P6's transport half is already landed** (`262e03a`): one retry on a fresh
   connection before anything is classified. **P6 is the reporting half only.**
2. **`stale_connection.rs` is CLOSED and is not a hardware finding** (`1b51df0`).
   Measured: failed on an **idle** machine ~1 run in 30, in **0.00 s**, on
   `os error 10054`. No timeout involved. The test's own fake server answered
   after a single `read`; closing a socket with unread bytes RSTs on Windows and
   **discards the reply already in the send buffer**. Fixed both fake servers;
   0/200 idle, 0/100 under 48 busy loops on 24 cores. **It does not enter P6, and
   the target-hardware note built on the same inference is withdrawn.**
3. **P1 is an EDGE gap, not a cloud one, and is cheaper than the kickoff said.**
   The cloud ingest routes exist: `/invoices`, `/payments`, `/kots/{kotId}/status`,
   `/inventory/counts`, `/cash-shifts`, `/items/{itemId}/availability`. What is
   missing is `edge/sync/src/route.rs`, which maps only `("order", 5 events)` and
   `("table_session", 2)`; everything else returns `UnroutedEvent` and is silently
   counted `unrouted_skipped` (`worker.rs:285`). `load_aggregate_envelope_fields`
   (`worker.rs:412`) handles the same two and no more. **That is what
   `[orders] published=0 unrouted=36` was.**
4. **P7's premise is partly wrong.** The drain *does* `eprintln`
   (`state.rs:298`, added `29d6092`) and its lines **were observed** in the M5
   run. The real question is which build/attach state loses them — a windowed
   release build has no console, and `RunEvent::Exit` may run after stdio
   teardown. **Fix the channel, not the line.**
5. **P2 is wider than orders.** `23505` (unique_violation) is mapped in five
   contexts; **`23503` (foreign_key_violation) is mapped nowhere.**

---

## §50.1 — the authority call for aggregator orders (DECIDED)

**There are two aggregates, not one.** Recorded here in full because it is the
largest design decision in M6 and it must not be rediscovered from a transcript.

- **`aggregator_order` — CLOUD-AUTHORITATIVE**, syncs down, **replace-not-merge**.
  It is an inbound document from an external system that the till cannot receive
  directly, **because the till has no public address**. The platform's own status
  changes land here.
- **`order` — EDGE-AUTHORITATIVE**, syncs up, append-only, exactly as today. The
  edge creates one from the inbound document, linked by `external_order_id`, and
  from that moment it is a local transaction carrying every offline guarantee the
  product already makes.

**Why not one aggregate.** Making `order` cloud-authoritative when the channel is
an aggregator and edge-authoritative otherwise is **split authority on one
aggregate** — the thing the contract rubric forbids — and it would kill the
differentiator for exactly the orders where it matters most: a delivery-heavy
outlet could not modify, bill or close an aggregator order with the line down.
That is not a variation on the product, it is the opposite of it.

**The state round-trip splits the same way.** Accept / ready / picked-up
originate at the till, so they ride the existing edge→cloud→platform path on the
**edge-authoritative** `order`. Platform-originated changes — cancellation, rider
assigned — arrive on the **cloud-authoritative** document and are **surfaced to
the till, never silently applied** to the local order.

**The consequence, stated plainly and published as the guarantee:** *a new
aggregator order cannot arrive while the uplink is down; one that has already
arrived is fully operable offline.* This is true of every competitor too.

This is the first decision of **ADR-022** (drafted alongside this file, status
PROPOSED, **no tables drawn**).

---

## Phase order

**Sync gaps → admin → aggregators.** Aggregator orders flow through the same
outbox that is wedged today; building on top of it would bury the defect. Admin
precedes aggregators because acceptance criterion 5 needs it and because
aggregator menu push needs a menu surface that is not `devseed`.

### Phase A — the seven sync gaps (NO contract change; starts immediately)

Addressed in this order.

| ID | Gap | Work |
|---|---|---|
| **A1 / P2** | The cloud returns **500 for a client-data failure**. An FK violation on a replayed order falls through to `httpx: unhandled error`. | Map `23503` to **4xx with a reason the edge can record**, at the `httpx` boundary. Then **audit every ingest handler for other unhandled paths — this one was found by accident.** Enumerate the **sinks, not the handlers**: every `httpx.Error`-returning path per bounded context. |
| **A2 / P3** | **Head-of-line blocking** — one unreplayable row strands the whole outbox. | `worker.rs:66-70` states the ordering requirement is **per-aggregate, not global**. The drain must skip past a blocked aggregate and continue. |
| **A3 / P4** | **Retry budget never spends on 5xx-classified rows.** | **Counting and blocking are different decisions**: every attempt increments; classification decides only whether the row *blocks*. Add an age-or-attempt ceiling above which even a transient-classified row is **surfaced to a human**. Note the general outbox has no per-entry budget at all today; procurement and ranged both do. |
| **A4 / P6** | `Offline` conflates four states: no listener, listening-but-refused, stale pooled socket, answered-with-error. | Transport half is done (`262e03a`). **Reporting half only:** reuse the three-probe fail-closed logic in `scripts/check-cloud-unreachable.ps1` rather than writing a second, weaker version. |
| **A5 / P5** | **No periodic sync pump.** Drain runs at startup and shutdown only, so an abnormal exit means the day never leaves the till. | A timer calling the already-bounded `AppState::drain_outbox`. |
| **A6 / P7** | The shutdown drain is **silent on success and on failure** in at least some builds. | **Establish whether stderr is still attached at that point in Tauri teardown.** That is a different fix from adding a log line. |
| **A7 / P1** | ~120 rows pending since M1 with **no edge route**. | Routes and envelope fields for `kot`, `invoice`, `payment`, `cash_shift`, `stock_count` and item availability. Cloud routes already exist (correction 3). |

**HARD SEQUENCING RULE: do not touch the cloud/edge menu seed drift (cloud has 2
`menu_item` rows, edge seeds 43) until A1 and A2 are closed and their tests are
RED-THEN-GREEN.** That drift is **the stimulus, not the defect**. Seeding the
cloud makes the 500 disappear, makes the drain look healthy, and **ships both
defects looking like a fix**. This sentence goes in the retro verbatim.

### Phase B — `apps/admin`

New Vite + React + TypeScript + TanStack application. Menu and pricing, suppliers
and pack sizes, purchase orders, staff and permissions, goods-receipt list.

### Phase C — aggregators

**Contracts 0.7.0 lands AFTER Phase A is green, not before.** A1–A3 change the
outbox the aggregator tables will ride on, and bumping contracts across a wedged
outbox would bury the same defect twice.

ADR-022 is drafted now and **escalated before a single table is drawn**.
`order` already carries `external_order_id` and `aggregator_discount_paise`;
there are **no** aggregator tables anywhere in 0.6.3 — no platform credential, no
menu/item mapping, no webhook dedupe, no snooze state.

---

## Acceptance criteria — APPROVED

Every criterion names **the observation and the falsifying condition**, because
M5's binding lesson is that *a criterion satisfied by two different
implementations cannot tell you which one you built*. **The falsifying condition
must be watched failing before the fix**, per §66 — a guard nobody has watched
fail is not a guard, and that includes precondition scripts.

| # | Observation | Falsifying condition — watched FIRST |
|---|---|---|
| 1 | An aggregator order already received **bills, prints and closes with the cloud provably unreachable** | `scripts/check-cloud-unreachable.ps1` watched printing **STOP with the cloud up**, then all three probes agreeing with the backend stopped **by PID** |
| 2 | A stock-out at the till **snoozes the item on the platform**, observed on the platform's own sandbox surface | **PARKED — see below.** Would be: snooze with the push path disabled → sandbox still shows the item available |
| 3 | A permanently-rejected outbox row **blocks itself and not its neighbours** | The same fixture on the **pre-fix** binary strands the neighbours; neighbour counts recorded both times |
| 4 | An order placed offline **reaches the cloud without the operator closing the application** | Kill the app with `taskkill` (so `RunEvent::Exit` never fires) → the row is still pending; then the periodic pump lands it with the window open |
| 5 | A supplier and pack size created in the admin console makes the next receipt **convert exactly and raise no `NO_SUPPLIER_ITEM`** | Receive **before** creating them → gap recorded, so its absence afterwards means something |
| 6 | A goods receipt is **readable back in-product** with its line quantities and totals | Field-by-field against the edge row, with a fixture that **populates every provenance field** (contracts 0.5.9's lesson) |
| 7 | A client-data failure is reported as **4xx with a reason the edge records** | Replay an FK-violating row on the **pre-fix** binary → 500, budget uncharged; after → 4xx, reason stored, row surfaced |

Criterion 4's `taskkill` falsifier **retires the abnormal-exit question (P7's
sibling) at the same time as it proves the pump**.

Criterion 7 was added during planning: **P2 is the milestone's first defect and
nothing else observes it.**

### Criterion 2 is PARKED, not softened

Swiggy and Zomato partner sandboxes need a signed partnership this project does
not have. **Do not evidence a snooze from our own log — that is the definition of
a criterion that cannot fail.**

ONDC was checked rather than assumed, 2026-09-02:

```
https://preprod.registry.ondc.org/        -> 403  (server answered)
https://staging.registry.ondc.org/        -> 000  (no answer, 12s)
https://pilot-gateway-1.beckn.nsdl.co.in/ -> 000
https://staging.gateway.proteantech.in/   -> 000
```

One host answers; three do not. **Answering is not access:** the ONDC registry
requires a subscribed, whitelisted participant with a signed key pair and entity
details — the same signed-partnership shape as the other two, not a public
sandbox.

**So criterion 2 is PARKED with the trigger "when platform sandbox access is
granted"**, in the same register as the ESC/POS hardware gate. **M6 closes
without it rather than with a fake pass.** If access is granted mid-milestone,
criterion 2 is observed on ONDC and **named ONDC-only** in the record.

---

## Carried forward, unresolved

Neither changes any verdict. Settle both the next time Docker is up; the SQL is
already recorded in `docs/m5-acceptance.md`.

- **The ~120 pending outbox rows** — count not re-verifiable today (PostgreSQL is
  down and the edge database is encrypted).
- **The GRN/20260902 ordinal reconciliation** — the pre-drain baseline named the
  pending receipts `0001`/`0002`, the post-drain comparison named them
  `0002`/`0003`. One is wrong; recorded as UNRESOLVED rather than guessed.

---

## Standing rules for the execution session

These are not new. They are repeated here because this file is the only thing the
fresh session is guaranteed to read.

- **§84 rule 8 — no success claims without executed verification. Report
  Executed vs Read-verified separately, always.**
- **§66 — break the property, watch the check fail, then fix.** A guard nobody
  has watched fail is not a guard. **Precondition scripts included.**
- **A test invocation that reports zero tests executed is a FAILURE, not a pass.**
  Run test commands through `node scripts/assert-tests-ran.mjs <cargo|go|vitest>
  -- <command>`. Never pipe a test command through `tail`: the pipeline reports
  `tail`'s status.
- **Milestones close on observed behaviour, not a passing suite**, and **a
  milestone does not close until its acceptance evidence is committed to the
  repo.**
- **Findings triage:** breaks a criterion of this milestone → block; otherwise
  **backlog with a named trigger**. `docs/backlog.md` is the only register.
- **Prefer structural enforcement over documented obligations.**
- **Contracts are the orchestrator's alone and are frozen.** Version bump + ADR +
  operator approval + full consumer list, and **not landed until every layer
  carries it** — schema, Go struct, Zod schema, OpenAPI, repository INSERT/SELECT,
  wire serialiser.
- **History-rewriting git stays denied. Never disable the sandbox. Two identical
  failures is the limit. Stage claimed paths only — never `git add -A`.**
- **Run the backend in its OWN WINDOW** (`scripts/dev-up.ps1`), never as a
  session-owned background process — that interfered with acceptance evidence in
  M5. **Step 0 of any acceptance run verifies it by PID, not by the port
  answering.**

**Escalate immediately:** anything that changes a frozen contract, changes what a
stored number means, crosses the §50.1 authority split, or trades a criterion for
scope.
