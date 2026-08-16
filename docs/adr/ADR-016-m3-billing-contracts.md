# ADR-016 — Milestone 3 billing contracts (v0.4.0)

**Status:** Accepted
**Date:** 2026-08-12
**Supersedes:** nothing. **Extends:** ADR-008 (contracts-first), ADR-009/§50.1 (authority split), ADR-011, ADR-014.

## Context

Milestone 3 delivers the tax engine, GST invoice, discounts, split bills, split payments, cash shift and invoice numbering (§81). Contracts were frozen at 0.3.1 and carry none of these shapes. `payment` has sat in `AGGREGATE_AUTHORITY` as `EDGE_TO_CLOUD` since Milestone 0.5 with no payload behind it.

M3 is also the first milestone that puts money on the wire, which raises the cost of every modelling mistake: a wrong authority assignment on an order is an operational bug, the same mistake on an invoice is an accounting one.

## Decisions

### 1. Authority follows §50.1 with no new exceptions

| Aggregate | Direction | Why |
|---|---|---|
| `invoice`, `cash_shift`, `payment` | EDGE_TO_CLOUD | The outlet issues bills and takes money with the uplink down. The cloud only replays. |
| `tax_profile`, `compliance_version`, `invoice_series`, `discount_definition` | CLOUD_TO_EDGE | Tax rules, numbering format and discount policy are management decisions. |

`tax_rule`, `invoice_line`, `payment_allocation`, `cash_movement` and `outlet_fiscal_profile` are **not** aggregates — they are child rows travelling inside their parent's payload or config bundle, the `menu_item_variant` and `station_printer` precedent.

Adding a payload for `payment` is a **fill-in, not a new authority claim**: the direction was decided when the map was written at Milestone 0.5. Recorded explicitly so a later reader does not mistake 0.4.0 for the moment that call was made.

### 2. Invoice numbering splits definition from counter

The load-bearing decision. §33 requires numbering be concurrency-safe with duplicates never generated, and §17 requires it work offline.

- `invoice_series` — the **definition** (prefix template, reset policy, padding). Cloud config.
- `invoice_sequence` — the **counter**. SQLite only. No Postgres mirror, no `AggregateType`, no sync direction, ever.

Mirroring the counter would make the cloud a second writer of invoice numbers, which is precisely what §33 forbids. ADR-013 makes the outlet a single writer over one SQLite file, which is what makes a local counter concurrency-safe rather than merely convenient. The issued number travels to the cloud on the invoice; the counter that produced it stays at the outlet.

This is the `print_job` / `refresh_token` precedent applied to numbering — and it is what keeps `invoice_series` from being the half-config, half-transaction row ADR-011 forbids.

### 3. Rounding: per-invoice, per-component, half-up, then to the rupee

Tax is computed per line at full paise precision, summed per component (CGST/SGST/IGST/cess) across the invoice, and each component rounded **half-up** to paise **once**. The grand total is then rounded to the nearest rupee, with the delta recorded in `round_off_paise`.

Half-up is pinned here rather than left to an implementation: banker's rounding and half-up disagree on exactly the ₹x.x5 cases a menu produces constantly, and an accountant reconciling two systems that differ by a paise per bill has a real problem.

Per-invoice-per-component rather than per-line was chosen because line-level rounding errors accumulate, and the invoice's printed component total must equal the same rate applied to the printed taxable value — an accountant checks that by hand.

The policy is enforced in **four** places: a CHECK in `sqlite/0006`, a CHECK in `postgres/0007`, a `.refine()` on `InvoiceSchema`, and `Invoice.SumsCorrectly()` in Go. A bill that does not add up is unstorable, unreplayable and unconstructable — not merely untested.

Two §66 properties bind the implementation:
- `grand_total = Σ(components) + round_off`, and `|round_off| ≤ 50` paise.
- Across a split group: **Σ(split invoice lines) = order lines exactly** — no loss, no duplication, no double-tax — with group round-off bounded by 50 paise × `split_count`.

### 4. Split bills are N invoices, not a split entity

One order yields N invoices sharing a `split_group_id`; each `invoice_line` references its `order_item_id` with its own quantity. There is no `bill_split` table.

Each part is a real, independently numbered, independently payable GST invoice — which is what the customer physically receives. `order_item_id` on the line is what makes the conservation property checkable.

### 5. ECO fields modelled, not reported

§32 requires that direct and ECO supplies never be combined in compliance reporting. That is only possible if the classification is captured **at issue time**; it cannot be reconstructed later. So `channel`, `tax_liability_party`, `eco_operator_*` and `supply_classification` land on the invoice now, while §81's EXCLUDES keeps the reporting outputs out of M3. The exclusion is on the outputs, not the fields.

### 6. `order.display_number`

A short human-facing number (`#A184`), closing the M2 finding that a printed KOT carries the raw UUID — a cook cannot read one aloud across a kitchen. A display string, never a key (CLAUDE.md forbids exposing sequential PKs as identifiers).

**Nullable, and currently always null.** SQLite cannot add a NOT NULL column to a populated table without rebuilding `"order"`, and that table is referenced by `order_item`, `kot` and now `invoice`. More importantly: **nothing mints one yet.** The column and the type exist; the per-outlet minting logic is builder work in the M3 graph.

Until that lands the defect is *not closed* — a printed ticket still shows a UUID. The Rust round-trip test pins `display_number` as a synthesized null with a comment naming the track that must flip it, so the milestone cannot quietly report this done. The contract is a prerequisite for the fix, not the fix.

**Binding on the milestone (orchestrator decision, 2026-08-12):** the contract is deliberately *not* blocked on minting. In exchange, two conditions hold and are not negotiable at report time:

1. The minting track's verification gate must **flip `fixtures/order.json` to a real display number** and remove the synthesized-null pin in `edge/database/src/lib.rs`. A track that leaves the pin in place has not delivered.
2. **No milestone report may claim the UUID-on-KOT defect closed until that flip has happened** and a printed ticket has been observed carrying the short number. Per CLAUDE.md, acceptance is an observed behaviour, not an implemented API — a `display_number` column that no ticket renders is exactly the "wired but uncalled" shape `docs/retro.md` records twice.

### 7. `PaymentCaptureStatus` is not `PaymentStatus`

Two different things were competing for one name, and Go forced the issue with a genuine redeclaration against `order.go`. Both languages were renamed rather than only Go, so the mirror stays honest.

- `CanonicalOrder.payment_status` — the **order's** overall standing: UNPAID / PARTIALLY_PAID / PAID / REFUNDED. One value per order, derived from its tenders.
- `Payment.status` (`PaymentCaptureStatus`) — the capture lifecycle of **one tender**: PENDING / CAPTURED / FAILED / VOIDED / REFUNDED. One value per tender.

**The §34 reasoning.** §34 is explicit that `order.paymentMethod = "UPI"` is the wrong model, and requires Payment, PaymentAttempt, PaymentAllocation, Refund, Settlement and ReconciliationRecord as distinct entities. The reason is that a bill is settled by *tenders*, each with its own independent fate. §35's worked example — ₹2,000 as ₹500 cash + ₹1,000 UPI + ₹500 card — is precisely a case where the order and its tenders cannot share a status: the order sits at PARTIALLY_PAID while the cash tender is CAPTURED, the card tender FAILED on a declined swipe, and the UPI tender is still PENDING against a QR the customer has not scanned.

Collapsing the two onto one name invites collapsing them onto one field, which is the exact failure §34 was written to prevent. It would also break M7: a Razorpay webhook transitions **a tender**, not an order, and reconciliation (§37) matches settlements against individual captures. A shared status would make "which payment failed?" unanswerable while still looking correct on a fully-paid bill.

The rename is therefore not cosmetic deconfliction — it is the type system carrying §34's distinction, so a builder cannot accidentally write the model the spec forbids.

## Contract review rubric self-check

| Check | Finding |
|---|---|
| App-generated UUIDv7, no DB-side defaults | Clean — no `gen_random_uuid()` default on any new id. |
| No nullable columns in primary keys | Clean — all single-column `id`; `invoice_sequence` PK is `(series_id, period_key)`, both NOT NULL. |
| Single authority per §50.1 | Clean. The one hazard — numbering — is resolved by splitting definition from counter (§2 above). `POST /invoices` replays only; no cloud handler issues a number or cancels a bill. |
| No credential material in audit/logs/wire | Clean for this ADR. Device credentials are ADR-017's concern. |
| Tenant-scoped uniqueness | `invoice_number` unique on `(outlet_id, series_id, invoice_number)`, never global. All config codes unique per outlet. |
| Version bump + ADR | 0.3.1 → 0.4.0, this ADR. |

## Addendum — 0.4.1: `ItemQuantityChanged` (2026-08-13)

Quantity control landed in Milestone 3 with a command (`SET_ORDER_ITEM_QUANTITY`) but **no event**. The T3 verification gate found the consequence: `correct_pending_item_added_quantity` folds a correction into a still-unpublished `ItemAdded`, but once that event has published, **nothing ever corrects the cloud's `quantity` and `line_total_paise`** — no other frozen event carries a full item snapshot.

The builder disclosed this honestly and cited the `update_order_shape` precedent. **That precedent does not transfer.** An order's shape (type, table) being briefly stale in the cloud is an operational inaccuracy usually caught by context. A wrong `line_total_paise` is a *money* inaccuracy: silent, permanent, and flowing straight into revenue reporting — with M3 building tax and invoicing on exactly these fields. §53 requires financial records never be silently lost or overwritten.

So 0.4.1 freezes `ItemQuantityChanged`, carrying the corrected `OrderItem` in full plus `previous_quantity`.

**A delta-only payload was considered and rejected, explicitly on §50.1 grounds.** Sending only `(order_item_id, new_quantity)` would require the cloud to recompute `line_total_paise` from its own copy of the unit price and modifier deltas. That makes the cloud a **second computer of money the edge is authoritative for** — precisely the split-authority shape §50.1 forbids, and the same reasoning that keeps invoice numbering edge-local (§2 above). The edge computes; the cloud stores what it is told. A smaller payload is not worth reintroducing the one property this architecture is organised around.

The event is self-describing for the reason `ItemRemoved` is: the cloud must reconcile without replaying every prior event in order.

`scripts/check-event-type-drift.mjs` carries a `NOT_YET_EMITTED` entry for it naming the T3 retry as the track that must remove it. **That entry surviving past T3 is itself the defect signal.**

## Addendum — 0.4.4: the compliance config write routes enter the OpenAPI spec (2026-08-14)

The T13 track implemented the compliance config write path and mounted it, and its own report flagged that `packages/contracts/openapi/openapi.yaml` carried **no path entry for any of it** — only the response schemas the aggregates already ride in on `GET /sync/config`. The routes shipped and were tested; they were simply undocumented in the contract. 0.4.4 closes that.

**It is eight routes, not six.** The gap was recorded as "the six compliance config write routes" and carried forward that way in `docs/RESUME.md`. Reading `compliance.Handler.Mount` shows eight: the `invoice-series` create and deactivate pair was missed in the original tally. Both are now documented. The miscount is worth recording because it was a count of *undocumented* routes made by reading a summary rather than the mount — exactly the error the spec entry now prevents.

This is **additive and documentation-only**. Every request and response shape was transcribed from the handlers in `backend/internal/compliance/http.go`; no schema was added, no aggregate changed, no handler touched. All eight reference component schemas that already existed at 0.4.3 (`ComplianceVersion`, `TaxProfile`, `TaxRule`, `InvoiceSeries`, `DiscountDefinition`, `OutletFiscalProfile`).

`info.version` in the OpenAPI document was still `"0.3.1"` — it had not been advanced through the 0.4.x line at all, so the document was self-describing as three minor versions behind the package it lives in. It is now `0.4.4`, matching `package.json`.

**Two gaps are documented in the spec text rather than silently fixed**, because closing either is a semantic change needing its own decision:

- Every route is gated on `outlet.manage`, because **no `billing.manage` permission exists** in the frozen `Permission` enum. Whoever may rename a table may therefore also set the GSTIN that prints on every invoice. The spec now says so at the top of the block.
- A non-`NEVER` `reset_policy` combined with a `prefix_template` lacking the matching date token yields duplicate invoice numbers across periods. It fails loudly at issue time on the UNIQUE index, but is not validated at config-write time. The `prefix_template` description now carries that caution.

**Note on what this does and does not buy.** Nothing machine-checks the OpenAPI document against the handlers — the CI contract-drift check covers TS↔Go types only, and no test parses this file. Route/shape parity was verified here by extracting the paths from `Mount` and diffing against the parsed spec (8 = 8, both directions clean), but that was a one-time check by hand, not a standing gate. **This document can drift from the handlers tomorrow and nothing will go red.** A real generator or a spec-vs-router test is the fix; it is not in this change.

### Self-review against the rubric

| Check | Finding |
|---|---|
| App-generated UUIDv7, no DB-side defaults | N/A — no schema change. |
| No nullable columns in primary keys | N/A — no schema change. |
| Single authority per §50.1 | Clean. All eight are cloud-owned config writes, human-authenticated, never mounted under `DeviceAuthenticate` and taking no `SyncEnvelope` — the read path stays unwrapped per 0.2.1. |
| No credential material in audit/logs/wire | Clean — no route here carries a hash or token. |
| Tenant-scoped uniqueness | N/A — no new constraint. Existing per-outlet code uniqueness unchanged. |
| Version bump + ADR | 0.4.3 → 0.4.4, this addendum. Additive: paths only. |

## Addendum — 0.4.5: append-only enforced, invoices printable, HSN/SAC sourced (2026-08-15)

Three changes, all driven by defects that only became visible once something consumed the contract.

### 1. `payment` append-only is now enforced by the storage engine

`sqlite/0009_payment_append_only_triggers.sql` adds `BEFORE UPDATE` and `BEFORE DELETE` triggers on `payment` that `RAISE(ABORT)`.

Append-only was already the design (§53, and this ADR's own reasoning) and the T7c gate confirmed `repo::insert_payment` is the sole writer with no `UPDATE payment`/`DELETE FROM payment` anywhere in the workspace. But that same gate found `Db::connection()` is plain `pub`, returns `&rusqlite::Connection` whose `execute` takes `&self`, and is held by `edge/device`, `edge/printer` and `edge/sync` **in production code** — while its doc comment claimed it was not exposed beyond the crate. The guarantee rested on nobody writing a line of raw SQL the type system permits and the codebase already demonstrates elsewhere.

Narrowing that visibility would touch three sibling crates and was rejected as the wrong tool here. Triggers move the rule into the engine with no change to any sibling API.

**Scope is `payment` only, deliberately.** `cash_shift` is legitimately mutable — `close_cash_shift_in_tx` performs the OPEN→CLOSED transition, guarded by `WHERE ... AND status = 'OPEN'`. A blanket no-update trigger there would break a correct path. `payment_allocation` is insert-only in practice but was wired only at `dc1c5be` and has not had the workspace-wide audit `payment` has had; covering it is deferred rather than assumed.

### 2. `print_job` can reference an invoice

T10 built the GST invoice renderer and its gate found it had **zero callers**: `print_job.kot_id` was `NOT NULL REFERENCES kot(id)`, so no shape existed by which an invoice could become a print job. A rendered invoice could not reach a printer at all.

`sqlite/0010_print_job_invoice_ref.sql` rebuilds the table with nullable `kot_id`, a new nullable `invoice_id`, and a CHECK that **exactly one** is set. Edge-local as before — no Postgres mirror, still absent from `AggregateType` (ADR-014 §3).

**The alternative — a separate `invoice_print_job` table — was rejected.** It would duplicate the spool's queue, retry-with-backoff, attempt counting and failure surfacing, and those are exactly the parts that are hard to get right and must not diverge between two copies. A print job is a print job; what it prints does not change how it queues or retries. The cost is that `kot_id` becomes nullable, which is a change to an existing frozen column and the reason this is a table rebuild rather than an `ALTER`. **This shape was chosen by the orchestrator on a recommendation, not specified by the requester — it is the most reversible-if-wrong decision in this version, and reversing it means moving invoice jobs to their own table, not undoing a column.**

The original `UNIQUE(kot_id, printer_id)` became **partial**: SQLite treats NULLs as distinct in a UNIQUE index, so once `kot_id` was nullable the unqualified index would have permitted unlimited `(NULL, printer)` rows, silently losing idempotency for invoice jobs. Each kind now has its own guarded index.

### 3. `menu_item.hsn_sac` — the source that did not exist

`invoice_line.hsn_sac` has existed since 0006 and has been NULL on **every line ever issued**, because `assemble.rs:226` hard-coded `None` — no table in the frozen contract carried a code to read. A GST tax invoice must print the HSN (goods) or SAC (services) code per line, so this is a compliance defect, not a cosmetic gap. It surfaced only once T10 rendered an invoice and there was a blank field to notice.

**On `menu_item`, not `tax_profile`.** Both were candidates and the requester left it open. HSN/SAC classifies *what is sold*; a tax profile classifies *how it is rated*. They correlate but are not the same axis, and collapsing them loses the ordinary Indian-restaurant case: prepared food is SAC 9963 while packaged bottled water is HSN 2201, and both routinely sit under one 5% profile. Hanging the code off the profile would force every item sharing a rate to share a code.

**Nullable, and not a licence to print a blank code.** Nullable because this is additive over catalogues that have none, and because a `NOT NULL` with an invented default (`'9963'` for everything) would stamp a plausible, wrong, legally-meaningful code on packaged goods — a wrong HSN is worse than a missing one because it looks configured. The completeness rule therefore lives at **issue time**: an invoice must not issue with a line whose `hsn_sac` is NULL, and the error must name the item so a manager can fix the catalogue. **That guard, and the §66/harness assertion that no issued invoice line carries a NULL HSN/SAC, are the accompanying track's work. This migration supplies the source of truth; it does not by itself close the compliance defect, and M3 cannot claim "GST complete" until that track lands.**

`invoice_line.hsn_sac` stays nullable and stays a snapshot, written from the resolved value at issue time and never re-read — exactly as `invoice_line.description` already behaves, because §31 requires a reprint to reproduce the original document.

This is the only part of 0.4.5 that changes a wire type: `MenuItem` gains `hsn_sac` in TS, Go and the `menu_item` fixture, kept in lockstep and verified by the existing drift tests.

### Self-review against the rubric

| Check | Finding |
|---|---|
| App-generated UUIDv7, no DB-side defaults | N/A — no new identifiers. |
| No nullable columns in primary keys | Clean. `print_job`'s PK remains `id`; the newly nullable `kot_id` is not part of any key, and its partial unique index is explicitly guarded `WHERE kot_id IS NOT NULL`. |
| Single authority per §50.1 | Clean. `print_job` stays edge-local with no sync direction; `menu_item.hsn_sac` is cloud-owned config, cloud→edge, like every other `menu_item` column. |
| No credential material in audit/logs/wire | N/A — none of these carry credential material. |
| Tenant-scoped uniqueness | Clean. Both `print_job` unique indexes are scoped by a specific document plus printer, never global. |
| Additive + version bump + ADR | Items 1 and 3 additive. **Item 2 relaxes an existing NOT NULL**, which is why it is a table rebuild and is called out here rather than filed as routine. 0.4.4 → 0.4.5, this addendum. |

### Verification performed before commit

Not a claim that it should work — what was run. All eleven SQLite migrations applied in order into a fresh in-memory database, and all eleven Postgres migrations applied into a dropped-and-recreated schema. Then each new constraint was **falsified**: `UPDATE payment` and `DELETE FROM payment` rejected by name while inserting a reversal row still succeeds; `cash_shift` close still permitted, confirming the triggers do not touch it; `print_job` rejecting neither-set and both-set, accepting kot-only and invoice-only, rejecting a duplicate `(invoice, printer)` and a duplicate `(kot, printer)`, and accepting a second invoice at the same printer. TS and Go drift tests pass with the updated fixture.

## Consequences

- Builders gain the full billing surface without a further contract change; adding a payment gateway in M7 needs no shape change, only the already-modelled states.
- The four-layer rounding enforcement means a builder cannot "temporarily" store a bill that does not add up. This is deliberate friction.
- `display_number` is a known-open item with a test pinning it, not a silent gap.
- The e2e money invariant can finally see modifier price deltas and real quantities once Track B lands — until then, any tax engine is being validated against a data shape the product will not have.

## Addendum — 0.4.7: `printer_role`, and a recommendation reversed by counting first (2026-08-16)

T10 built the GST invoice renderer; its gate found it had **zero callers**. 0.4.5 gave `print_job` an `invoice_id` so an invoice could become a print job, and the enqueue path landed at `3e217ee`. But `queue_invoice_for_print` must be told *which* printer, and nothing in the frozen contract answered that: `printer` (0005) carries `name`, `connection_kind`, `address`, `paper_width_mm`, `is_active`, and nothing else. KOTs route `station` → `station_printer`; a bill has no station, so a bill had no route.

The enqueue builder was told not to invent a convention and did not — it takes `printer_id` as a caller-supplied argument and validates it. This addendum supplies the missing config so a caller can answer the question without matching on a name string, which CLAUDE.md's "no magic values" rule forbids and which breaks the first time someone renames a printer.

### The recommendation this reverses

The orchestrator recommended a `role` column on `printer` three times, on the reasoning that routing belongs on the printer and mirrors `station_printer`. **Counting the blast radius first — the rule adopted in the 2026-08-15 retro entry — reversed it.**

`printer` is constructed by struct literal in at least eight places across `edge/database` (`model.rs`, `repo.rs`), `edge/printer` (`model.rs`, `adapter.rs`, `routing.rs`) and `apps/pos/src-tauri` tests, plus the TS and Go mirrors and the fixture. Adding a field breaks every one of them at once — precisely the cascade 0.4.5 caused, where one nullable column broke five consumers and its migration was inert.

A join table adds a row shape instead of widening an existing one, so **nothing that compiles today stops compiling**. The consumer sweep confirmed it: all four edge crates, `apps/pos/src-tauri` and the backend build unchanged against this migration.

It is also the better model. One physical printer at a small outlet prints both tickets and bills; a single-valued column forces a `BOTH` member whose meaning must be special-cased at every read. Two rows say the same thing, and it matches how routing already works.

**This reversal is the rule paying for itself on its first application.** The earlier recommendation was not wrong on taste — it was made without counting, which is exactly the failure mode the rule exists to prevent.

### Shape

`printer_role(printer_id, role, config_version)`, PK `(printer_id, role)`, `role IN ('KITCHEN','BILL')`. CONFIG, cloud→edge, a child row in the config bundle like `station_printer` — not an `AggregateType`, no sync direction of its own.

`KITCHEN` does not replace `station_printer` routing; it classifies the device. A printer with **no** row here has no role and is a candidate for neither path — absence must never be read as "sure, print bills to it". An outlet with no BILL printer must fail loudly at issue time naming the problem, the way a missing HSN/SAC code does (0.4.5).

### Self-review against the rubric

| Check | Finding |
|---|---|
| App-generated UUIDv7, no DB-side defaults | N/A — no new identifiers; the PK is the natural `(printer_id, role)` pair. |
| No nullable columns in primary keys | Clean — both PK columns `NOT NULL`. |
| Single authority per §50.1 | Clean. Cloud-owned config, cloud→edge, child row in the config bundle. No sync direction of its own, exactly like `station_printer`. |
| No credential material in audit/logs/wire | N/A — carries none. |
| Tenant-scoped uniqueness | Clean. Uniqueness is `(printer_id, role)`; `printer_id` is already outlet-scoped by its own FK, so this is never global. |
| **Blast radius** (added by the 2026-08-15 retro) | **Zero existing struct widened.** Consumers enumerated before writing: `edge/database`, `edge/printer`, `edge/sync`, `edge/device`, `apps/pos/src-tauri`, `backend`, harness. All built after. |
| Additive + version bump + ADR | Additive, new table only. 0.4.6 → 0.4.7, this addendum. |

### Verification performed before commit

Not a claim — what was run. All twelve SQLite migrations applied in order into a fresh database and all twelve Postgres migrations into a dropped-and-recreated schema. Constraints falsified: an invalid role (`'LABEL'`) rejected by the CHECK, a duplicate `(printer_id, role)` rejected by the PK, and one printer holding both `KITCHEN` and `BILL` accepted. TS and Go drift tests pass with the new `printer_role.json` fixture (39 → 40 TS).

**And the runner, not the file** — the lesson of 0.4.5. `edge/database`'s registration guard, added at `92f3511`, **failed immediately** on the unregistered `0012`, naming the 0009–0011 bug in its message. Registering it turned the crate green at 176. That guard did exactly the job it was written for, on its first real test.
