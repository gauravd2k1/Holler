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

1. **Aggregator integration — the FRAMEWORK and the ONDC IMPLEMENTATION, NOT a
   live channel on any platform.** Inbound orders, menu and availability push,
   order state round-trip, stock-out snooze, behind one internal contract proven
   by two working adapters. **Swiggy and Zomato are loud placeholders**;
   ONDC Network Participant certification is a named gate **outside** this
   milestone. **"Aggregators done" does not mean "orders arriving from Swiggy",
   and nobody may read it that way six weeks from now.**
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

**Sync gaps → admin → aggregators, with the NP paperwork (Phase D) running
alongside Phase A from week one.** Aggregator orders flow through the same outbox
that is wedged today; building on top of it would bury the defect. Admin precedes
aggregators because acceptance criterion 5 needs it and because aggregator menu
push needs a menu surface that is not `devseed`. Phase D is calendar-bound and
engineering-free, so it starts immediately and in parallel — it is the only item
that working faster cannot compress.

### Phase A — the seven sync gaps (NO contract change; starts immediately)

Addressed in this order.

| ID | Gap | Work |
|---|---|---|
| **A1 / P2** | The cloud returns **500 for a client-data failure**. An FK violation on a replayed order falls through to `httpx: unhandled error`. | Map `23503` to **4xx with a reason the edge can record**, at the `httpx` boundary. Then **audit every ingest handler for other unhandled paths — this one was found by accident.** Enumerate the **sinks, not the handlers**: every `httpx.Error`-returning path per bounded context. |
| **A1b / P2 follow-on** | Two more ingest paths report a client-data failure as **500**, each with a named consequence. | `kitchen.InsertKot` carries `order_id`, so an order the cloud rejects also produces a **KOT that can never land**. `payments.InsertPayment` carries `outlet_id`, `order_id`, `cash_shift_id` and `reverses_payment_id`, and `payment` is **append-only in both stores**, so a wedge strands money movements with nothing later correcting them. Each gets its **own red-then-green**, not a shared one. Named as its own gap rather than folded into A2: **A2 shrinks the blast radius of every unclassified sink at once; classifying two contexts fixes two.** The remaining ten sinks are in `docs/backlog.md`, not carried in the audit file. |
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

#### The A1→A3 sequencing invariant — DECIDED 2026-09-03

**At no commit boundary may an order become droppable with no operator trace.**

A1 maps `23503` to **422 `missing_reference`**. The edge's
`is_permanent_rejection` (`edge/sync/src/ranged.rs`) treats every 400..=499 as
permanent, so on its own that change makes an FK-violating order **die** — and
nothing surfaces a permanently-rejected general-outbox row to a human today
(`sync_replay_block` covers the ranged and procurement streams; the general
outbox has no per-entry budget at all). A3 is what adds the budget, the ceiling
and the surfacing. Between the two commits the order would be dropped silently,
which is a **worse** failure mode than today's loud wedge.

Two ways to satisfy the invariant were weighed. Collapsing A1+A2+A3 into one push
satisfies it only by **un-splitting evidence we deliberately split**, and it means
a slip in A3 stops A1 landing at all. **Rejected.** The decision is the additive
one:

- **A1 lands alone and HOLDS the row**, with a test asserting the row is
  **HELD, not rejected**.
- **A3 adds the budget and the surfacing to the general outbox**, and its own
  test asserts the row is **permanent-and-surfaced**.
- **The two tests are mutually exclusive by construction** — A1's requires the
  row still pending and unblocked, A3's requires it blocked and visible — so
  **A3 cannot go green while A1's behaviour survives**. Forced removal, not
  remembered removal, the same move as every structural guard in this
  repository.

**CORRECTION, 2026-09-03, made while implementing A1 — the repository wins.**
The plan said A1 would add `422` to the edge's non-permanent set beside
`401/403/404/408/429`. **That carve-out is unnecessary AND harmful, and was not
written.** Two facts, both checked:

- `is_permanent_rejection` has exactly **two** callers — `ranged.rs:209` and
  `procurement.rs:248`. **The general outbox is neither.** `pump_outbox` records
  the attempt, stops, and leaves the row unpublished whatever the status is
  (`worker.rs:374-378`), so an order is **already held** after A1 with no edge
  change at all. The exposure the invariant was written against does not exist
  on this path.
- Adding the carve-out would have **regressed the two streams that already do
  this correctly**: `ranged_replay.rs` asserts a `422` spends the budget, lands
  in `sync_replay_block` and becomes visible on the POS — permanent-and-surfaced,
  which is A3's end state, already true there since contracts 0.5.8. Making
  `422` transient would have taken that away and retried a known-bad ledger
  entry forever.

So A1's edge half is **a test, not a behaviour change**: it pins "held, not
dropped" so A3 cannot quietly remove it. The invariant is unchanged and still
holds at every boundary; only the mechanism the plan predicted was wrong. **The
guard was watched failing first** — `pump_outbox` was temporarily patched to mark
a 4xx-rejected row published, and the test failed with `the row must still be in
the outbox: left: 0, right: 1`, before the patch was reverted.

Interim states, all acceptable, none dropping an order:

| After | Behaviour |
|---|---|
| today | 500, loud wedge |
| **A1** | 4xx on the wire, row **held**, still wedging |
| **A2** | row **skipped and retained** — head-of-line blocking gone |
| **A3** | row **rejected, budget charged, surfaced** to a human |

**Caveat binding on A2's commit message:** after A2 the held row is retained but
**not visible**. Retained is not dropped, so the invariant holds — but A2 must not
claim visibility. That is A3's, and **M6 C7 closes at the end of A3**, not A1:
C7's falsifier requires "reason stored, row surfaced", and both are A3 work. A1
puts the 4xx and the machine-readable code on the wire and leaves C7 **OPEN**.

No throwaway persistence is written in A1 to close C7 early. A second, weaker
record replaced one gap later is the exact thing A4 is told not to build.

### Phase B — `apps/admin`

New Vite + React + TypeScript + TanStack application. Menu and pricing, suppliers
and pack sizes, purchase orders, staff and permissions, goods-receipt list.

### Phase C — aggregators

**What Phase C actually buys, in one sentence: M6's aggregator phase delivers the
FRAMEWORK and the ONDC IMPLEMENTATION — not a live channel on any platform.**
Write it that way everywhere. Six weeks from now nobody may read "aggregators
done" as "orders arriving from Swiggy". **The framework is the durable asset;
ONDC is the forcing function that proves it carries an async protocol; the
channels arrive when access does.**

**Contracts 0.7.0 lands AFTER Phase A is green, not before.** A1–A3 change the
outbox the aggregator tables will ride on, and bumping contracts across a wedged
outbox would bury the same defect twice.

ADR-022 is drafted now and **escalated before a single table is drawn**.
`order` already carries `external_order_id` and `aggregator_discount_paise`;
there are **no** aggregator tables anywhere in 0.6.3 — no platform credential, no
menu/item mapping, no webhook dedupe, no snooze state.

#### Scope: ONDC for real, Swiggy and Zomato as loud placeholders

**ONDC is implemented for real.** Swiggy and Zomato are **adapters behind the
same internal contract, not implemented**, and the framework around them is built
properly. Four conditions bind that, and condition 2 is the important one.

**C-1. A placeholder must FAIL LOUDLY, NEVER SILENTLY.** No Swiggy or Zomato
adapter may return success, return empty, or no-op. Invoking one produces a
single explicit **`PlatformNotImplemented`** error naming the platform and the
operation, **at the first line of the call**. No default selection, no fallback
that quietly routes to it, no config value that can accidentally point at it.
**An unimplemented adapter that returns a plausible-looking nothing is the same
defect as a stream reporting `published=0` while rows sat unrouted** — this
milestone's P1, one layer out.

**C-2. PROVE THE ABSTRACTION WITH TWO WORKING ADAPTERS, NOT ONE.** A one-adapter
abstraction is indistinguishable from no abstraction: the internal contract
silently takes the shape of ONDC and nobody notices until the second platform
arrives and the whole thing is redone. **Same family as a test that constructs
its own subject.**

So a **second real adapter** is built against a **local fake platform speaking a
conventional synchronous REST shape** — the documented Swiggy/Zomato style — with
recorded fixtures. **Not a stub: a working adapter against a working fake.** If
the internal contract carries ONDC's async callback model and a sync
request/response model **without special-casing either**, it is right. If it
needs a branch on platform identity anywhere in the core, it is wrong — and that
is discovered now for the price of a fake server rather than later for the price
of a rewrite.

**C-2a. THE BECKN FAKE IS GENERATED FROM ONDC'S PUBLISHED ARTEFACTS, NEVER FROM
OUR READING OF THE SPEC.** A fake we author proves only that we agree with
ourselves; conformance against it demonstrates that the adapter matches our
*interpretation*, not ONDC — and it arrives in the one place where no external
check exists for six weeks. Generate it from the **official JSON schemas, the
published example payloads, the reference flows and the applicable layer-2
config**. **Cite the source and version of every artefact in the fake's own
header**, so a future session can tell what it was built from.

Reachability checked 2026-09-02: `github.com/ONDC-Official/ONDC-RET-Specifications`
and `ondc.org` both answer **200**. (Two `raw.githubusercontent.com` paths
returned 404 — those were *guessed* file paths, not evidence of absence. The
execution session pins the real paths and their versions.)

**If those artefacts cannot be obtained, say so plainly and label the ONDC
adapter "UNVERIFIED AGAINST THE SPECIFICATION" in the acceptance file, with
certification as the only thing that can lift the label.** Do not quietly let a
self-authored fake stand in for conformance.

**C-3. BE EXPLICIT ABOUT WHICH ARTEFACT IS EVIDENCE OF WHAT.** The local fake
proves **the contract shape**. Only ONDC staging proves **the integration**.
Record them **separately** in the acceptance file and never let the fake stand in
for the real thing — **M5's lesson was that a harness proving replay is not the
product proving replay, and that mistake cost a milestone.**

**C-4. ENFORCE THE BOUNDARY STRUCTURALLY, NOT BY CONVENTION.** A drift check
fails the build if any platform-specific identifier appears outside its own
adapter module: **`swiggy`, `zomato`, `ondc`, `beckn`, `on_search`, `on_confirm`,
`on_status`, `on_cancel`**, the other `on_*` callback names, and the signing
header names (`Authorization` / `X-Gateway-Authorization` in their Beckn sense).
**A documented obligation to keep the core platform-agnostic holds until the
first person is in a hurry. A check does not get tired.** Falsified before
trusted, per §66.

#### The cut line is TWO-STAGE — DECIDED

**Build what a published schema can verify. Defer what only a live registry can
verify.** One principle, applied twice.

**Stage 1 — M6.** Framework, both adapters, and the **Beckn message surface**.
The surface is driven by ONDC's published schemas, so a schema-generated fake
**actually checks something**, and it is the forcing function that proves the
internal contract carries an **async callback protocol** rather than a
REST-shaped one. That is the whole reason ONDC went first.

**Stage 2 — M6.1.** Three items leave M6, **not because they are hard, but
because nothing we have can verify them**:

- **Ed25519 signing** (BLAKE-512 digest, auth headers, key management) — there is
  no counterparty to sign for.
- **Registry `subscribe` + `/on_subscribe` X25519 challenge decryption** — there
  is no registry to subscribe to.
- **The public HTTPS callback ingress** — nothing legitimate can call it.

Building them now means **checking crypto against a fake registry we authored** —
the same shape as a harness proving replay while nothing hosted the worker, and
the same shape as a fake we write from our own reading of the spec (C-2a).
A self-authored counterparty cannot falsify a signature scheme; it can only agree
with it.

**M6.1 is triggered by Phase D completing** — legal entity, whitelisted domain,
SSL certificate, public callback host. When that lands, the three are built and
verified **in one stretch against the real registry**, and the **ingress security
gate is reviewed then, against a surface that actually receives traffic**.

**Certification stays outside both**, the same shape as the ESC/POS hardware
gate: the code is done, the external thing is not.

Sizing, measured against this repository's own velocity (whole history is 26
calendar days, 2026-08-07 → 2026-09-02, ~13 commits/day):

| Work | Stage | Ours? | Estimate |
|---|---|---|---|
| Internal platform contract, framework, `PlatformNotImplemented` adapters, drift check | M6 | yes | 0.5–1 wk |
| Local sync-REST fake + its adapter + recorded fixtures | M6 | yes | 0.5 wk |
| Beckn message surface: `search/select/init/confirm/status/cancel/update` + every `on_*`, against the schema-generated fake | M6 | yes | 1.5–2 wk |
| **Phase C subtotal in M6** | | | **2.5–3.5 wk** |
| Ed25519 signing, BLAKE-512 digest, auth headers, key management | **M6.1** | yes | 0.5–1 wk |
| Registry `subscribe` + `/on_subscribe` X25519 challenge, key rotation | **M6.1** | yes | 0.5 wk |
| Public HTTPS callback ingress + its security gate, dedupe, out-of-order and duplicate `on_*` | **M6.1** | yes | 0.5–1 wk |
| **M6.1 subtotal** | | | **1.5–2.5 wk**, starting when Phase D lands |
| NP onboarding + certification | outside both | **NO — a review queue** | 2–8 wk calendar |

With Phase A (~1 wk) and Phase B (~1.5–2 wk): **M6 is ~5–6.5 weeks.** The earlier
scope was 7–9; holding certification inside the close would have been 9–17, up to
four months, **none of the overrun code**.

Three facts drove both stages, and only the first is ours to move:

1. Certification is a review queue with round-trips on log verification.
2. Registry subscription needs a **registered legal entity, a whitelisted domain,
   a valid SSL certificate and a publicly reachable HTTPS callback** — a chain
   that starts with paperwork, not a sprint. Our cloud is `localhost:8080`.
   Probed 2026-09-02: `preprod.registry.ondc.org` → **403**;
   `staging.registry.ondc.org` and two gateways → **no answer**.
3. Callbacks force an architectural commitment: **the callback endpoint lives in
   the cloud backend, never the edge** — consistent with ADR-022, since the till
   has no public address.

### Phase D — NP paperwork (STARTS THIS WEEK, in parallel with Phase A)

**It is the long pole, it consumes no engineering time, and it is the only item
on the list that cannot be compressed by working faster.**

**Phase D now sets M6.1's START rather than M6's close** — which is the point of
the two-stage cut. Nothing in M6 waits on it; everything in M6.1 does. Starting
it in week one means the registry-verifiable work can begin as soon as the
paperwork clears, instead of beginning from zero at that moment.

Record, for each step, **what it requires and who has to sign what**:

| Step | Requires | Signature / owner |
|---|---|---|
| Registered legal entity | Incorporation details, GST registration | Operator |
| Domain whitelisted with ONDC | Owned domain, subscriber id derived from it | Operator |
| Valid SSL certificate | Public CA cert on that domain | Operator |
| Publicly reachable HTTPS callback host | Hosting for the cloud backend's ingress | Operator + this track |
| Registry subscriber keys | Ed25519 signing + X25519 encryption keypairs, generated and stored | This track |
| `subscribe` submission + `/on_subscribe` challenge | All of the above live simultaneously | This track |
| Certification / log verification | A working implementation and ONDC's queue | **Not ours** |

**This table is a status record, not a checklist to tick silently.** An
unstarted row after Phase A is a schedule fact to report, not a detail.

### M6.1 — the public callback ingress is its own SECURITY GATE

**NOT IN M6.** It arrives with the deferred three, when Phase D lands and there
is something that can legitimately call it. Recorded here in full so it is not
rediscovered, and so that no M6 track quietly exposes a public endpoint.

**Reviewed before it accepts a single external request, not after.** This is the
first publicly addressable, externally-authenticated surface this product has
ever had — the same register as M2's LAN socket, which needed device enrollment
before it was trusted. **Reviewing it inside M6 would mean reviewing it against
traffic we generate ourselves**, which is what the two-stage cut exists to
avoid.

The gate covers, at minimum:

- **A written threat model** for the ingress.
- **Signature verification failure modes** enumerated: absent header, malformed
  header, unknown subscriber id, key not in registry, stale key after rotation,
  valid signature over a mismatched digest. **Each one must have a watched
  failing test** — §66 applies hardest here.
- **Replay and duplicate handling**: a dedupe key, a bounded acceptance window,
  and a decision on what a repeated `on_confirm` does. **Duplicate delivery is
  normal in this protocol, not an anomaly.**
- **Rate limiting** on an endpoint anyone on the internet can reach.
- **No credential material in logs or errors** — the standing rule, on a surface
  that will be noisy.

**Nothing is exposed publicly until this gate is reviewed and its falsifiers have
been watched failing** — and in M6, nothing is exposed publicly at all.

**CLARIFICATION A BUILDER MUST NOT GET WRONG.** "Expose no public endpoint in M6"
does **not** mean "do not build the callback receive path". **The callback
receive path IS BUILT IN M6 and is reachable LOCALLY ONLY** — that is what the
schema-generated Beckn fake calls, and it is the async half that makes having two
adapters prove anything at all. What defers to M6.1 is the **public HTTPS
ingress** — the internet-facing host, the signature verification against a live
registry, and this security gate.

Read the other way, a builder deletes the async shape, both adapters collapse to
request/response, and **M6 C8 stops proving the one thing it exists to prove.**

---

## Acceptance criteria — APPROVED

**THESE ARE M6 CRITERIA. "C1".."C8" IN ANY M6 CONTEXT MEAN THE ROWS BELOW,
NEVER M5's.** M5's seven are closed and live in `docs/m5-acceptance.md`; its
criterion 6 must not be re-run and its criterion 7 (weighted average cost) is a
different thing entirely from **M6 C7** (a client-data failure reported as 4xx).
Write **"M6 C7"** in full whenever the number appears in a prompt or a report.

Every criterion names **the observation and the falsifying condition**, because
M5's binding lesson is that *a criterion satisfied by two different
implementations cannot tell you which one you built*. **The falsifying condition
must be watched failing before the fix**, per §66 — a guard nobody has watched
fail is not a guard, and that includes precondition scripts.

| # | Observation | Falsifying condition — watched FIRST |
|---|---|---|
| 1 | An aggregator order already received **bills, prints and closes with the cloud provably unreachable** | `scripts/check-cloud-unreachable.ps1` watched printing **STOP with the cloud up**, then all three probes agreeing with the backend stopped **by PID** |
| 2 | **ONDC-ONLY.** A stock-out at the till snoozes the item **on ONDC staging**, observed on ONDC's own surface and **named as ONDC-only** in the record | **PARKED behind its trigger — see below.** Would be: snooze with the push path disabled → the platform still shows the item available |
| 3 | A permanently-rejected outbox row **blocks itself and not its neighbours** | The same fixture on the **pre-fix** binary strands the neighbours; neighbour counts recorded both times |
| 4 | An order placed offline **reaches the cloud without the operator closing the application** | Kill the app with `taskkill` (so `RunEvent::Exit` never fires) → the row is still pending; then the periodic pump lands it with the window open |
| 5 | A supplier and pack size created in the admin console makes the next receipt **convert exactly and raise no `NO_SUPPLIER_ITEM`** | Receive **before** creating them → gap recorded, so its absence afterwards means something |
| 6 | A goods receipt is **readable back in-product** with its line quantities and totals | Field-by-field against the edge row, with a fixture that **populates every provenance field** (contracts 0.5.9's lesson) |
| 7 | A client-data failure is reported as **4xx with a reason the edge records** | Replay an FK-violating row on the **pre-fix** binary → 500, budget uncharged; after → 4xx, reason stored, row surfaced |
| 8 | An aggregator order flows **end to end through BOTH adapters** — the ONDC/Beckn adapter against the **artefact-generated Beckn fake** (staging needs the registry work deferred to M6.1) **and the second adapter against the local sync-REST fake** — **with no branch on platform identity in the core** | **Introduce a platform-specific branch in the core and watch the drift check go RED**, then remove it. A boundary check nobody has watched fail is not a boundary |

Criterion 4's `taskkill` falsifier **retires the abnormal-exit question (P7's
sibling) at the same time as it proves the pump**.

Criterion 7 was added during planning: **P2 is the milestone's first defect and
nothing else observes it.**

**M6 C8 IS SHAPE-ONLY, AND THE ACCEPTANCE FILE MUST SAY SO IN THOSE WORDS.**
With staging deferred to M6.1, **both** adapters now run against fakes we
authored — one generated from ONDC's published artefacts, one written by us. So
C8 proves the **contract shape twice and the integration zero times**. Left
unlabelled, M6 closes with C8 green and a reader takes it as "ONDC works", which
is exactly the substitution C-3 exists to prevent.

Therefore:

- **M6 C8 is recorded as `SHAPE ONLY — no integration evidence`.**
- **The integration row travels to M6.1 as an explicitly UNMET row** (M6.1 C1,
  below). It is carried, not closed, and never merged into C8.
- The artefact-generated fake is the strongest available check on shape and the
  weakest possible check on integration. Both halves of that sentence go in the
  acceptance file.

### M6.1's criteria and its close condition

M6.1 is triggered by Phase D landing. It carries two rows out of M6 and adds its
own close condition, so that "M6.1 is done" cannot be claimed the way "aggregators
are done" could have been.

| # | Observation | Falsifying condition |
|---|---|---|
| **M6.1 C1** | **CARRIED FROM M6 AS UNMET.** An aggregator order flows end to end against **ONDC staging** — the real counterparty, not a fake we authored — proving the **integration** that M6 C8 explicitly does not | Point the adapter at staging with the signing path disabled → the request is rejected by ONDC, not by us |
| **M6.1 C2** | Every enumerated **signature-verification failure mode** is rejected by the public ingress: absent header, malformed header, unknown subscriber id, key absent from the registry, stale key after rotation, valid signature over a mismatched digest | **Each mode watched failing first**, individually. §66 applies hardest on the first internet-facing surface this product has ever had |
| **M6.1 C3** | A **duplicate and an out-of-order `on_*` callback** are handled per the recorded decision, not merely tolerated | Deliver the same `on_confirm` twice → the second is recognised as a duplicate rather than producing a second effect |

**M6.1's close condition:** all three observed, the ingress security gate
reviewed against traffic it did not generate, and **`aggregator_order` documents
arriving from the real registry**.

**Certification has its own landing and is NOT M6.1's close condition.** It is a
named gate — the ESC/POS shape — with the trigger *"ONDC NP certification granted"*,
filed in `docs/backlog.md`. Its duration is a review queue, not a task, and a
milestone whose close depends on someone else's queue is a milestone that stops
closing. **A live channel on ONDC is what certification buys; neither M6 nor M6.1
delivers one.**

### The zero-tests-executed audit — DONE, not outstanding

Recorded here because the review that produced these amendments read it as
outstanding, and the repository says otherwise.

**Executed 2026-09-02, commit `1b51df0`.** All three runners were probed
empirically rather than assumed — `cargo test <filter typo>`, `go test -run
<typo>`, `vitest run -t <typo>` each execute nothing and exit **0** — and
`scripts/assert-tests-ran.mjs` now fails a job on zero executions and is wired
into **13 CI steps**, the three builder agent files and the verifier's rubric.
Every suite count was re-measured through it (`edge/database` 316, `edge/sync` 56,
`apps/pos` 230, and the rest in `docs/m5-acceptance.md`).

**The one residue is Playwright** (`pnpm test:e2e`, KDS), whose summary format the
guard does not parse — **already filed in `docs/backlog.md` with an M6 trigger.**
That row is the whole of what remains, and it needs no new landing.

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

- ~~**The ~120 pending outbox rows**~~ — **RESOLVED 2026-09-03: exactly 120**
  (published 75), and 114 of them have `attempt_count = 0`. See
  `docs/m5-acceptance.md`.
- ~~**The GRN/20260902 ordinal reconciliation**~~ — **RESOLVED 2026-09-03: the
  post-drain naming was right, the baseline was wrong.** Settled on
  `local_outbox.published_at` (0001 at 05:13, 0002/0003 at 05:48), not on
  timestamp clustering. See `docs/m5-acceptance.md`.

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
