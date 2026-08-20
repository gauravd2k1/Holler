# ADR-018 — Milestone 4 inventory and recipe contracts (v0.5.0)

**Status:** Accepted (2026-08-20, both open decisions ruled on)
**Date:** 2026-08-20
**Supersedes:** nothing. **Extends:** ADR-008 (contracts-first), ADR-009/§50.1 (authority split), ADR-011, ADR-014, ADR-016, ADR-017.

## Context

Milestone 4 delivers raw materials, units, recipes, sub-recipes, the stock ledger, automatic consumption, wastage, stock counts and variance (§81). Contracts are frozen at 0.4.7 and carry **none** of these shapes — no inventory, recipe, supplier or stock type exists in SQLite, PostgreSQL, TypeScript, Go or OpenAPI. `backend/internal/inventory` is an empty directory. This is the largest additive contract change since 0.4.0, hence a minor bump to **v0.5.0** rather than another 0.4.x amendment.

Two properties of this milestone raise the cost of a modelling mistake above M3's:

**Volume.** A ledger entry is written per ingredient per line per sale. At roughly 8 lines per order and 6 ingredients per line, a 300-order day is ~15,000 rows, ~5M rows a year, on the ADR-013 target: bare Windows 10, 4GB RAM, spinning disk. M3's `invoice` table grows at one row per bill. This one grows two orders of magnitude faster, and a column added later must be backfilled across every one of those rows.

**Irreversibility.** The ledger is append-only and immutable by design (§stock ledger, `docs/domain/INVENTORY_MODEL.md`). A deduction recorded wrongly cannot be edited away; it can only be corrected by an appended adjustment, which is itself a permanent record. Getting the *shape* wrong is therefore not a refactor — it is a permanent artefact in the accounting history.

M4 is also the first milestone whose core write path runs **inside an existing transaction that must not fail**: deduction happens within `confirm_order`. Anything that can loop, block or error there can wedge a POS mid-service.

---

## Decisions

### 1. Authority follows §50.1 with no new exceptions

| Shape | Direction | Why |
|---|---|---|
| `inventory_item`, `recipe` | CLOUD_TO_EDGE (aggregates) | A raw material's definition and a recipe are management decisions, like `menu_item` and `tax_profile`. |
| `stock_ledger_entry`, `stock_count`, `stock_deduction_gap` | EDGE_TO_CLOUD (aggregates) | The outlet consumes, wastes and counts stock with the uplink down. The cloud only replays. |
| `item_unit_conversion`, `recipe_ingredient`, `modifier_ingredient_delta`, `stock_count_line` | **not aggregates** — child rows | They travel inside their parent's payload or config bundle: the `menu_item_variant` / `station_printer` / `invoice_line` precedent. No sync direction, ever. |
| `stock_balance_snapshot` | **edge-local** — SQLite only | No Postgres mirror, no `AggregateType`, no direction. The `invoice_sequence` / `print_job` / `refresh_token` precedent. See §9. |

`modifier_ingredient_delta` is a child of `menu_item_modifier`, which is itself a child of `menu_item`. It rides in the `MenuItem` config payload and needs no route of its own.

**The rule this table enforces, stated because it is the easiest one to break:** *current stock is never a column on `inventory_item`.* A quantity written by the edge on a row owned by the cloud is exactly the half-config, half-transaction row ADR-011 forbids, and it would silently become the second writer §50.1 exists to prevent. The same applies to `weighted_average_cost` and `last_purchase_price` from `docs/spec/inventory.md`: those are derived from purchases the *edge* records, so they do not belong on the config row either. Cost lives on the ledger entry (§8).

### 2. One recipe per sellable unit. `recipe.menu_item_variant_id` is NOT NULL.

A recipe binds at the same grain as a price.

The nullable form — null meaning "applies to all variants" — is **rejected on a structural defect, not a preference**: `NULL != NULL` in both PostgreSQL and SQLite, so a unique index over a nullable `menu_item_variant_id` does not prevent two "all variants" rows for the same item. Making it safe needs two indexes (one partial), plus an exact-match-else-null fallback rule duplicated in edge Rust and cloud Go, plus a precedence rule for when both exist. Three drift surfaces against one index and no rule.

- `recipe` is unique on `(tenant_id, menu_item_id, menu_item_variant_id)`, all NOT NULL. One index. No fallback branch exists anywhere in the resolution path.
- `modifier_ingredient_delta` keys on `menu_item_modifier_id` and carries a **signed** `quantity_micro` — "Extra Paneer" is positive, "No Onion" is negative.
- **A modifier with no delta row deducts nothing.** Absence is never read as consent — 0.4.7's `printer_role` rule applied to ingredients.

**Authoring duplication is a UI problem and stays one.** Four variants means four recipes; the authoring surface offers copy-recipe-from-variant. It is never mitigated in the data layer, because an authoring convenience that becomes a resolution rule in the deduction path is exactly the fallback branch this decision exists to avoid.

#### 2.1 Default variant — the prerequisite this created

NOT NULL only holds if every sellable menu item resolves to a variant row. **It does not today.** `order_item.variant_id` is nullable (`packages/contracts/sqlite/0001_init.sql:88`), carried as `Option<String>` in `edge/database/src/model.rs:183,196` and `apps/pos/src-tauri/src/dto.rs:250`, and the dev seed menu has items with no variant at all (Samosa, Pani Puri, breads).

The fix is a **default variant in the menu model**, not a nullable FK:

1. `menu_item_variant.is_default` (additive), with a partial unique index enforcing **at most one default per menu item**.
2. Every menu item has at least one variant. An item authored with none gets an auto-created `Regular` at `price_delta_paise = 0`, so pricing is unchanged.
3. `add_order_item` **stamps** `variant_id = chosen ?? item's default` at line creation. Deduction therefore never sees a null and needs no fallback.

This keeps 0.5.0 fully additive. `order_item.variant_id` **stays nullable in the contract** and historical rows keep their NULLs: no backfill, because no deduction was ever computed against a pre-M4 sale. Nothing that compiles today stops compiling.

**The one soft spot, stated rather than hidden:** "every menu item has at least one variant" is a cross-row invariant that no DB constraint can express. It is enforced at the cloud menu write path, in `devseed`, and by a CI assertion. Rule 2 is the safety net — a line that somehow reaches deduction with a null variant records a deduction gap and completes the sale, rather than failing a confirm on a config defect.

The POS must stamp silently: a cashier is never made to choose "Regular".

### 3. Quantities are integers in micro-units. No float in the quantity path.

The money-is-paise rule, generalized — with **one scaling rule instead of a per-dimension choice**. The base is the canonical unit of the dimension, scaled by 10⁶, and the scale is carried in the field name:

| Dimension | Canonical unit | Stored as |
|---|---|---|
| MASS | gram | `quantity_micro` — micro-grams |
| VOLUME | litre | `quantity_micro` — micro-litres |
| COUNT | piece | `quantity_micro` — micro-pieces |

Every stored quantity — `recipe_ingredient.quantity_micro`, `modifier_ingredient_delta.quantity_micro`, `stock_ledger_entry.quantity_micro`, `stock_count_line.counted_quantity_micro`, `stock_balance_snapshot.closing_quantity_micro` — is an integer count of micro-units of its dimension. No float, no decimal, no numeric type anywhere in the quantity path, in any of the four languages. Display units are presentation-only, converted at the outermost edge of the UI.

An earlier draft of this ADR used mg / ml / milli-piece and accepted a 1 ml precision floor. That was revised: the mitigation the draft offered — recording an essence by mass, or amortising it across a prep batch — **does not exist in M4**, because prep-batch amortisation needs `semi_finished_batch`, which §7 of this same ADR defers to M5. A workaround a chef must perform consistently, using a table that does not ship, is not a mitigation. Micro-units remove the floor entirely and put 0.5 piece on exactly the same footing as 0.5 ml.

**The binding range constraint is JavaScript, not `i64`.** TypeScript and Zod carry these values as `number`, so the real ceiling is `Number.MAX_SAFE_INTEGER` (2⁵³ ≈ 9.0 × 10¹⁵), which binds long before `i64`'s 9.2 × 10¹⁸. A 50 kg sack is 5 × 10¹⁰ micro-grams — five orders of magnitude of headroom. Intermediates are `i128` in Rust (§5) and never cross the wire, so the safe-integer limit applies only to stored values, which stay far inside it.

### 4. Conversions are integer ratios, in two tiers

**Tier 1 — dimensional, global, frozen in code.** kg→g→mg, l→ml, dozen→piece. These are physical constants, not configuration. They ship as a constant map in `packages/contracts`, not as a table: no config write path, no sync, no drift, nothing to get wrong per tenant.

**Tier 2 — pack conversions, item-scoped.** `item_unit_conversion { inventory_item_id, pack_unit_label, numerator, denominator }`, a child row of `inventory_item`. "1 packet paneer = 200 g" is a property of *that* paneer, not of packets. Two outlets, or two suppliers, may disagree; a global packet size would be wrong for one of them.

Both tiers are `numerator`/`denominator` **integer** pairs. A conversion is a rational multiplication, never a decimal factor.

### 5. Rounding: exact rational resolution, half-up, applied once at the leaf

Sub-recipe resolution accumulates as an exact rational (`i128` numerator and denominator) through the whole tree. **Rounding happens exactly once**, when the leaf ingredient's applied quantity is written to its ledger entry:

```
applied_micro = round_half_away_from_zero( recipe_qty × line_qty × pack_ratio × … )
```

Implemented on integers only: for a non-negative rational `n/d`, `applied = (2n + d) / (2d)` under truncating division; for a negative rational, the sign is taken out first and reapplied, so the rule is **half away from zero** and a "No Onion" delta of −0.5 micro-units rounds to −1, not to 0. Half-up (never banker's) matches the M3 tax decision, and the direction of the tie is the same one an accountant reconciling two systems expects.

Rounding once at the leaf, rather than at each level of the sub-recipe tree, is the same reasoning ADR-016 §3 used for per-invoice-per-component tax: intermediate rounding accumulates, and a three-level sub-recipe would drift measurably across a service.

**Edge and cloud compute byte-identical results** because both compute over `i128` integers with the same tie rule and no floating point exists on the path. There is nothing platform-dependent left to differ.

### 6. The ledger is self-describing. Recipe reference is provenance, not a join.

`stock_ledger_entry` stores **the quantity actually applied** as the authoritative value, and snapshots enough context to be read without any other table:

- `inventory_item_id` + `inventory_item_name` + `dimension` (MASS | VOLUME | COUNT) — snapshotted, **no FK**
- `recipe_id` + `recipe_version` + `recipe_name` — provenance, **no FK**
- `source_order_id` + `source_order_item_id` — provenance, **no FK**
- `origin` — `RECIPE` | `MODIFIER_DELTA` | `MANUAL` | `COUNT_ADJUSTMENT` | `WASTAGE`
- `entry_type` — `PURCHASE` | `CONSUMPTION` | `WASTAGE` | `TRANSFER_IN` | `TRANSFER_OUT` | `ADJUSTMENT` | `RETURN_TO_VENDOR` | `PRODUCTION_CONSUMPTION` | `PRODUCTION_OUTPUT`
- `reason_code`, `note`, `occurred_at`, `business_date`, `created_by_user_id`

The `order_item_modifier` precedent: snapshot the values, do not point at a live catalogue row. Consequences, all intended:

- **A recipe edit never retro-alters a past deduction.** The old entry keeps the old applied quantity and the old version number. `recipe_version` increments cloud-side on every edit.
- **Deleting a recipe orphans nothing.** There is no FK to violate.
- **An auditor can read a year of ledger without the recipe table**, which matters because the recipe table is config and will have been overwritten by sync many times over.

`recipe_version` is an integer on the recipe, not a separate version table. M4 does not ship a recipe-history feature; it ships the number that makes a past deduction interpretable.

### 7. Sub-recipes: bounded at both ends

- **At cloud write time:** a DFS cycle check plus `MAX_RECIPE_DEPTH = 8`. A recipe that cycles or exceeds depth is rejected at the write, with the offending path named in the error.
- **At edge resolution:** a defensive visited-set and depth counter, independent of the cloud check. It exists because config arrives over a wire from a service that may be older than this rule, and because the cost of being wrong is a wedged POS rather than a bad row.

The defensive check **degrades to a deduction gap, never to a failed confirm** (Rule 2). A cycle that reaches an outlet loses that item's stock accuracy for that sale and reports itself; it does not stop the restaurant trading.

Sub-recipes resolve **transitively at deduction time**. A semi-finished item is expanded to its leaves unless it is itself stocked. `semi_finished_batch` — physical batch production with expected-vs-actual yield — is **deferred to M5**; §81's M4 list does not include it, and it needs the procurement side to be meaningful.

### 8. Deferred columns land now, unused, pinned by exact assertion

`yield_factor_ppm` (parts per million: 92.5% = `925000`) and `unit_cost_paise` land in 0.5.0 as real columns:

| Column | Where | Landing |
|---|---|---|
| `yield_factor_ppm` | `recipe_ingredient`, `inventory_item` | M5 |
| `unit_cost_paise` | `stock_ledger_entry` | M5 |

They are written as a fixed placeholder in M4 and pinned by **exact assertion** in the round-trip tests — the synthesized-canonical-field precedent at `edge/database/src/lib.rs:4026`, where a deferred field's placeholder is asserted equal to its exact value so that the day it starts carrying data, the test fails and says so.

Cost belongs on the ledger entry rather than on `inventory_item` for the §1 reason: a weighted average cost is derived from edge-recorded purchases, so on the config row it would be a split-authority column.

This is a volume decision, not a tidiness one. Adding a column to a 5M-row SQLite table on a spinning disk, at an outlet, during a version upgrade, is not an operation this product should ever need to perform.

### 9. Retention: sealed snapshots, structural archival, no deletion in M4

`stock_balance_snapshot { outlet_id, inventory_item_id, business_date, closing_quantity_micro, dimension, last_entry_id, sealed_at }`, primary key `(outlet_id, inventory_item_id, business_date)` — three NOT NULL columns.

Current stock = **latest sealed snapshot + entries since**. A stock read is therefore bounded to one business day's entries forever, regardless of how large the ledger grows.

**There is no materialized current-stock table.** An earlier draft proposed one; the snapshot makes it redundant, and removing it removes an entire class of projection-drift defect. Current stock is a bounded query, not a row someone must remember to update.

**Archival eligibility is structural, not time-based.** A `stock_ledger_entry` may be archived only when both hold:
1. its outbox replay is **acked by the cloud**, and
2. a **sealed snapshot covers its business date** for its item.

M4 computes and reports eligibility. **M4 deletes nothing.** Whether to delete, and at what threshold, is decided later against a measured row count and read latency from the 4GB box — which T0 is the first opportunity to produce.

#### 9.1 Sealing is idempotent and lazily caught up. It never depends on an operator.

The bounded-read guarantee holds only while days actually get sealed. An outlet that skips day-end close for a month, or a POS that dies at 11pm, would silently degrade every stock read to a full-ledger scan — and the degradation is invisible until the box is slow. **A guarantee that depends on a human performing a daily action is not a guarantee.** That is the ADR-013 lesson restated: design intent is not verified fact.

So sealing is not an effect of day-end close. It is:

- **Idempotent** — sealing an already-sealed day is a no-op, not an error or a second row.
- **Lazily caught up** — on database open, every unsealed prior business day is sealed, in order, **before the first stock read is served**. Day-end close may trigger it; nothing depends on day-end close having happened.

**T6 invariant:** skip three business days, reopen, assert three snapshots exist and that the resulting balance equals a full-ledger sum. Like every §66 invariant, it is deliberately broken and watched to fail before it is trusted.

#### 9.2 The business-date definition is a pre-track decision, and its current form is an M3 defect

The snapshot keys on `business_date`, which is a 0.5.0 column, and its definition — computed from an **outlet-configured day-start time**, which is cloud config on the outlet — is therefore a schema-level decision. It is settled in the **pre-track**, not in an implementation track: settling it after this ADR freezes the column would be backwards.

**The definition, settled here.** `outlet.day_start_time` — `TEXT NOT NULL DEFAULT '00:00'`, local `HH:MM`, cloud config travelling cloud→edge with the rest of the outlet row. Then:

```
business_date(instant_utc, outlet)
    = date_part( (instant_utc → outlet.timezone) − outlet.day_start_time )
```

An outlet with `day_start_time = '04:00'` books a 01:30 sale to the previous date; the default `'00:00'` is the plain outlet-local date, which is already correct for any outlet that closes before midnight. `outlet.timezone` has existed since `sqlite/0001_init.sql:13` — the data needed to do this correctly has been on the row since Milestone 0.5.

Two rules bind it:

- **Both sides resolve IANA identifiers**, Rust via `chrono-tz` and Go via `time.LoadLocation`, so an offset is never hard-coded and a zone with DST behaves correctly even though `Asia/Kolkata` has none.
- **`business_date` is computed once, at write time, at the edge, and stored.** It is never recomputed on read. Changing an outlet's timezone or day-start must not retro-move a past invoice, a past shift or a sealed snapshot into a different day — the same immutability reasoning as the no-FK provenance in §6.

**Separately, and more urgently: the current UTC bucketing is a defect in shipped M3 code, not merely an M4 prerequisite.** `business_date_from` (`apps/pos/src-tauri/src/commands/billing.rs`) and the display-number reset (`edge/database/src/repo.rs`) both bucket by UTC day. In IST the UTC day rolls at **05:30 local**, so any outlet trading past midnight is already mis-bucketing invoice numbers and day-end / cash-shift reconciliation today. The M3 milestone record claims correctness it does not have. Recorded in `docs/retro.md` and corrected in the M3 acceptance record as part of the pre-track.

### 10. Ingest is envelope-wrapped. No bare REST writes.

| Route | Aggregate types pinned by route | Direction pinned by §50.1 |
|---|---|---|
| `POST /inventory/ledger-entries` | `stock_ledger_entry`, `stock_deduction_gap` | EDGE_TO_CLOUD |
| `POST /inventory/counts` | `stock_count` | EDGE_TO_CLOUD |

Each takes a `SyncEnvelope` whose `payload` is the aggregate. A mismatch between the envelope's `aggregate_type`/`direction` and what the route pins is **422, never a coercion** — the ADR-012 §50.1 pattern, unchanged. Config write routes (`inventory_item`, `recipe`) are ordinary unwrapped cloud writes, and read paths stay unwrapped, as everywhere else.

### 10.1 The deduction gap is cloud-visible, and rides the ledger route

`stock_deduction_gap` is not edge-local, for two reasons that are about people rather than storage:

- **The person who can see it and the person who can fix it are different people in different places.** Fixing a gap means authoring a recipe, which is cloud config under `recipe.manage`. A POS-only report reaches a cashier who cannot act on it.
- **Variance is read in the cloud, and whatever explains a number must live where the number is read.** An edge-only gap record makes the cloud variance report unexplainable by construction, and an unexplained shortfall reads as theft.

The row records `menu_item_id`, `menu_item_variant_id`, quantity sold, reason and timestamp — enough to author the missing recipe without going back to the outlet.

**It is a signal, not a correction.** Deductions are **never** backfilled when the recipe is later authored; that would retro-alter history, which §immutable-financial-history forbids and which the no-FK provenance model in §6 exists to make impossible. In the variance report it appears as a **named term** — "N sales unaccounted" — and is never folded into shrinkage.

**One mechanical correction to the instruction, reported rather than worked around.** "A sibling `aggregate_type`, not a new aggregate" cannot be taken literally: `validateAuthority` (`backend/internal/ordering/statemachine.go:94`) rejects any aggregate type absent from `contracts.AggregateAuthority` as *unknown*, so `stock_deduction_gap` must be a real `AggregateType` member with an authority entry or no envelope carrying it can validate at all. What it does **not** get is its own route. `requireEnvelope` (`backend/internal/ordering/service.go:24`) pins exactly one type per call, so the inventory handler switches on `env.AggregateType` across the route's declared set, calls the existing single-type pin for the matched arm, and returns 422 from the default arm. The invariant weakens from "a route pins one aggregate type" to "a route pins a declared set"; anything outside the set is still 422, and no existing context's pinning changes.

### 11. Permissions

Added to the frozen `Permission` enum: `inventory.manage`, `inventory.count`, `recipe.manage`, and — riding along — `billing.manage`. `RoleCodeInventoryManager` already exists in `packages/contracts/go/identity.go` and currently maps to no permissions at all.

**`billing.manage` lands with its check, in the same sequence.** `backend/internal/compliance` today gates GSTIN writes on `outlet.manage`, so whoever may rename a table may set the GSTIN printed on every invoice. The permission and the enforced check on that write path land together; a permission defined and never checked is a documented obligation dressed as structural enforcement, which is worse than the honest gap it replaces.

**`wastage.approve` is deliberately NOT added**, and the reasoning was accepted at review: rule (i) binds it as much as it binds `billing.manage`, so an unused permission is the thing to avoid, not the thing to pre-place. The approval *workflow* moves to M5, landing with the append-only approval row that enforces it — a mutable approval flag on an append-only row is a contradiction.

**Wastage recording stays in M4, in T3, explicitly confirmed.** Only the approval workflow moves. An append-only wastage fact needs no approval to be true: a cook dropped a tray, the stock is gone, and the ledger records it whether or not a manager has since countersigned. Recording is gated on `inventory.manage`.

---

## Rules written into the contract

These are normative for every builder on M4 and are restated in the migration comments.

1. **Stock never blocks a sale.** Negative stock is permitted, is a variance signal and is not an error. There is **no `CHECK (quantity >= 0)`** anywhere in the stock path, in either store. A restaurant that has sold more than its records say it held has a counting problem, not a reason to refuse a customer.
2. **A missing or broken recipe never fails a confirm.** No recipe, an unresolvable unit, a cycle, a depth overrun — every one records a `stock_deduction_gap` and lets the sale complete. "Items sold with no recipe" is a visible report, per §64: staff are told whether intervention is needed.
3. **Concurrent deduction is serialized by transaction.** The edge is a **single SQLite writer** (ADR-013: one native executable over one SQLite file, WAL). LAN clients — the KDS today, the waiter app at M9 — are *command clients*, not writers: they send commands to that process and it performs the write. Deduction therefore runs inside the same transaction as `confirm_order` and needs no lock of its own. This is written down rather than left implicit because it is load-bearing: the day a second process writes that file, this decision and `ReplayTransition`'s duplicate handling both break, and they break silently.
4. **Stock never syncs downward.** The cloud **may** re-derive a stock view by summing the ingested ledger. It may **never** mirror the edge's snapshot, and no route ever sends a stock quantity cloud→edge. The ledger is the only thing that crosses, and it crosses upward.

---

## Migrations

| File | Contents |
|---|---|
| `sqlite/0013_outlet_day_start.sql`, `postgres/0013_…` | `outlet.day_start_time` (§9.2). An **outlet**-context change and the prerequisite of `business_date`. |
| `sqlite/0014_menu_default_variant.sql`, `postgres/0014_…` | `menu_item_variant.is_default` + partial unique index (§2.1). A **menu**-context change, kept separate from inventory so its blast radius is legible. |
| `sqlite/0015_m4_inventory_config.sql`, `postgres/0015_…` | `inventory_item`, `item_unit_conversion`, `recipe`, `recipe_ingredient`, `modifier_ingredient_delta` |
| `sqlite/0016_m4_stock_ledger.sql`, `postgres/0016_…` | `stock_ledger_entry`, `stock_count`, `stock_count_line`, `stock_deduction_gap` |
| `sqlite/0017_m4_stock_snapshot.sql` — **SQLite only, no PostgreSQL counterpart** | `stock_balance_snapshot` |

0017 having no PostgreSQL twin is deliberate and is the visible marker of §9. The absence should be as obvious to a future reader as `invoice_sequence`'s is.

Three of the five are single-column additions to existing contexts. They are separate files rather than one because a future reader tracing why `outlet` grew a column should not have to read an inventory migration to find out.

---

## Self-review against the CLAUDE.md contract rubric

Findings, including the ones that changed the design.

| Rubric item | Finding |
|---|---|
| **App-generated UUIDv7/ULID per §74, no DB-side defaults** | ✅ Every new table takes an app-generated id. No `DEFAULT gen_random_uuid()`, no `DEFAULT (lower(hex(randomblob(16))))`. |
| **No nullable columns in primary keys** | ✅ `stock_balance_snapshot` PK is `(outlet_id, inventory_item_id, business_date)`, all NOT NULL. Every other table has a single NOT NULL `id`. `recipe.menu_item_variant_id` is NOT NULL (§2), so its uniqueness is one plain index over three non-null columns — the ruling removed the nullable-uniqueness hazard this rubric line exists to catch. |
| **Single authority per §50.1 — no split-authority columns** | ⚠️ **This rubric line changed the design twice.** (a) `docs/spec/inventory.md` lists *Current cost, Weighted average cost, Last purchase price* as fields of the inventory item, and `docs/domain/INVENTORY_MODEL.md` describes current stock as a materialized projection. Modelled literally, `inventory_item` would be a cloud-owned config row carrying four edge-written columns — the precise defect ADR-011 named. Resolved by moving cost to `stock_ledger_entry` and stock to the snapshot (§1, §8, §9). (b) The draft's separate current-stock projection table was removed for the same family of reason: a second stored representation of a derived quantity is a second thing that can be wrong. |
| **No credential material in audit values, logs or wire types** | ✅ Nothing in v0.5.0 carries credential material. The audit redact list is unchanged. |
| **Uniqueness constraints tenant-scoped, not global** | ✅ `inventory_item` SKU is unique per `(outlet_id, sku)` — inventory is outlet-scoped per `docs/spec/inventory.md` — never globally. `recipe` is unique per `(tenant_id, menu_item_id, menu_item_variant_id)`, one index, no partial. `item_unit_conversion` is unique per `(inventory_item_id, pack_unit_label)`. `menu_item_variant.is_default` uses a partial unique index on `(menu_item_id) WHERE is_default` — the one place a partial index is correct, because it enforces *at most one* rather than distinguishing rows. |
| **Additive change to frozen contracts requires version bump + ADR** | ✅ v0.4.7 → **v0.5.0**, this ADR. Every change is additive: no existing column changes type, nullability or meaning, and nothing that compiles today stops compiling. |

Three further findings from reading the rubric's neighbours in CLAUDE.md:

- **The migration-list trap.** Migrations 0009–0011 existed on disk and never applied because nobody added them to `MIGRATIONS` in `edge/database/src/migrations.rs`; 0005 did the same before them. Three new SQLite migrations land here. This is in the landing checklist below rather than left to a builder's memory.
- **Multi-crate cascade.** New `pub` signatures in `edge/database` will be consumed by `apps/pos/src-tauri` and `edge/sync`, which are separate cargo workspaces. `make check-seams` is mandatory; this has broken nine times.
- **`openapi.yaml` is machine-checked against nothing.** The drift check is TS↔Go only, and the spec silently drifted on three `MenuItem` fields for two versions. v0.5.0 adds the largest OpenAPI surface since 0.4.0 to a document with no automated guard. Not a blocker for this ADR; recorded so it is not mistaken for a covered case.

---

## Landing checklist

1. SQLite `0013`–`0017`; PostgreSQL `0013`–`0016` (no `0017`).
2. **Add all five SQLite migrations to `MIGRATIONS` in `edge/database/src/migrations.rs`.** A migration absent from that list never applies.
3. TS + Zod types, mirrored Go structs, fixtures, and TS↔Go round-trip drift tests.
4. `AggregateAuthority` entries for `inventory_item`, `recipe`, `stock_ledger_entry`, `stock_count`, `stock_deduction_gap` — and a drift-test assertion that `stock_balance_snapshot`, `item_unit_conversion`, `recipe_ingredient`, `modifier_ingredient_delta` and `stock_count_line` are **forbidden** as `AggregateType` members, the `print_job` / `refresh_token` precedent (`go/drift_test.go:174`).
5. OpenAPI: three envelope-wrapped ingest routes, the config write routes, and the `/sync/config` contribution.
6. `Permission` enum: `inventory.manage`, `inventory.count`, `recipe.manage`, `billing.manage`. No `wastage.approve`.
7. A **persistence round-trip test per new table, named per track** — not folded into the T6 e2e suite.
8. Deferred-column exact assertions for `yield_factor_ppm` and `unit_cost_paise`.
9. `packages/contracts/package.json` → `0.5.0`.
10. **CLAUDE.md line 61** → `FROZEN at v0.5.0 … migrations through 0017`, plus a v0.5.0 paragraph in the contracts-status block stating the four rules.
11. ~~Outlet day-start time and the `business_date` definition~~ — **decided, §9.2.** Implementation of the corrected `business_date_from` follows in T2/T3 against that definition.
12. ~~`docs/retro.md` entry and M3 acceptance-record correction~~ — **done 2026-08-20**: `docs/retro.md` "A function named for outlet-local time, computing UTC"; `docs/RESUME.md` §2 correction block; the stale `device_token` item in `docs/backlog-m2.md` closed in the same pass.
13. `make check-seams`.

---

## Resolved decisions

Both questions this ADR was held open on were ruled at review on 2026-08-20 and are folded in above.

1. **Variant binding → NOT NULL** (§2). Rejected nullable on the `NULL != NULL` uniqueness defect rather than on preference. Triggered the default-variant prerequisite in §2.1, since sellable items demonstrably do not all carry a variant today.
2. **Deduction gap → cloud-visible, sibling type on the ledger ingest route** (§10.1), with one mechanical correction reported: it must be a real `AggregateType` member, because `validateAuthority` rejects unknown types outright. It gets no route of its own.
