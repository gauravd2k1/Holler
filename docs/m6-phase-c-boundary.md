# M6 Phase C — boundary report

**Date:** 2026-09-11
**Written at:** the Phase C boundary, after contracts 0.8.1 landed
**Status of M6:** seven of eight criteria met, one PARKED, nothing blocking

This report is the handover at the end of Phase C. It states what is done, what
is deliberately not done, what was learned the hard way, and what the next
session must not redo. Where it disagrees with a plan document, **the repository
is the authority** — every claim below names the artefact it rests on.

---

## 1. Where M6 stands

| # | Criterion | State |
|---|---|---|
| C1 | Aggregator order bills and closes with the cloud unreachable | **MET** 2026-09-10 |
| C2 | Stock-out snoozes on ONDC staging | **PARKED** — platform sandbox access |
| C3 | A permanently-rejected row blocks itself and not its neighbours | **MET** 2026-09-11, both halves |
| C4 | An offline order reaches the cloud without the operator closing the app | **MET** 2026-09-11, strict run |
| C5 | Supplier and pack size created in admin convert on the next receipt | **MET** 2026-09-10 |
| C6 | A goods receipt is readable back in-product | **MET** 2026-09-10 |
| C7 | A client-data failure is reported as 4xx with a reason the edge records | **CLOSED** 2026-09-07 |
| C8 | An aggregator order flows through both adapters | **MET** 2026-09-10 — `SHAPE ONLY — no integration evidence` |

Evidence for each is in `docs/m6-acceptance.md`, naming what was observed, by
whom, on what date, and on which binaries. **C2 is the only criterion M6 will
close without**, behind the trigger *when any platform sandbox access is
granted*. C8's integration half travels to M6.1 as an explicitly unmet row.

### The M6 boundary list

One item, **contracts 0.8.1**, and it is **LANDED** (`2615cac`, ADR-026). The
list is otherwise empty. Phase D (Network Participant paperwork) is
calendar-bound, consumes no engineering time, and sets M6.1's start rather than
M6's close.

---

## 2. Contracts 0.8.1 — what landed and what deliberately did not

`order.source` now carries **`AGGREGATOR`** and **`TABLE_TAB`**. Full reasoning
in `docs/adr/ADR-026-order-source-widening.md`; the parts a later session must
not re-litigate:

- **One generic aggregator member, never one per platform.** This reverses the
  boundary-list item as written ("name the platform"), which was escalated and
  ruled on before any code was written. `aggregator_order.platform` is free
  `TEXT` because a new platform must not need a migration (contracts 0032), and
  a platform-named member would become a new false claim the first time a
  different platform used the same path.
- **The two existing platform-named members are deprecated, not removed.**
  Removal is breaking; the trigger is the next breaking bump. Enforcement, not
  assertion: the postgres migration raises and the SQLite pre-condition refuses
  to run if any row carries one.
- **Nothing writes either new member yet, and that is pinned.**
  `scripts/check-order-source-drift.mjs` fails the build if `AGGREGATOR` or
  `TABLE_TAB` is emitted outside a test module. The change that starts writing
  one removes it from that list **in the same commit**.

### One open item carried out of it

**0035 is applied in PostgreSQL only. The live edge database has NOT taken it.**
Decided deliberately: applying it rebuilds the `order` table, and no trustworthy
backup can be taken while the DB-sealing defect is open — the `.enc` is only as
current as the last successful seal, and the plaintext half must never be copied.
With no writer for either member, a lagging edge schema changes no behaviour.
**It applies at the next clean bootstrap, on an empty database.** The runner fix
(snapshot before a rebuild-class migration, restore on failure) is filed in
`docs/backlog.md` with the trigger *before the first pilot*.

---

## 3. What Phase C's sittings found, beyond the criteria

Four findings, all filed in `docs/backlog.md`, all triggered *before the first
pilot*. Two of them are more important than any criterion in this milestone.

### 3.1 Every `ItemAdded` is refused, and the cloud's copy of an order is a create and nothing else

The cloud seeds **one** `menu_item_variant` row for the seeded outlet; the edge
mints one per item. `order_item` carries a foreign key to `menu_item_variant`, so
an item event is refused with `missing_reference` **even when the menu item it
names exists cloud-side**. `order` has no such foreign key, so the create always
lands.

The consequence is the whole finding: **the cloud's copy of every order is its
create event and nothing after it.** Totals in the cloud agree with neither the
till nor their own lines. This was filed twice as "the cloud copy does not track
the till, cause unknown" before being identified.

Two details that matter for whoever fixes it:

- **A fix for one defect uncovered this one.** Before `7e88d1c` the till
  hardcoded `variantId: null`, satisfying the foreign key trivially. 35 landed
  lines carry a null variant; 2 carry a real one. The M4 criterion-1 fix started
  sending real variants to a cloud that had none, and nothing announced it.
- **This is the third catalogue in the config-push family**, after the inventory
  and menu entries already filed, and the first with a consequence on the order
  path. **Do not dismiss it as a dev-seed artefact**: the config push has never
  been demonstrated to move a row for *any* catalogue, so "production ids match
  by construction" is the assumption under test, not a given. What is missing
  independently of any seed is that **nothing validates a line's references
  before the row is queued**, and the failure mode is not an error anyone sees.

### 3.2 The till is sluggish while the cloud is unreachable

Observed with the cloud confirmed down by all three probes: every pump tick takes
the database lock and walks the pending backlog against a dead uplink, and the UI
waits behind it. **ADR-013's promise is that an outlet is unaffected by a dead
uplink**, and an outlet with no connectivity is the normal case, so this is that
ADR's own guarantee failing under the condition it was written for. Severity at
the shipped 60-second interval is **unmeasured** — the runs used 10 seconds so a
tick could be watched inside a sitting.

### 3.3 The sync banner is unreadable on a till

A narrow scrolling column of truncated order ids, one line per outbox row rather
than per order, with no item, total or time. It is **the only surface on which a
permanently-rejected row is ever shown to a human** — A3 built the surfacing and
this is what it renders. Filed to land with the two banner entries already open
(the `aggregate_id` duplication and the top-bar overlay) as one pass over the
component.

### 3.4 The Orders screen renders raw UTC

`2026-09-10T13:33:20.733Z` for an order rung at 19:03 local. Reported as a clock
fault by the operator mid-run before being identified. Cosmetic, no data
implication, on the screen an operator uses most.

---

## 4. Method corrections from this phase — read these before the next acceptance run

Written up in full in `docs/retro.md` (four entries, 2026-09-11). In short:

- **A criterion a pre-fix binary also passes is not evidence for the fix.** C4's
  first run satisfied its falsifier's wording and was discarded, because a
  startup drain that predates the feature explains it equally well. Write the
  falsifier against the alternative explanations, not against the feature.
- **A reconstructed pre-fix binary reproduces the shape of a defect, not its
  duration.** Removing A2's line reproduced head-of-line blocking for ~50
  seconds, because A3's retry budget — built later — puts a ceiling on it. A
  reconstruction removes a fix; it does not restore the world that fix was
  written in.
- **Name the exact constraint a fixture depends on, and reproduce it in
  isolation, before building a run on it.** C3's fixture was planned on the menu
  seed drift and the run falsified that assumption; one rolled-back `INSERT`
  would have named the variant key in the first minute.
- **A check is falsified by what it lets through.** The aggregator boundary check
  had two holes — it could not see `packages/contracts`, and its word boundary
  could not match `AGGREGATOR_ONDC` — both found by planting a value and watching
  it **pass**.

---

## 5. The state of the live development stack

As this report was written:

- **Postgres, Redis, NATS:** up, healthy. Postgres carries 0035.
- **Backend:** pid **58148** (restarted to apply 0035). Verify by identity, never
  by the port answering.
- **POS:** pid **26140**, running the binary built from `df5e030`, which is
  `main` with no local modifications. Its edge database has **not** taken 0035.
- **13 rows are permanently blocked in the edge outbox, and they are FIXTURE, not
  a fault.** Each is `missing_reference (HTTP 422)` on the variant foreign key,
  produced deliberately by C3's runs. **Nothing was seeded to clear them**, per
  the standing rule that the drift is the stimulus rather than the defect.
  To clear them, bootstrap the edge from a clean seed — which is also how 0035
  reaches the edge. Do **not** clear them by seeding the cloud's catalogue: that
  would make the symptom disappear while leaving §3.1 unfixed, which is the exact
  failure the A1/A2 sequencing rule was written against.

---

## 6. The table-ordering handover (ADR-025)

**No code exists and none is M6 scope.** The shape is settled and written down so
that it is not re-derived:

- **Architecture:** `docs/architecture/SYSTEM_ARCHITECTURE.md`, section
  "Table-side ordering (customer tab)".
- **Decision:** `docs/adr/ADR-025-table-ordering-device.md`, **PROPOSED**, eight
  numbered decisions — the tab is a LAN client of the till like the KDS and never
  a cloud client; `table_device` is a new principal kind enforced at the LAN
  boundary rather than in the tab's UI; `ordering_mode` and `tab_enabled` are
  cloud→edge config on the A5 loop; binding rides `table_session` with no new
  aggregate; the tab is append-only, which is what makes per-aggregate ordering
  sufficient against tab-versus-waiter edits; availability is edge state answered
  by the edge with a typed rejection; payment stays on the till.
- **Follow-ups:** eight entries in `docs/backlog.md`, each with its own trigger.
  Customer-side payment is **PARKED with no trigger**, deliberately.
- **Landing:** **M8**, trigger *after the first pilot runs on `STAFF_ONLY`*.

**Prerequisites, in order, with current state:**

| # | Prerequisite | State |
|---|---|---|
| 1 | **M6 A7** — `kot` and the other aggregates have no edge route | **OPEN.** A tab on top of this is a customer ordering into a queue nothing drains |
| 2 | **The `order.source` widening** | **DONE** — contracts 0.8.1, ADR-026 |
| 3 | **LAN security gate review** for a device the public holds | **OPEN** |
| 4 | **A pilot running `STAFF_ONLY`** | **OPEN** |

Both modes must coexist within one outlet; that is a requirement, not a phase.

---

## 7. What the next session should do

1. **`docs/pilot-readiness.md`** — the consolidated list of everything triggered
   *before the first pilot*, plus the Phase A and Phase B carries. Being written
   with this report.
2. **Close M6** — the acceptance file is complete and committed; what remains is
   the decision to close with C2 parked, which is a human call.
3. **A7** if pilot work starts, because it gates both the table tab and any
   aggregate beyond `order` replaying at all.

**Do not** re-run C1, C3, C4, C5, C6, C7 or C8, and do not reconstruct any
verdict from git history — every one is written up with its observation,
falsifier and date in `docs/m6-acceptance.md`.
