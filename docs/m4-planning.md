# Planning inputs for Milestone 4 — Inventory & Recipes

Written at M4 kickoff (2026-08-20), after a competitive gap reconciliation against
Petpooja / Recaho / BillBoox / BhojanSetu and a review pass that returned the first
draft for change. Scope is approved: **M4 = inventory + recipes**.

These are inputs to the M4 task graph, not the graph itself. The contract shapes
are decided in `docs/adr/ADR-018-m4-inventory-contracts.md` (v0.5.0); this file
carries scope, sequencing, deviations and the gates.

---

## 1. Approved scope

**Deliver:** raw materials with units and conversions, recipes and sub-recipes,
modifier-driven ingredient deltas, an append-only stock ledger, automatic
recipe-level consumption on order confirm, wastage recording, physical stock
counts, theoretical-vs-actual variance, and low-stock surfacing at the POS.

**Excludes, absolutely:** procurement / PO / GRN / suppliers; central kitchen;
batch and expiry *alerting* (model the fields, per §81); aggregator auto-snooze on
stock-out; food-cost dashboards; the menu-engineering matrix; the waiter app.

### Stated deviation — counts and variance stay in M4

The prior direction was to push variance and waste to M4.5/M5. This plan keeps
**stock counts and variance inside M4**, and records that as a deviation rather
than absorbing it silently.

The argument: M4's acceptance is that selling a dish produces correct ledger
entries. A physical count is the only mechanism that can *falsify* that claim.
Theoretical deduction is arithmetic over data we control — it will agree with
itself whatever we build. Actual = Opening + Purchases + Transfers In − Transfers
Out − Closing is the independent measurement, and variance is the difference.
Shipping deduction without the instrument that checks it repeats the §66 lesson
from M3: an invariant nobody has watched fail is not a gate.

Wastage *recording* stays; wastage *approval workflow* does not — see §4(d).

---

## 2. Decisions taken at review

### 2.0 Rulings folded in (2026-08-20)

- **Variant binding: NOT NULL.** One recipe per sellable unit; a recipe binds at
  the grain of a price. Nullable was rejected on a structural defect
  (`NULL != NULL` defeats the unique index), not a preference. This triggered the
  **default-variant prerequisite** — sellable items do not all carry a variant
  today — which lands as an additive `menu_item_variant.is_default` plus stamping
  at line creation. ADR-018 §2, §2.1.
- **Deduction gap: cloud-visible**, as a sibling aggregate type on the existing
  ledger ingest route. It is a signal, never a correction: deductions are never
  backfilled when a recipe is later authored, and it appears in variance as a
  named term ("N sales unaccounted"), never folded into shrinkage. ADR-018 §10.1.
- **Base unit: micro (10⁶).** Gram / litre / piece scaled by a million, scale
  carried in the field name (`quantity_micro`). One rule instead of three
  per-dimension choices, no precision floor, and 0.5 piece expressible on the
  same footing as 0.5 ml. ADR-018 §3.
- **Wastage recording stays in T3.** Only the approval workflow moves to M5;
  `wastage.approve` is not defined in 0.5.0.

### 2.1 Deduction is modifier-aware, via a separate delta table

Recipe deduction accounts for modifiers. It is implemented as a distinct
`modifier_ingredient_delta` table keyed on the modifier, **not** by expanding
`recipe_ingredient` to cover variants.

**A modifier with no delta row deducts nothing.** Absence is never read as
consent — the `printer_role` rule from 0.4.7, applied to ingredients: a printer
with no role row is a candidate for neither path, and a modifier with no delta
row moves no stock.

This was a decision reserved for the reviewer and closed in the first draft
without being surfaced. Recorded here so the reservation is visible in the record,
not only the outcome.

Variants (Half / Full) are *not* modifier deltas and were ruled on separately —
see §2.0. Authoring duplication (4 variants = 4 recipes) is mitigated in the
authoring UI with copy-recipe-from-variant, **never in the data layer**: an
authoring convenience that becomes a resolution rule in the deduction path is the
fallback branch the NOT NULL ruling exists to eliminate.

### 2.2 Hardware gate — M4 starts, M3 is not tagged

M3 stays untagged. M4 opens anyway, with **T0 running concurrently with T1**:

- `bundle.windows` in `tauri.conf.json`, embedding the WebView2 and VC++
  runtimes rather than downloading a bootstrapper.
- The installer completing on a **clean 4GB Windows 10 VM with no internet**,
  and the app launching, logging in and creating an order offline on it.
- SQLite open/decrypt time measured on a spinning disk; crash recovery after a
  hard power cut, not a clean shutdown.

No hardware is required for any of this. It has been deferred since M2. It ends
in T0.

**ESC/POS-on-paper remains an open M3 exit gate** and is not in M4 — a physical
printer is being sourced. M3 is code-complete and functionally exercised; it is
not acceptance-complete until that closes.

Also in T0, unrelated to any milestone gate: **move `device_token` out of the
WebSocket query string** to an `Authorization` header or a first-frame auth
message. Since ADR-017 that parameter carries real authority, and it currently
lands in every proxy and access log on the path. This is credential material in
already-shipped code. It is not waiter-app blocked and does not wait for M9.

### 2.8 The config-delivery guard, generalised — and T4's acceptance condition

Three instances is enough evidence that scoping a guard to the last instance
does not hold. `/sync/config` returned an empty `users` array; `printer_role`
never reached an edge at all; `outlet.day_start_time` is read and never written.
Each time the fix was narrower than the class.

**The guard, stated generally:** for every column of every cloud-authoritative
table in the SQLite schema, assert that it appears in `syncConfigResponse` **or**
is explicitly declared edge-local-derived, with a reason per exemption. Same
shape as `SINGLE_STORE_MIGRATIONS`, and for the same purpose — the declaration
is the guard, and an undeclared omission fails rather than passing quietly.

Columns, not aggregates: `day_start_time` is a column on `outlet` and would slip
past an aggregate-scoped check.

**T4's acceptance condition, because a write path is not a working knob:**
acceptance **must exercise a non-midnight `day_start_time`**. Ship the write
path, run every test at `00:00`, and the knob is green without ever having
turned — which is how `day_start_time` reached this state in the first place.

### 2.7 Stopping rule for further contract bumps before T2

0.5.1 and 0.5.2 both landed between T1 and T2, each correcting a defect the
previous version could not express. That is the right call twice and a bad
habit three times, so the test is written down:

> **Does it become impossible, or require rewriting ledger rows, once T2 has
> run? Yes → it blocks T2. No → it is 0.6.0, and T2 proceeds.**

Both bumps passed it: a quantity's dimension and a recipe's output are baked
into every `stock_ledger_entry` the moment deduction starts writing, and that
table is append-only — correcting either afterwards stops being a schema change
and becomes a data migration across immutable history.

**Better error messages, nicer authoring flows and extra indexes do not pass.**
They are all still available at 0.6.0, and none of them are harder to add after
a million ledger rows exist than before.

### 2.6 `printer_role` delivery — fixed in T4, not filed

`printer_role` has existed in SQLite, PostgreSQL, Go and TypeScript since 0.4.7,
and `syncConfigResponse` (`backend/cmd/api/syncconfig.go:134`) has sixteen
fields and **no `printer_roles`**. So an outlet syncing from the cloud receives
zero printer roles, and since a printer with no role row is a candidate for
neither path — deliberately, so absence is never consent — **`print_invoice`
fails by name at every cloud-syncing outlet.** It works in development only
because `devseed` writes the roles locally.

That is a shipped M3 defect, the fifth found during M4 planning. It is fixed in
**T4**, not filed: T4 already opens `syncConfigResponse` to add the inventory
config, so the marginal cost is small and the alternative is leaving a bill
printer unreachable in production while the file is open on the desk.

**Two instances earns a guard.** This is the second defect of the shape *the
contract shape exists, the delivery path does not* — after `/sync/config`
returning an empty `users` array, which had the same consequence for offline
login. T4 therefore also ships an assertion that **every config aggregate the
edge needs appears in `syncConfigResponse`**, so a third instance fails a test
instead of reaching an outlet.

### 2.5 Heartbeat output for long gates — now in T0's tail

Long-running work must emit intermediate progress. **A nine-hour agent with no
intermediate output is unreviewable and un-interruptible: a stuck loop and slow
progress look identical from outside**, and the only way to tell them apart is to
wait for the end, which is the thing that costs nine hours.

Both T0 and T0b ran multi-hour with a single terminal report each. That is the
concrete case; the rule is general and applies to any gate, build or suite that
can run long.

Note for the record: this was believed to be an existing entry in
`docs/backlog.md`, carried through two milestones. It is not there — a
repo-wide search finds "heartbeat" only in `docs/DEV_SETUP.md`, describing the
unrelated KDS LAN protocol heartbeat. So it is filed here as new work rather than
promoted, which is the same outcome by a different route: it is now in a track,
not in a backlog.

### 2.3 Waiter app — M9, and it stops being carried as pending

The waiter/captain app lands in **Milestone 9 (Customer Experience)**. Actions:

- Remove the open decision from `docs/competitive.md` — it has been carried as
  "raise this during M2 planning" through two milestones.
- File it in the backlog with the trigger: **blocked until a multi-writer
  `ReplayTransition` ADR is approved.** `ReplayTransition` treats
  `version <= stored` as a duplicate and silently returns current state, which is
  correct under §50.1 single-writer versioning and becomes a silent-drop the
  moment a tablet and a POS transition one table.

The security half of that blocker is closed: ADR-017 enrollment shipped and the
LAN socket now verifies a real `device_credential_cache` credential.

### 2.4 Ledger retention — snapshots, not deletion, and the shape lands in 0.5.0

`stock_balance_snapshot`, keyed `(outlet_id, inventory_item_id, business_date)`,
sealed at day-end close. Current stock = latest sealed snapshot + entries since,
so a stock read is bounded to one business day forever regardless of ledger age.

Edge-local derived: **SQLite only, no Postgres mirror, no `AggregateType`, no
sync direction** — the `invoice_sequence` precedent.

The snapshot also **removes the materialized current-stock table** this plan's
first draft proposed (ADR-018 §9): current stock becomes a bounded query over
snapshot + entries-since, so there is no second stored representation of a
derived quantity to drift.

**Archival eligibility is structural, not time-based.** A ledger row may be
archived only once (a) its outbox replay is acked by the cloud, **and** (b) a
sealed snapshot covers its business date for its item. M4 computes and reports
eligibility; it deletes nothing. Actual deletion is decided later, on a measured
row count and read latency from the 4GB box — which T0 is the first opportunity
to measure.

**Sealing never depends on an operator.** The bounded-read guarantee holds only
while days get sealed, so sealing is **idempotent and lazily caught up**: on
database open, every unsealed prior day is sealed before the first read is
served. An outlet that skips day-end for a month, or a POS that dies at 11pm,
degrades nothing. A guarantee that depends on a human action is not a guarantee —
the ADR-013 lesson. T6 invariant: skip three days, reopen, assert three snapshots
exist and the balance matches a full-ledger sum.

**Prerequisite this exposes — and it is an M3 defect, not only an M4 input.**
`business_date_from` (`commands/billing.rs`) and the display-number reset in
`edge/database/src/repo.rs` both bucket by **UTC day**. In IST the UTC day rolls
at **05:30 local**, so any outlet trading past midnight is already mis-bucketing
invoice numbers and day-end / cash-shift reconciliation **in shipped code**. The
M3 milestone report currently claims something untrue.

Two consequences, both in the **pre-track**, not T3: the outlet-configured
day-start time and the `business_date` definition derived from it are
schema-level decisions and must be settled before 0.5.0 freezes the column; and
the defect is recorded in `docs/retro.md` and corrected in the M3 acceptance
record.

---

## 3. Sequencing

**Pre-track — contracts v0.5.0.** Orchestrator/architect only, serialized, no
builder touches `packages/contracts/` (ADR-008). See ADR-018. Nothing below
starts until it is accepted and the migrations are wired into
`edge/database/src/migrations.rs`'s `MIGRATIONS` list — a migration on disk but
absent from that list never applies (0009–0011 sat dead; 0005 before them).

Pre-track scope beyond the inventory shapes themselves, all of it schema-level
and therefore not deferrable to an implementation track:

- `menu_item_variant.is_default` + the stamping rule (§2.0, ADR-018 §2.1).
- The outlet-configured **day-start time** and the `business_date` definition
  derived from it — `business_date` is a 0.5.0 column and the snapshot keys on it.
- The `docs/retro.md` entry and M3 acceptance-record correction for the UTC
  bucketing defect (§2.4).

| Track | Deliverable | Notes |
|---|---|---|
| **T0** (concurrent with T1) | Windows 10 gate + `bundle.windows` + **heartbeat output for long gates** | §2.2, §2.5. No hardware needed. Produces the first measured numbers from the target box. |
| **T0b** | Implement `HOLLER_DEV_MENU_SPEC.md` in `devseed` — commit the spec file first | Hard prerequisite of T1/T2. See §5. |
| **T1** | Units, integer conversion, recipe resolution incl. sub-recipes with cycle/depth guards | Pure arithmetic. Same shape as the tax engine; test it to death. |
| **T2** | Ledger + automatic deduction inside the `confirm_order` transaction | Includes the deduction-gap path (§4 rule 2). |
| **T3** | Wastage recording, stock counts, variance, snapshot sealing | Wastage **recording** is in M4; only the approval workflow moved to M5. The business-date definition is settled in the pre-track, not here. |
| **T4** | `backend/internal/inventory` — config write routes, `/sync/config` contribution, envelope-wrapped ledger ingest, **cross-tenant isolation tests for `recipe`, `recipe_ingredient`, `modifier_ingredient_delta`**, **the `printer_roles` and `day_start_time` sync-bundle fixes**, and **the generalised config-delivery guard** (§2.8) | Directory is currently empty. The isolation tests are not optional: `backend/internal/menu` has none today, and adding three tables to an untested boundary is how it stays untested. The retrofit for the pre-existing menu tables stays in the backlog; M4's own tables do not inherit that exemption. |
| **T5** | POS surfaces — stock list, wastage entry, count entry, visible low-stock signal, "items sold with no recipe" report | §64 error design binds: a gap that reaches nobody is not a feature. |
| **T6** | e2e harness invariants, including the skip-three-days sealing invariant (§2.4) | Each **deliberately broken and observed to fail** before being trusted, per the §66 precedent. Persistence round-trip tests are **not** here — see §4(h). |

---

## 4. Contract requirements carried into ADR-018

Recorded here so the plan and the ADR cannot drift. Full treatment in the ADR.

- **(a) Recipe versioning.** `stock_ledger_entry` stores the quantity **actually
  applied** as authoritative, plus `recipe_id` + `recipe_version` as provenance,
  **no FK** (the `order_item_modifier` precedent). The ledger is readable and
  auditable without touching the recipe table. A recipe edit never retro-alters a
  past deduction.
- **(b) Integer quantities.** Canonical base units, integer only: **mg** (mass),
  **ml** (volume), **milli-piece** (count). Conversion factors as integer
  numerator/denominator. No float anywhere in the quantity path — the money=paise
  rule, generalized. Deterministic half-up rounding, applied **once**, at the leaf
  ingredient's applied quantity; sub-recipe resolution stays exact rational
  arithmetic. Edge and cloud compute byte-identical results.
- **(c) Two-tier conversions.** Dimensional conversions (kg→g, l→ml) are global.
  Pack conversions (1 packet paneer = 200 g) are **item-scoped**.
- **(d) No mutable approval flag on an append-only row.** The wastage approval
  workflow is **dropped from M4**, and — deviating slightly from the offered
  options — the `wastage.approve` permission is **not defined in 0.5.0 either**.
  Rule (i) says an unenforced permission is a documented obligation rather than
  structural enforcement; that argument applies to shipping the permission unused
  as much as to `billing.manage`. It lands in M5 together with the append-only
  approval row that enforces it.
- **(e) Cycle detection.** Depth limit + cycle check at cloud write time;
  defensive depth/visited-set check at edge resolution. An unbounded loop inside
  the `confirm_order` transaction would wedge the POS mid-service. The defensive
  check degrades to a deduction gap, never to a failed confirm.
- **(f) Envelope-wrapped ingest.** Ledger and count ingest take a `SyncEnvelope`
  with `aggregate_type` pinned by the route and `direction` pinned by §50.1;
  mismatch is 422, never a coercion. No bare REST writes for an edge-authoritative
  aggregate.
- **(g) Deferred columns land now.** `yield_factor_ppm` and `unit_cost_paise`
  land as real columns in 0.5.0, unused, landing milestone **M5**, pinned by
  exact assertion (the synthesized-canonical-field precedent,
  `edge/database/src/lib.rs:4026`). Retrofitting a multi-million-row ledger is not
  an option.
- **(h) Persistence round-trip test per new table, named per track.** Not folded
  into the T6 e2e suite. A table whose round-trip is only exercised through an
  end-to-end scenario is a table with no round-trip test.
- **(i) `billing.manage` rides along, with enforcement in the same sequence.**
  It must land with an enforced check on the GSTIN write path
  (`backend/internal/compliance`, which today gates on `outlet.manage` — whoever
  may rename a table may set the GSTIN printed on every invoice). A permission
  defined and not checked is worse than the gap it documents.

### Rules to be written into the ADR

1. **Stock never blocks a sale.** Negative stock is permitted and is a variance
   signal, not an error. No `CHECK (quantity >= 0)` anywhere in the stock path.
2. **A missing or broken recipe never fails a confirm.** It records a deduction
   gap, and "items sold with no recipe" is a visible report.
3. **Concurrent deduction is serialized by transaction**, because the edge is a
   single SQLite writer and LAN clients are command clients, not writers. Written
   down rather than left implicit.
4. **The cloud may re-derive a stock view from the ingested ledger. It may never
   mirror the edge projection, and stock never syncs downward.**

---

## 5. The seed menu — currently unsatisfiable, so it is a track

Recipes need real menu items, and T1/T2 must test against the real seed rather
than synthetic fixtures: a recipe suite that passes on fabricated ingredients is
green-on-absent-data, which the M3 `REQUIRED_SHAPES` work already caught once.

State of play, verified 2026-08-20:

- `HOLLER_DEV_MENU_SPEC.md` exists at the repo root and is **untracked**. It is
  not a source of truth until it is committed.
- It describes ~39–41 priced items across 8 categories, 3 tax profiles and 5
  stations. The exact count must be pinned when it lands.
- `edge/database/src/bin/devseed.rs` still seeds the **2-item placeholder**
  (`ITEM_CHAI_ID, "Masala Chai", 4000` and one sibling), one category, one
  station.

So T0b is real work and gates T1/T2. It also pays for itself outside M4: the
spec's own rationale is that a single order spanning 5% / 18% / 40% exercises the
mixed-rate invoice path that `menu_item.tax_profile_id` exists for and that no
seed has ever produced.

---

## 6. Offline-first reconciliation — where the guarantee holds and where it cannot

Carried from the gap scan so M4 is not planned in isolation.

| Feature | Offline verdict |
|---|---|
| **Inventory + recipes + deduction + low stock (M4)** | ✅ Fully preserved. Deduction is a local recipe resolution plus N inserts in the same SQLite transaction as order confirm. The ledger is append-only edge→cloud, exactly the §50.1 shape. |
| Purchase orders (M5) | ⚠️ Approval is inherently multi-party and cloud-side. **GRN receipt must be edge-capable** — goods arrive at outlets whose uplink is down. |
| Aggregators (M6) | ❌ Cannot be offline-first: a webhook needs internet. What is preserved: an order already landed is billable, printable and closable offline, and no core path may depend on the gateway. |
| Deep reporting | ⚠️ Split. Shift and day-end reports (Z-report, sales summary, payment mix, tax summary, discount log, cashier reconciliation) **must** run at the edge offline. Multi-outlet and period analytics are cloud-only. "~80 report types" is a warehouse programme, not a milestone feature. |
| Waiter app (M9) | ⚠️ Preservable, but only as a command client. As a second writer it breaks both `ReplayTransition` and the single-writer serialization M4 depends on (§4 rule 3). |
| CRM + loyalty + WhatsApp (M9) | ⚠️ Accrual and lookup offline: fine. **Redemption cannot be safely offline** — a points balance is a shared mutable counter, and offline redemption is double-spend by construction. Cloud-authoritative, or a per-outlet offline cap with reconciliation, chosen deliberately. WhatsApp is cloud-only and queued. |
| Online-ordering storefront (M9) | ❌ Cannot preserve it and should not try. Constraint: its orders enter by the same cloud→edge path, and the outlet keeps serving walk-ins when it is down. **QR-at-table could be edge-served over the restaurant LAN** and would then survive an uplink outage — something a cloud-only competitor cannot match. |
| Multi-outlet / central kitchen (M8) | ✅ Preserved. Transfers are `TRANSFER_OUT` / `TRANSFER_IN` ledger rows replayed per outlet. Inventory-shaped end to end; cannot start before M4 lands. |
| Tally / accounting | ✅ Irrelevant to the guarantee — export is a back-office act. Highest value-per-effort on the list for the Indian market; wants purchase data, so it follows M5. |
| AI layer (M10) | ❌ Cloud-only by definition. AI never alters financial records without an explicit authorized action. |

---

## 7. Acceptance — observed behaviour, none evidenced by a test harness

CLAUDE.md's rule binds: an acceptance run exercises the binaries that ship, and
if the only thing that starts a component is a test, that component is not wired.

1. Sell one dish **from the real seed menu** through the POS window with the
   network disconnected → `stock_ledger_entry` rows exist for every ingredient at
   recipe quantity × line quantity, **plus** the `modifier_ingredient_delta` rows
   for the modifiers actually chosen, and nothing for modifiers with no delta row.
2. Kill the POS between confirm and deduction → on reopen, order and ledger
   agree. No half-deducted sale. Judged against the crash, not against the API.
3. A physical count entered at the outlet produces a variance report whose
   Actual / Theoretical arithmetic is checked against an independently computed
   figure — the M3 discount and tax precedent.
4. An ingredient crossing its reorder level is **visible to a human on the POS**,
   not only present in a table.
5. An item sold with no recipe completes the sale, records a deduction gap, and
   appears on the "items sold with no recipe" report.
6. Ledger entries created at the edge replay to the cloud and read back
   identically.
7. Stock reads stay bounded after a sealed snapshot: a read on day N+1 touches
   one day of entries, not the whole ledger. Measured, not asserted.

---

## 8. Carried-forward gates (not new work, but they still bind)

- **ESC/POS on paper** — open M3 exit gate. Printer being sourced.
- **The e2e harness CI job still cannot go red on an invariant failure.** It
  asserts harness-level fatals only. M4 adds invariants to a suite that is a smoke
  test rather than a regression gate until that is tightened.
- **`openapi.yaml` is machine-checked against nothing.** It silently drifted on
  three `MenuItem` fields for two versions. v0.5.0 adds a large surface to it.
- **`backend/internal/{auth,menu,tables}` never call `postgres.Migrate`** and
  seed into an assumed schema. T4 adds a context to that pattern; do not extend it.
- **Build-green ≠ dev-works.** Any T5 claim states which runtime it was observed
  in: build output, dev server and browser are three runtimes.
- **`make check-seams`** after any `pub` signature change in `edge/` or
  `apps/pos/src-tauri`. This has broken nine times.
