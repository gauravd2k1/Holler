# Milestone 5 planning — Procurement

Written 2026-08-28 at the `m4-complete` tag. Inputs: `docs/RESUME.md`,
`docs/M5_HANDOFF.md`, `docs/spec/procurement.md`, `docs/spec/inventory.md`,
`HOLLER_MASTER_PROMPT.md` §27/§28/§81, `docs/backlog-m2.md`.

This document carries two things: the **M4 backlog triage** (§1), and the **M5
plan** (§2). §1 exists because the milestone was replanned on its evidence.

---

## 0. The replanning, and why it is recorded

M5 was first planned as **ten tracks**. It was cut to **six** on one objection:

> Triage FILES items, it does not schedule them all into the next milestone.
> Half of T7/T8/T9 is M1/M2 repair with no bearing on whether procurement works.

The test applied to every candidate track was **"does procurement work without
this?"** That test removed four tracks, and it also removed three fields from the
proposed contract version — including one the orchestrator had proposed in the
same breath as writing the rule against it (§2.2).

**One gating claim was wrong and is corrected here rather than quietly dropped.**
The business-date unification was argued as a blocker for T1/T2, on the grounds
that "procurement must not land on two disagreeing business-date functions."
That is false: `edge/database/src/deduction/ledger.rs:246` already calls
`compute_business_date`, so GRN ledger entries inherit the correct function for
free. The two defective call sites are the **invoice** business date and the
**order display number**, neither of which procurement touches. It is a real
money-adjacent defect (§1.2) and it is M3 repair, not an M5 dependency.

---

## 1. M4 backlog triage

Every open item gets a **landing milestone**, a **trigger**, or **closure**.
Nothing is left in the "we'll get to it" state that produced the eleven-instance
"a column nothing reads" class across M4.

### 1.1 `billing.manage` — an approved v0.5.0 condition that never landed

`docs/RESUME.md` §6 says "no `billing.manage` exists in the frozen `Permission`
enum." **That is false, and the truth is worse.** Measured 2026-08-28, these are
every occurrence in the repository:

```
packages/contracts/src/types/identity.ts:56     "billing.manage",
packages/contracts/go/identity.go:60            PermissionBillingManage Permission = "billing.manage"
packages/contracts/src/types/drift.test.ts:586  expect(PermissionSchema.options).toContain("billing.manage")
packages/contracts/go/drift_test.go:622         PermissionBillingManage,
```

The permission is **defined, mirrored across both languages, and drift-tested —
and checked by nothing.** `backend/internal/compliance/http.go` still gates all
nine compliance config write routes on `PermissionOutletManage`, and
`service.go:26` reads `const permConfigManage = auth.PermissionOutletManage`.

The v0.5.0 approval condition, in the contract's own comment at
`identity.ts:50-56`, was: *"Rides along, and lands **WITH** its enforced check on
the GSTIN write path... A permission defined and never checked is a documented
obligation dressed as structural enforcement."* The enum member landed. The
check did not. **The comment predicting the exact failure shipped alongside the
failure.**

Two things made it invisible:

1. **The drift suites are green because they only assert the member exists.**
   Presence in an enum is not enforcement, and nothing distinguishes them.
2. **`service.go:19` carries a stale comment asserting no such permission exists
   in the frozen enum.** That comment is why RESUME.md is wrong too — the error
   propagated from code comment to resume doc and was never re-measured.

This is the `menu_item_variant.is_default` shape one layer out: **a permission
nothing checks is a permission that does not exist.** Whoever may rename a table
may still set the GSTIN printed on every invoice.

> **Landing: M5, inside T1.** Not deferrable — it was an approval condition, and
> the fix is a constant plus a test. Folded into T1 rather than given its own
> track: same builder, same files, and T1 must define and enforce
> `procurement.manage`/`procurement.approve` in the same package, so it needs to
> be shown the defective pattern next door explicitly or it will copy it.
>
> The `check-contract-field-consumers.mjs` widening (§1.4) must grow to cover
> **enum and permission members**, or this class stays invisible after the fix.

### 1.2 Invoice business date — money-adjacent, not the cosmetic item it was filed as

Filed in RESUME.md §6 as a display-number nicety. It is not:

```rust
// apps/pos/src-tauri/src/commands/billing.rs:71
fn business_date_from(instant_iso: &str) -> String {
    instant_iso.get(0..10).unwrap_or(instant_iso).to_string()
}
```

That value becomes `invoice.business_date`, which drives the **invoice numbering
reset policy** and every financial day report. For an IST outlet (UTC+05:30),
dinner service running to 01:00 IST is 19:30 UTC — so **one trading night splits
across two `business_date` values**, and under a `DAILY` `reset_policy` the
invoice series resets in the middle of service. CLAUDE.md is explicit:
*"Business day may cross midnight."*

The modelling blocker is gone. `compute_business_date(occurred_at, &timezone,
&day_start_time)` exists at `edge/database/src/deduction/business_date.rs`, is
validated, is fed by `outlet.day_start_time` (contracts 0.5.0), and is already
called by the stock ledger. Both defective call sites — `business_date_from` and
`repo.rs`'s display-number bucket — are one function swap away.

Note the comment at `billing.rs:67`, which discloses the defect as "the same
known limitation... not a new one introduced here." **That disclosure is what
kept it open**: it converted a defect into a documented property.

> **Landing: M6.** Not an M5 dependency (see §0). Requires a test whose fixture
> is an outlet at UTC+05:30 with a post-midnight invoice — every current fixture
> is structurally incapable of failing on this.

### 1.3 The four M1/M2 POS ordering defects

Found by twenty minutes of driving the shipped POS by hand on 2026-08-27. Every
suite in the repo passes over all four.

| Defect | Verdict |
|---|---|
| DINE_IN accepts an order with **no table selected** | **M6.** Decided 2026-08-28: an explicit named **"Counter / no table"** option, *not* a hard table requirement — restaurants genuinely sell across a counter, and a forced field gets a fake table typed into it. Silently accepting an empty selector remains the one unacceptable outcome. No contract change: `table_session` already exists for this. |
| Cart does not clear after send; per-item `-`/`+`/Remove ignore the non-amendable state; error is developer text | **M6.** Three bugs in one entry. The refusal itself is correct and stays — `SENT_TO_KITCHEN` must not be silently amendable. §64 binds the error text. |
| **"Beverages" appears twice** | **M6, and it is a harness fix, not a menu fix.** Both rows are real and deliberately seeded, which is why no test caught it. `tests/e2e-scenario/harness` pins the legacy fixture category by exact id, price and routing, so merging them breaks the harness. Scope the harness to ids first, then rename the legacy category. |
| **"Kitchen Prep (internal — not sold)" is orderable** | **M6, with its contract field.** A modelling gap: nothing in the frozen contract marks anything non-sellable, and `recipe.menu_item_variant_id` is NOT NULL (migration 0015), so every pure sub-recipe *must* bind to a sellable menu item. Filtering the one known id is a patch that breaks the moment a second internal category exists. See §2.2 for why `is_sellable` was **dropped from v0.6.0**. |

### 1.4 The unused-contract-field check — order-dependent, and it ships inert if reordered

`scripts/check-contract-field-consumers.mjs` covers **fields**. It should cover
**enum members** too. `docs/M5_HANDOFF.md` §2.2's ordering is carried verbatim
because the failure mode is silent:

1. **First** exclude doc comments and `#[cfg(test)]` modules from the consumer
   corpus. Measured 2026-08-28: the six unwritten
   `stock_ledger_entry.entry_type` values appear in the consumer roots **only**
   in a doc comment enumerating the CHECK constraint
   (`edge/database/src/model.rs:1248-1250`), plus `"PURCHASE"` once in a test
   fixture (`edge/database/src/stock/variance.rs:150`). Extend the check before
   narrowing the corpus and **all six report green on day one** — a comment
   listing permitted values is indistinguishable from a branch acting on one,
   under a grep.
2. **Then** extend to enum members — **and to `Permission` members**, per §1.1.
3. **Then** declare exemptions with the milestone named: `PURCHASE`,
   `TRANSFER_IN`, `TRANSFER_OUT`, `RETURN_TO_VENDOR` → M5/M8; the two
   `PRODUCTION_*` values → M8 (central kitchen).

The follow-up the script's own header files — the **per-surface** check, where a
field must appear in at least one file that is not a model, DTO or fixture — is
what would actually have caught the five instances it was written for. It is
substantially bigger than a widening.

> **Landing: steps 1–3 → M6. The per-surface check → M6+, with a trigger: the
> next instance of this class found by hand.** The class stands at **eleven**
> across M4, up from the five the script was written against. A twelfth found by
> a human is the signal that the floor is not enough.
>
> **Not deferred, because it is an M5 obligation:** when T2 consumes
> `unit_cost_paise` and `yield_factor_ppm`, it **removes those two entries from
> `EXEMPT`** in the same commit. That is a two-line change inside T2, never a
> track. *An exemption that outlives its reason is a silenced failure.*

### 1.5 Seeded reorder levels — reclassified, not a code task

28 of 32 seeded items read LOW, so M4 criterion 4's surface is correct and the
data behind it is noise. There is no code defect: `devseed.rs` needs plausible
restaurant pars.

The real finding is that this is **the fourth instance of one thing**, currently
filed in four different places. An outlet cannot operate until *all* of these are
configured, and each fails correctly and loudly by design:

- `menu_item.hsn_sac` is NULL on every row of every existing edge database, and
  the edge **refuses invoice issuance** when any line's code is NULL or blank.
  Correct and deliberate — no fallback, because a wrong code that looks
  configured is worse than a missing one.
- An outlet with **no `BILL`-role printer** cannot print a bill; `print_invoice`
  fails loudly by name rather than queueing into nothing (contracts 0.4.7).
- Tax profile, fiscal profile and invoice series must exist before any bill.
- Reorder levels must be real, or the low-stock banner trains people to ignore it
  the first time they look.

> **Landing: M5, T0 — as one document, not four backlog entries.** An "outlet
> go-live configuration checklist" written by the orchestrator alongside this
> plan. Setting real reorder pars in `devseed.rs` rides with T4, which is the
> track that will be looking at that screen anyway.

### 1.6 Everything else open in `docs/RESUME.md` §6

| Item | Verdict |
|---|---|
| `backend/internal/{auth,menu,tables}` never call `postgres.Migrate`; CI works around it with a `devseed` step | **M5 for the new context only** — T1 does it correctly for `procurement`. Fixing the other three → **M6**. |
| **OpenAPI machine-checked against nothing**; drift check is TS↔Go only; drifted silently on three `MenuItem` fields for two versions | **M6, trigger: the next OpenAPI drift found by hand.** M5 adds ~8 shapes to that file, which raises the odds. **This is the item the orchestrator is least comfortable deferring** — recorded so the discomfort survives the milestone boundary. |
| `Db::connection()` is plain `pub`, held by three sibling crates; `cash_shift` not trigger-protected | **Partly closed as unfixable:** no append-only trigger can exist on `cash_shift`, because OPEN→CLOSED is a legitimate UPDATE. Narrowing `Db::connection()` to `pub(crate)` with explicit accessors → **M6**. |
| `cashShift.ts` recovery runs on `BillingScreen` mount, not app-global startup | **M6** — same file cluster as §1.3. |
| `payment_allocation` assumes one payment settles at most one invoice | **M7 (Payments), trigger: the first real split-tender-across-invoices requirement.** Genuinely unmodelled. |
| `PAID_IN`/`PAID_OUT` emit no outbox event; they ride inside `CashShiftOpened`/`Closed` | **M7.** Visibility latency, not a money defect. |
| `edge/sync/src/config.rs`: empty `device_credentials` is not an error, unlike empty `users` — "none enrolled" and "cloud forgot" are indistinguishable | **M6.** A silent-outage shape this repo has been bitten by three times; it is not procurement. |
| `{OUTLET}` invoice token derives from `outlet.name`; no `outlet.code` in the contract — a rename changes invoice numbers | **M6, with its consumer.** See §2.2 for why the field was **dropped from v0.6.0**. |
| A non-`NEVER` `reset_policy` whose prefix lacks a matching date token yields duplicate invoice numbers across periods; caught by the UNIQUE index, not validated at config-write time | **M6**, with §1.2 — same surface, same fixture work. |
| `cargo fmt --check` diffs in `apps/pos/src-tauri/tests` and the two bridges under `tests/`; not CI-enforced (crate is not built on Linux, ADR-013) | **M6.** |
| e2e HSN check folded into `9_tax_reconciliation`, so a tax arithmetic error and a missing compliance field share one invariant id | **M6.** One invariant id for two failures means one of them can never be seen. |
| **No POS dev-server smoke test** | **M6. The constraint is load-bearing and must be carried with the item:** it must drive `pnpm dev`, **not** `pnpm build`. `optimizeDeps` prebundling is a dev-server-only mechanism that `vite build` never reads, so a build-based smoke test — including the headless-Chromium one used to *diagnose* the incident — cannot catch this class **at all**. |
| `seed_offline_sale.rs` proves the resolver, not the caller | **M6.** Product fixed at `7e88d1c`; the test that drives the resolution the ordering screen actually performs still does not exist. |
| Nothing machine-checks the Tauri DTOs (`apps/pos/src-tauri/src/dto.rs`) against the Zod schemas | **M6.** The same three `MenuItem` fields drifted twice, one layer apart (0.4.6 OpenAPI, then the DTO). M5 adds a receiving DTO, so the exposure grows. |
| `PosScreen.tsx`: a **failed** menu query renders "Loading menu…" forever — `isError` is never surfaced | **M6.** This is *why* the DTO drift went unnoticed: a rejection is indistinguishable from a slow load. |
| `cancel_kitchen_items_with_outbox` has no Tauri command; `#132-C` cancellation unreachable from the shipped surface | **M6.** Real gap, kitchen context. |
| Postgres integration tests never clean up their rows; the database grows without bound across CI runs | **M6, trigger: CI runtime or storage becomes a problem.** Harmless today — ids are minted per run since `1cc087c`. |
| `make test` covers only Go; the Rust and TS suites are run directly | **M6.** Either extend the target or stop calling it the project test command — it is quoted as the integration gate in the milestone workflow itself. |
| Six permitted-but-unwritten `entry_type` values | **Three → M5** (`PURCHASE`, `RETURN_TO_VENDOR`, `TRANSFER_OUT` written this milestone). **`TRANSFER_IN` → M8** with the destination receipt. **Two `PRODUCTION_*` → M8.** Mechanism in §1.4. |
| `gh` installed but not authenticated (§2a) | **Cleared 2026-08-28** — `gh auth login` run. A push whose CI verdict nobody read is not a push. |
| `m4-complete` tag is local, not pushed (§2b) | **M5, T0** — pushed with the contracts commit. |
| Two ADR-013 hardware gates (§4) | **PARKED, unchanged.** They block **M3** acceptance, not M5. Revisit ~2 September 2026. Not re-litigated. |
| CLAUDE.md does not record the period M2 item 5 was falsely green (§5) | **Closed, T0** — now in the Completed-milestones note. |
| Batch/expiry alerting | **M6.** Deferred a second time, deliberately: it depends on GRN existing, it is not procurement, and M5 was over-scoped. |

---

## 2. The M5 plan

### 2.1 Scope conflict, resolved

`HOLLER_MASTER_PROMPT.md` §81 lists **central kitchen** under M5. M4's EXCLUDES
list and `docs/M5_HANDOFF.md` §4 both put it in **M8**, and the handoff assigns
`PRODUCTION_CONSUMPTION`/`PRODUCTION_OUTPUT` to M8 explicitly.

**Decided 2026-08-28: central kitchen → M8.** It is a production engine plus
goods-in-transit across outlets, it depends on multi-outlet, and it would roughly
double the milestone. The handoff is the more recent decision.

**Inter-outlet transfer, decided 2026-08-28: outbound half only in M5.** A
transfer spans *two* edge databases with goods in transit between them, which is
multi-outlet machinery. M5 ships `TRANSFER_OUT` plus a cloud-held in-transit
record; **`TRANSFER_IN` destination receipt and goods-in-transit → M8.**

### 2.2 Contracts v0.6.0 — and the three fields the scope cut removed

Approved in shape 2026-08-28; the diff takes a second approval before anything
touches `packages/contracts/`.

**Three fields were proposed and then dropped, by applying rule 6 below to the
proposal itself:**

- **`menu_item.is_sellable` — dropped.** Its only consumer is the Kitchen Prep
  POS fix (§1.3), now M6. Landing the field without its consumer is `is_default`
  and `source_stock_count_id` again — proposed in the same breath as writing the
  rule against it. It lands in a later version **with** the filter that reads it.
- **`outlet.code` — dropped.** Its consumer is the invoice `{OUTLET}` token, M6
  repair.
- **`wastage.approve` — dropped**, against the v0.5.0 comment that assigned it to
  M5. Its enforcing append-only approval row is not in procurement scope, and
  adding a permission with no check is **verbatim the `billing.manage` defect in
  §1.1**. It moves to M6 with the row that enforces it.

What remains is procurement shapes plus two permissions, **both enforced by T1 in
this milestone**.

**Authority split**, on the line ADR-009/011/014/016/018 already draw:

- **Config, cloud→edge:** `supplier`, `supplier_item` (purchase unit, pack size,
  last price), `purchase_order`, `purchase_order_line`. Purchasing policy and
  approval limits are management decisions.
- **Edge-authoritative, edge→cloud:** `goods_receipt_note`, `purchase_return`,
  `stock_transfer_out`. **GRN is edge-capable**: the outlet receives goods with
  the uplink down and the cloud replays. The `invoice`/`payment`/`cash_shift`
  split exactly.
- **Child rows, no sync direction, travelling inside their parent:**
  `purchase_order_line`, `grn_line`, `purchase_return_line`,
  `stock_transfer_line`.
- **Cloud-only, deliberately not an `AggregateType`** (the `refresh_token` /
  `device_credential` precedent): `supplier_invoice`, `supplier_credit`. Fields
  modelled; accounts posting is M7.
- **Permissions:** `procurement.manage`, `procurement.approve`.

**Rules for ADR-019, each mirroring a precedent this repo has already paid for:**

1. **A GRN posts its ledger entries inside its own transaction** — the
   `confirm_order`/`deduct_stock_for_confirmed_order` precedent. Receiving 50kg
   and crashing must not leave a GRN with no stock.
2. **A GRN NEVER BLOCKS ON A PO.** *(Named by the approver as the load-bearing
   rule of this version.)* Goods arrive against a PO that never synced, against a
   PO amended after dispatch, and with no PO at all. Each case records a gap and
   **accepts the receipt** — the M4 rules "stock never blocks a sale" and "a
   missing or broken recipe never fails a confirm", generalised to the inbound
   side. Refusing a delivery standing in the kitchen doorway because a row is
   missing is the outage, not the protection. Acceptance criterion 3 exists to
   observe this against the shipping binary.
3. **The GRN quantity is in the supplier's purchase unit and converts exactly
   once, at the edge**, through `item_unit_conversion` — the money=paise /
   quantities=integer-micro-units discipline. Never reconvert in TypeScript or
   the Tauri layer; those layers format what the edge returns.
4. **`grn_sequence` is edge-local** — SQLite only, no Postgres mirror, no
   `AggregateType`, no sync direction, ever. The `invoice_sequence` precedent: the
   issued number travels on the GRN, the counter that produced it never leaves
   the outlet.
5. **Cost lands on the ledger entry, never on `inventory_item`.**
   `unit_cost_paise` is finally consumed and weighted average is derived from the
   ledger. A cost column on a cloud-owned config row is the half-config,
   half-transaction row ADR-011 forbids.
6. **Every new field has a named consumer before v0.6.0 closes**, and the
   consumer list reaches the Go struct, the Zod schema, the OpenAPI shape and the
   repository's INSERT *and* SELECT — the 0.5.9 lesson. This rule is what removed
   the three fields above.

### 2.3 Track graph — 6 tracks

```
T0  contracts v0.6.0 + ADR-019 + CLAUDE.md block + this doc
    + go-live config checklist + push m4-complete    [orchestrator, serialized]
     │
     ├──────────────┬──────────────┐
    T1            T2               │      (parallel, concurrency cap 3)
  backend/       edge/database/    │
  internal/      procurement/      │
  procurement    GRN → PURCHASE    │
  supplier, PO,  ledger in one tx, │
  approval       purchase-unit     │
  limits, config conversion,       │
  push, envelope yield_factor_ppm, │
  ingest, cross- weighted avg cost,│
  tenant tests   EXEMPT removals   │
  + billing.     [rust-edge]       │
    manage                         │
  [go-builder]   └────────┬────────┘
     └──────────┬─────────┘
                │
               T3  edge/sync — GRN / return / transfer_out replay streams,
                   cursors, per-entry retry budget    [rust-edge-builder]
                │
       ┌────────┴────────┐
      T4               T5
    POS receiving      admin: supplier management,
    + returns          PO raise / approve
    entryIntentEcho    [pos-builder]
    [pos-builder]
       └────────┬────────┘
                │
               T6  e2e invariants + acceptance for the new shapes
                   [pos-builder]
```

**T4 must use `entryIntentEcho`** (`apps/pos/src/domain/inventory.ts`).
`docs/M5_HANDOFF.md` §2.1: receiving is the **third** quantity-entry path and the
same 1000x trap with worse odds — larger quantities than a count, read off a
delivery note written in the *supplier's* units, entered by someone reconciling
against a document rather than counting a shelf. A receipt is a **movement**, so
it takes no qualifier: `Receiving 5,000 millilitres of Sunflower Oil`. A label
alone is not the fix (`docs/retro.md` 2026-08-28).

### 2.4 Acceptance criteria

Verbatim in CLAUDE.md's milestone block. Seven items, every one an observed
behaviour against the shipping binaries, **none evidenced by a test harness**.
Criterion 3 observes rule 2 above; criterion 6 carries the 0.5.9 fixture lesson
(**every provenance group needs its own populated row**, or the check is green on
absent data).

### 2.5 EXCLUDES

Verbatim in CLAUDE.md's milestone block, plus the M1–M4 repair backlog triaged in
§1 of this document.
