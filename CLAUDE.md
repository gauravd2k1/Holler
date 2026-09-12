# HOLLER — Agent Working Context

Restaurant Operating System for India. Local-first: core ops run without internet.
Full vision: `docs/vision.md`. Full spec source: `HOLLER_MASTER_PROMPT.md` (orchestrator/humans only — builder agents do not load it).

## Tech stack
- POS desktop: Tauri + React + TypeScript + Rust + SQLite (WAL)
- Cloud backend: Go, PostgreSQL, Redis, NATS JetStream (modular monolith)
- Web admin: React, TypeScript, Vite, TanStack Query/Router
- KDS: PWA (web), LAN-first
- Waiter app: Flutter (Android-first) — decided, see ADR-010
- Contracts: `packages/contracts/` — TS+Zod, Go structs, OpenAPI, SQLite/Postgres migrations. Read-only for builder agents.

## Deployment target (ADR-013) — read before assuming anything about the host
- **Outlet machines run bare Windows 10, 64-bit, 4GB RAM, spinning disk.** No WSL, no Docker, no PostgreSQL, no Redis, no NATS — ever. The outlet runs one native executable (the POS) over one statically-linked SQLite file, syncing outbound over HTTPS.
- Restaurant hardware is old and minimal and will not be upgraded, virtualised or extended. Never add an outlet-side dependency that needs a developer toolchain, a service install, or internet at install time.
- Installer must embed the WebView2 runtime and the VC++ runtime rather than downloading them — installing on a flaky connection is the normal case.

## Environment map (where each thing actually runs)
| Environment | What runs there |
|---|---|
| **Native Windows (PowerShell)** | Claude Code itself, and all Rust/Tauri/Go builds and tests. Run `cargo`, `go` and `pnpm` here — not inside WSL. |
| **WSL2 Ubuntu** | Docker Compose only: Postgres, Redis, NATS. Reached via `make dev`; you do not work inside it. |
| **WSL2 Kali** | Unrelated to this project. Never use it for Holler work. |
| **Outlet machine (production)** | Bare Windows 10 — one native POS executable over one SQLite file. No WSL, no Docker, no database server (ADR-013). |

The split that matters: WSL2 hosts the **cloud** dependencies for local development and nothing else. It is a convenience — Hyper-V, a remote database or a native Go build would serve equally — and no Holler component requires it. Nothing in the shipped outlet path touches it.

## Dev environment (developer convenience only — NOT a product requirement)
- Dev machine here: Windows laptop (i7-14650HX, 24 logical cores, 32GB RAM). Cap concurrent agent sessions at 3 — a scheduling choice, no longer a memory one. Page file is system-managed (~62GB on D:, peak use ~16GB), so "paging file too small" is **not** the expected failure on this box; corrected 2026-08-22, having previously read i7-9750H/16GB.
- **The recurring Rust build failure here is `LNK1104: cannot open file ...exe`, and it is McAfee, not memory.** McAfee is the active real-time scanner (Defender's `WinDefend` service is stopped/passive); it holds a lock on each freshly-linked test binary, so a different test target fails on each run. Compilation has already succeeded when this fires. Re-running makes forward progress, because cargo caches every binary that did link — two or three retries reach green. Do not read it as a code error, and do not "fix" it by reducing parallelism.
- `make dev` brings up the **cloud** stack for local development. It is never run at an outlet. Frontend runs natively for HMR.

## Money / time / identifiers
- Money: INR stored as integer paise (₹125.50 = 12550). Never floating point for money.
- Time: UTC storage; outlet timezone stored separately, rendered local. Business day may cross midnight.
- IDs: UUIDv7/ULID internally. Human-facing numbers are short (Order #A184, Invoice FY26/PNQ/001423). Never expose sequential PKs as security identifiers.

## Coding rules
- Strict typing, no `any`. Business logic outside UI components; DB logic outside HTTP handlers.
- Never store a bare global builtin on an object field. `setTimeout`, `setInterval`, `fetch`, `WebSocket`, `crypto.*` are receiver-bound in browsers and throw `Illegal invocation` when detached — bind at capture or call them free. Node tolerates it, so no Node-based test will catch it, and **no linter catches it either** (`unbound-method` cannot see it: `lib.dom.d.ts` declares these as functions, not methods). The only guard is the real-browser smoke test (`docs/retro.md`, 2026-08-11).
- **Build-green ≠ dev-works for the Tauri/web apps. Never report a frontend change as verified on `pnpm build` + `tsc` + unit tests alone.** Two incidents, both invisible to every green suite: the KDS detached-global crash (browser-only, `docs/retro.md` 2026-08-11) and the POS white screen (dev-server-only, 2026-08-20 — a stale `node_modules/.vite` prebundle; `optimizeDeps` is a dev-server mechanism that `vite build` never reads, so the build cannot fail on it). The dev server and the browser are each a distinct runtime from the build output, and a failure in either is invisible from the others. When a frontend change is claimed to work, say which runtime it was observed in. First move when a Tauri app renders blank: check `node_modules/.vite` against the mtime of what it was built from, and check the Network tab, not only the console.
- Provider-specific code (aggregators, payments, printers) behind interfaces — never leak into core domain.
- No magic numbers, no hard-coded tax rates/restaurant IDs/URLs, no secrets committed.
- Contracts (`packages/contracts/`) are edited only by the orchestrator/architect session — never by a builder agent.
- **An ADDITIVE contract change has a consumer list too.** A version is not complete until every consumer is updated or explicitly deferred with a reason — the discipline already applied to breaking changes. 0.5.2 added `recipe_ingredient.quantity_dimension` and nothing was updated to read it, so the guard it existed for still could not fire; a builder found it. **A column nothing reads is a column that does not exist.**
- Never `// TODO implement later` for current-milestone or excluded-list work.

## Directory ownership
- `apps/pos` — POS Tauri app. `apps/admin` — web admin. `apps/kds` — kitchen display PWA. `apps/waiter` — Flutter app.
- `edge/` — local edge node services (sync, printer, device, database) — Rust.
- `backend/internal/<context>` — one bounded context per directory (auth, tenant, outlet, menu, ordering, kitchen, inventory, procurement, payments, aggregators, compliance, reporting, crm).
- `packages/contracts` — cross-boundary source of truth (read-only to builders). `packages/ui`, `packages/validation`, `packages/generated`.
- `docs/spec/<context>.md` — one spec per bounded context; an agent loads only CLAUDE.md + its assigned spec file(s) + `packages/contracts/`.

## Test/build commands
- `make dev` — start local stack (WSL2 Docker Compose + backend).
- `make test` — unit + integration tests.
- Backend: `go test ./...` inside `backend/`.
- POS: `pnpm test` / `pnpm tauri dev` inside `apps/pos/`.
- CI: lint, format, unit, integration, contract-drift check, build, security scan.

## Contracts status: FROZEN at v0.8.2 (M6 Phase C aggregator shapes ADR-022, `order.source` widening ADR-026, `GET /orders` documented ADR-027; migrations through sqlite 0035 / postgres 0035)
<!-- The version and migration numbers on the heading above are checked by
     scripts/check-milestone-marker.mjs against packages/contracts/package.json
     and the migration files on disk. Third staleness of this line (0.4.7,
     0.5.3) is why. Bump the heading when you bump the package. -->
`packages/contracts/` holds the source of truth — SQLite schema, PostgreSQL migrations, TS+Zod types, mirrored Go structs, OpenAPI spec, and fixtures with Go+TS round-trip drift tests wired into CI. **Read-only to builder agents** (ADR-008); only the orchestrator/architect session edits it, serialized, with a version bump + ADR note for semantic changes.

v0.2.0 added identity/RBAC/tables for Milestone 1 (ADR-011): `app_user`, `role`, `role_permission`, `user_role`, `restaurant_table`, `table_session`, `audit_event`. Three rules bind every builder:
- A table's **definition** (`restaurant_table`) is config, cloud→edge. A table's **live state** (`table_session`) is an edge-authoritative operational aggregate, edge→cloud. No row is half-config, half-transaction.
- The edge SQLite file caches Argon2id hashes so login works offline. It is **encrypted at rest** — never copy it or its backups anywhere unencrypted.
- `password_hash`/`pin_hash`/`token_hash` never appear on the wire (except `GET /sync/config` to an enrolled edge node) and never inside an `audit_event` old/new value. Use the audit helper, which redacts them.

v0.2.1 (ADR-011 addendum, ADR-012) added: envelope-wrapped ingest as the **single** edge→cloud replay pattern — every mutating route for an `EDGE_TO_CLOUD` aggregate takes a `SyncEnvelope` whose `payload` is the aggregate, the route pins `aggregate_type`, §50.1 pins `direction`, and a mismatch is 422 rather than a coercion; `table_session` ingest routes on that pattern; `POST /menu/items/{itemId}/availability`; the `refresh_token` table (cloud-only, deliberately not an `AggregateType`); and `AuditEvent.tenant_id`. Read paths stay unwrapped.

v0.3.0 (ADR-014) added the Milestone 2 kitchen shapes. Four rules bind every builder:
- `station`, `menu_item_station`, `printer`, `station_printer` are **config, cloud→edge**. The `kot` is **edge-authoritative**. Same split ADR-011 drew between `restaurant_table` and `table_session` — a station's definition is a management decision, the ticket at it is a shop-floor transaction.
- `kot.station` stores the station's **`code`**, never its id, so a ticket survives a rename.
- `print_job` and `kot_status_history` are **edge-local**: SQLite only, no Postgres mirror, deliberately absent from `AggregateType` (the `refresh_token` precedent, from the other side). Never give either a sync direction.
- `POST /kots/{kotId}/status` is the **only** writer of `kot.status`, and it only replays. No cloud handler transitions a ticket.

v0.4.0 (ADR-016, ADR-017) added the Milestone 3 billing shapes — `invoice`, `invoice_line`, `invoice_series`, `invoice_sequence`, `tax_profile`, `tax_rule`, `compliance_version`, `outlet_fiscal_profile`, `discount_definition`, `payment`, `payment_allocation`, `cash_shift`, `cash_movement` — plus the `device_credential` enrollment shape. This is the first milestone that puts money on the wire, so five rules bind every builder:
- `invoice`, `payment`, `cash_shift` are **edge-authoritative** (edge→cloud): the outlet bills and takes money with the uplink down and the cloud only replays. `tax_profile`, `compliance_version`, `invoice_series` and `discount_definition` are **config, cloud→edge** — tax rules, numbering format and discount policy are management decisions. Same split ADR-011 and ADR-014 already drew.
- **Numbering splits definition from counter.** `invoice_series` is cloud config; `invoice_sequence` is **edge-local** — SQLite only, no Postgres mirror, no `AggregateType`, no sync direction, ever. Mirroring the counter would make the cloud a second writer of invoice numbers, which §33 forbids. The issued number travels on the invoice; the counter that produced it never leaves the outlet.
- **Money is integer paise end to end, and the edge computes it.** Tax is computed per line at full precision, summed per component, rounded half-up to paise **once**, then the grand total to the rupee with the delta in `round_off_paise`. Never recompute tax in TypeScript or the Tauri layer — those layers format what the edge returns (`edge/database/src/tax/`).
- `invoice_line`, `tax_rule`, `payment_allocation`, `cash_movement` and `outlet_fiscal_profile` are **child rows, not aggregates** — they travel inside their parent's payload or config bundle, the `menu_item_variant`/`station_printer` precedent. Do not give any of them a sync direction.
- `device_credential` is **cloud-only**: no SQLite mirror, deliberately not an `AggregateType` (the `refresh_token` precedent). The plaintext token is returned **once** at enrollment. `device_token_hash` joins `password_hash`/`pin_hash`/`token_hash` on the audit redact list.

v0.4.1–v0.4.7 are additive amendments to that baseline, each with its own version bump and ADR note:
- **0.4.1** `ItemQuantityChanged`; `POST /devices/enroll` frozen. **0.4.2** `menu_item.tax_profile_id` (null falls back to the outlet default). **0.4.3** `device_credential_cache` — the edge-cached half of ADR-017 — and ingest gated by device. **0.4.4** the compliance config write routes documented in OpenAPI.
- **0.4.5** per-row `config_version` on `device_credential`, so `GET /sync/config`'s `since_version` filter reaches it like every other config table; append-only triggers on `payment` (a tender is corrected by an appended reversal, never a mutation); `print_job.invoice_id`, so a bill can become a print job; and `menu_item.hsn_sac` — **an invoice cannot issue with a NULL or blank HSN/SAC on any line**, enforced at the edge, because a GST invoice without it is not a compliant document.
- **0.4.6** closed three-field `MenuItem` drift in the OpenAPI spec. **0.4.7** `printer_role` — a join table, not a column on `printer`, so one device can be both KITCHEN and BILL without a `BOTH` member every reader special-cases. **A printer with no role row is a candidate for neither path**: absence is never read as "sure, print bills to it", and an outlet with no BILL printer must fail loudly at issue time.

v0.5.0 (ADR-018) added the Milestone 4 inventory shapes — `inventory_item`, `item_unit_conversion`, `recipe`, `recipe_ingredient`, `modifier_ingredient_delta`, `stock_ledger_entry`, `stock_count`, `stock_count_line`, `stock_deduction_gap`, `stock_balance_snapshot` — plus `outlet.day_start_time` and `menu_item_variant.is_default`. Five rules bind every builder:
- **Stock never blocks a sale.** Negative stock is permitted and is a variance signal, not an error. There is no `CHECK` forbidding it anywhere, deliberately.
- **A missing or broken recipe never fails a confirm.** No recipe, an unresolvable unit, a sub-recipe cycle, a depth overrun — each records a `stock_deduction_gap` and lets the sale complete. "Items sold with no recipe" is a visible report.
- **Quantities are integer micro-units end to end** (gram/litre/piece × 10⁶, scale in the field name), the money=paise rule generalised. The binding range limit is JavaScript's 2^53, not `i64`. Sub-recipes resolve as exact `i128` rationals and round **half away from zero exactly once**, at the leaf.
- **Current stock is never a column on `inventory_item`, and cost never lives there either.** Stock is the ledger plus its sealed snapshot; cost is on the ledger entry. A quantity written by the edge on a cloud-owned config row is the half-config/half-transaction row ADR-011 forbids.
- **`stock_balance_snapshot` is edge-local** — SQLite only, no Postgres mirror, no `AggregateType`, ever (the `invoice_sequence` precedent). Reads select entries **not covered by its `through_entry_seq` mark**, never entries after a date: a late arrival carrying a sealed day's date would otherwise vanish permanently and silently.

**0.5.1** added `recipe.output_dimension` and `recipe.output_quantity_micro`, NOT NULL on every recipe. Without them a sub-recipe reference could only be a dimensionless multiplier, and rescaling a sub-recipe then silently multiplied every parent's deductions with no error — a gravy moving from 300ml to 3-litre batches makes every dish referencing it wrong by 10×. **The multiplier is `requested_quantity / output_quantity_micro`, carried to the leaf as an exact rational and never materialised as a rounded number.** A parent whose dimension differs from the sub-recipe's output is an authoring error rejected at cloud write time, never converted — a recipe is not an inventory item, so no density row exists.

**0.5.2** added `recipe_ingredient.quantity_dimension`, NOT NULL — **the unit the author chose, never derived from the referent.** Without it a quantity was dimensionless in storage: reclassify chicken from MASS to COUNT and every recipe silently reinterprets `220_000_000` as 220 whole birds, wrong on every plate until a physical count catches it. **If a write path or UI auto-fills this column from the item it points at, the comparison becomes `x == x` and the guard can never fire — and it will look correct in review.** The cloud rejects a mismatch at write time; the edge degrades to a `DIMENSION_MISMATCH` gap. Changing an item's dimension is forbidden while any recipe references it: that is a migration, not an edit.

v0.5.0 also closed four M2/M3-era defects that shared one shape — structural guarantees written as comments and enforced on at most one side: UTC business-date bucketing (`outlet.day_start_time` now defines it), and missing append-only enforcement on `payment` (PostgreSQL), `audit_event` and `cash_movement`. `invoice` gained real immutability too: every column frozen, one legal `ISSUED→CANCELLED` transition. Three lints in `edge/database/src/migrations.rs` hold that ground — every APPEND-ONLY/IMMUTABLE claim must have a trigger, every single-store migration must be declared with a reason, and `DEFAULT gen_random_uuid()` may only decrease.

**0.5.8** implemented ranged sync for the two high-volume stock streams (ADR-018 replay addendum). `sync_state` gained **two** cursors — one per stream, edge-local, SQLite-only — and `stock_deduction_gap` gained `entry_seq` NOT NULL in **both** stores, minted from its own counter separate from the ledger's. Three rules bind every builder:
- **A contiguity check must never wedge replay.** Rejecting an entry whose mark is beyond the cloud's high-water mark turns one lost row into a permanent silent outage — every row behind it is refused too, and nothing downstream distinguishes a quiet outlet from a wedged one. The hole is recorded in `ledger_replay_gap` (cloud-only) and the entry is **accepted**. That table carries `resolved_at`, because a hole that later fills is not a loss, and a UNIQUE span key, because a hole re-observed across batches is not N holes. **Detection is the goal; blocking is a side effect you do not want.**
- **The same outage exists at the other end.** An edge that retries one permanently-rejected entry forever strands every entry behind it. The retry budget is spent **per entry**, not per stream: after N permanent rejections the entry lands in `sync_replay_block`, the cursor moves past it, and a human sees it on the POS. Transient failures (transport, 5xx, 401/403, 408, 429) never spend that budget. Halting sync is survivable — nothing at the outlet depends on the uplink — halting it silently is not.
- **`entry_seq` is 1-based and cursors default to 0 meaning "nothing acked".** A 0-based sequence skips every outlet's first entry, permanently and silently. And **NOT NULL on an existing table is a trap**: a constant default under a UNIQUE key passes on an empty table and dies on the second row, so the column is added by rebuild-and-backfill, falsified against a populated table.

**0.5.9** put `stock_ledger_entry.source_stock_count_id` **on the wire**. The column has existed in both stores since 0.5.5 and the edge both writes and sends it, but the cloud had never heard of it — absent from the Go struct, the INSERT and the SELECT — and `json.Unmarshal` is lenient, so it was discarded in silence and the Postgres column was NULL for every row. Two rules bind every builder:
- **The additive-change consumer list reaches the wire types, not just the schemas.** A column added to both stores is not landed until the Go struct, the Zod schema, the OpenAPI shape and the repository's INSERT/SELECT all carry it. This is the 0.5.2 lesson again, one layer out: a column nothing reads is a column that does not exist, and here something *did* write it, one hop upstream.
- **A fidelity test proves fidelity only for the fields its fixture populates.** Criterion 6 compares the edge row against the 201 echo and the stored bytes, and passed throughout: the echo cannot see a dropped field at all (the handler echoes the struct it decoded, so the field is absent from both sides), and the storage compare was green because its fixture was a wastage entry, on which every count-provenance field is legitimately null — and a null round-trips through a nonexistent field perfectly. **Green on absent data.** Every provenance group now needs its own populated row in the criterion 6 fixture and in `packages/contracts/fixtures/`, or the hole reopens under the next field's name.

v0.6.0 (ADR-019) added the Milestone 5 procurement shapes — `supplier`, `supplier_item`, `purchase_order`, `purchase_order_line`, `goods_receipt_note`, `grn_line`, `grn_gap`, `purchase_return`, `purchase_return_line`, `stock_transfer_out`, `stock_transfer_line`, `grn_sequence`, `supplier_invoice`, `supplier_credit` — plus `procurement.manage`/`procurement.approve`, `role.po_approval_limit_paise`, and three provenance columns on `stock_ledger_entry`. Six rules bind every builder:
- **A GRN NEVER BLOCKS ON A PO.** Goods arrive against a PO that never synced, against one amended after dispatch, and with no PO at all. Each records a `grn_gap` and **accepts the receipt**. `goods_receipt_note.purchase_order_id`, `supplier_id` and `grn_line.purchase_order_line_id` are nullable in **both** stores and no CHECK ties a receipt to an order — that absence is load-bearing and must not be tidied up. This is "stock never blocks a sale" generalised to the inbound side: refusing a delivery standing in the kitchen doorway is the outage, not the protection.
- **`purchase_order` carries NO receipt state, and the edge's and cloud's receipt progress LEGITIMATELY DIFFER.** The edge derives it from its own `grn_line` rows; the cloud derives it from every outlet's. A shared PO reads "40 of 100" at one till and "90 of 100" in the admin, simultaneously, and both are right. **Show both and label them; never reconcile them** — reconciling reintroduces the second writer that keeping status off the row exists to avoid (§50.1).
- **The GRN converts the supplier's purchase unit exactly once, at the edge**, and stores **both** sides (`entered_quantity_micro` + `base_quantity_micro` + `pack_size_micro_applied`). Receiving is the third quantity-entry path and the one with the worst odds; when a receipt turns out 1000x wrong, "what did they actually type?" must be answerable from the row. `entryIntentEcho` is mandatory on the receiving screen.
- **`quantity_dimension` is the unit the author chose, never derived from the referent** — 0.5.2's rule, now on four more tables. If a write path or UI auto-fills it from `inventory_item.dimension`, the comparison becomes `x == x`, the guard can never fire, and it will look correct in review.
- **`grn_sequence` is edge-local and `grn_gap` is a PLAIN OUTBOX.** The counter never leaves the outlet (`invoice_sequence` precedent). The gap has no `entry_seq`, no cursor and no contiguity check: it is a discrete event a buyer acts on, not a per-sale stream like `stock_deduction_gap`, which is why that one earned the 0.5.8 machinery and this one does not.
- **A single-store table hidden inside a mirrored migration is undeclarable.** `SINGLE_STORE_MIGRATIONS` pairs files by stem, so `grn_sequence` ships as `sqlite/0028` and the cloud-only accounts shapes as `postgres/0029`, each declared with a reason. Note also that **there is no `role` table in SQLite at all** — the edge flattens permissions into `app_user.permissions_json` — so `po_approval_limit_paise` is Postgres-only by necessity and by design: the edge must never approve a purchase order.

0.6.0 also **removed** the `unit_cost_paise` and `yield_factor_ppm` exemptions from `scripts/check-contract-field-consumers.mjs`, because procurement now consumes both. **An exemption that outlives its reason is a silenced failure.** `batch_code`/`expiry_date` take their place, exempt with **M6** named — modelled now only because batch identity is captured at receipt or never.

**0.6.3** (ADR-021) added `stock_ledger_entry.line_total_paise` — the exact invoiced money a row is worth. `unit_cost_paise` is a per-base-unit RATE rounded to whole paise once per receipt, and weighted average cost summed that rate, inheriting a rounding it could never recover: **±0.5 paise on a per-gram figure is +20% at 2.5 paise/g**, one-directional per item and worst on cheap staples. The ledger now stores the total and `procurement::cost` divides exactly once. Four rules bind every builder:
- **Receipts set `line_total_paise`; every other origin leaves it NULL.** Only a receipt has an invoiced total — wastage, counts, variance and outbound movements are valued AT the average, so a `quantity × rate` product for them would fabricate precision and feed it back into the average that produced it. The CHECK is **directional**: a total never appears without its rate, a rate may stand alone.
- **`unit_cost_paise` survives as a derived DISPLAY rate and is never an averaging input.** It is pinned by a drift test asserting `unit_cost_paise == round_half_away(line_total_paise × 10⁶ / quantity_applied_micro)` over a costed fixture. A stated invariant with no test is the defect class in half the retro log.
- **Holler implements a LIFETIME CUMULATIVE PURCHASE-WEIGHTED AVERAGE, not weighted average cost of stock on hand.** The averaging query is unbounded — no `through_entry_seq`, no `business_date`, no on-hand term. Only half of that was decided: excluding outbound rows is argued; the unbounded property is recorded nowhere. Filed in `docs/backlog.md` with the trigger **before the first pilot**. A purchase return leaving the figure untouched is a consequence of it.
- **A rebuild proves its own guarantees or it has none.** The SQLite side rebuilds `stock_ledger_entry`, so `migrations.rs` asserts afterwards that the insert-only guard **actually fires** (a real `UPDATE`, required to be rejected — not a `sqlite_master` name lookup) and that `stock_ledger_sequence` still leads `MAX(entry_seq)`, so a rebuild cannot regress ranged replay below the cloud high-water mark.

**0.7.0** (ADR-024) added the two routes M6 Phase B's admin console needs, plus the error-code enum, and **made the cloud the menu authority in practice rather than only on paper**. Four rules bind every builder:
- **`PATCH /menu/items/{itemId}` mutates FIVE fields and no others** — `name`, `base_price_paise`, `category_id`, `tax_profile_id`, `hsn_sac`. `id`/`outlet_id`/`tenant_id` are 422 if present, never a silent ignore. **`is_available` is excluded deliberately**: `POST /menu/items/{itemId}/availability` owns it and is an EDGE→CLOUD replay route, so accepting it here would make the cloud a second writer of a field the outlet authors. **`hsn_sac` may be changed but never cleared** — an invoice cannot issue without it (0.4.5). **`POST /menu/items` stays create-only**; a duplicate is a 409 through the shared classifier, and turning that INSERT into an upsert to make PATCH easier would make a duplicate create silently overwrite.
- **`GET /procurement/goods-receipts` serves a REPLICA and every surface must label it as one.** A GRN is edge-authoritative (ADR-019); the cloud holds a copy, the outlet's own view may legitimately differ, and the two are **shown and labelled, never reconciled**. Read-only: a receipt is corrected by an appended `purchase_return`, never an edit. All three quantity fields travel — `entered_quantity_micro`, `base_quantity_micro`, `pack_size_micro_applied` — because "what did they actually type?" must stay answerable from the row, and the nullable `purchase_order_id`/`supplier_id`/`purchase_order_line_id` are served **as nulls**, because a GRN never blocks on a PO and no read path may imply it did.
- **`ErrorCode` is an enum now, and the edge classifier must read the generated set, not string literals.** The error body was `{ code: string, message: string }` with no enumeration while the edge branched on the value, so a cloud-side rename was a silent behaviour change no drift test could see. **An enum nothing consumes is a column nothing reads** (0.5.2's rule, pointed at a wire field).
- **THE CONFIG PULL NOW HAS A PRODUCTION CALLER, AND UNTIL 0.7.0 IT HAD NONE.** `config::pull_and_apply_config` had existed since M1 with exactly one caller in the repository — a test — so a price edited in the cloud never reached a till. It is now driven by the A5 periodic loop, inside the same database lock as the outbox pump so a config apply and a pump cannot interleave on one connection. **A failed pull is logged and nothing else**: the till keeps its last applied config and carries on selling, because an outlet with no uplink is the normal case (ADR-013). Note what apply does and does not do: it **upserts, it does not prune** — a menu item deleted in the cloud is never removed from an edge, which is why the dev seed's extra items survive a pull and why C7's seed drift is still there (C7 is closed; the drift is no longer needed).

**0.8.1** (ADR-026) widened the `order.source` CHECK with **`AGGREGATOR`** and
**`TABLE_TAB`**. Four rules bind every builder:
- **ONE GENERIC AGGREGATOR MEMBER, NEVER ONE PER PLATFORM.** `aggregator_order.platform`
  is free TEXT precisely so a new platform needs no migration (0032), and the
  boundary check exists to keep platform names out of the core. A
  platform-named member re-imports that rejected decision AND becomes a new lie
  the first time a different platform arrives down the same path — which is the
  defect being fixed, under a new name. The platform is already recorded twice,
  in `aggregator_order.platform` and `order.source_payload_json`.
- **THE TWO PLATFORM-NAMED MEMBERS ARE DEPRECATED, NOT REMOVED** — removal is
  breaking; the trigger is the next breaking bump. Nothing has ever written
  either, and that claim is ENFORCED rather than asserted: the postgres
  migration raises and the SQLite pre-condition refuses to run if any row
  carries one. A deprecation note that silently widens over live data it claims
  does not exist was false when it was written.
- **NOTHING WRITES EITHER NEW MEMBER YET, and that is pinned by exact
  assertion.** The accept path still writes `DIRECT`; `TABLE_TAB` has no code at
  all. `scripts/check-order-source-drift.mjs` fails the build if anything under
  `edge/` or `apps/pos/src-tauri/src` emits one, and the writer that starts
  writing one removes it from that list IN THE SAME COMMIT — forced removal, not
  remembered removal.
- **SQLite cannot ALTER a CHECK, so 0035 REBUILDS the order table** and proves
  its own guarantees: row count, a sampled row compared column for column with
  nulls encoded, all four indexes restored, and the CHECK watched REJECTING a
  real INSERT inside a rolled-back savepoint. Note the difference from 0030 —
  the order table carries NO triggers, so there is no append-only guard to watch
  fire and the CHECK is the only live guard there is.

0.8.1 also closed two holes in the aggregator boundary check, both found by
planting a value and watching it pass: **`packages/contracts` was outside
`SEARCH_ROOTS` and `.sql` outside its extensions**, so the shared schema — where
a platform name binds every consumer at once — was the one place it could not
see; and **a word-boundary match on `ondc` cannot match `AGGREGATOR_ONDC`**, because `_` is a word
character, so an enum member named for a platform passed cleanly. Platform names
now match with separator-aware lookarounds; protocol tokens keep the word-boundary match, so a
trigger named `..._on_update` is not read as a Beckn callback.

**0.8.2** (ADR-027) is DOCUMENTATION ONLY -- no `.sql`, no `.ts`, no `.go`
shape moved. `GET /orders` gained its OpenAPI entry, having been **routed and
served since Milestone 1** (`backend/internal/ordering/http.go:32`) and absent
from the spec the whole time, which is why a survey of `openapi.yaml` read
`/orders` as POST-only. Three rules bind every builder:
- **A ROUTE ABSENT FROM THE SPEC READS AS A ROUTE THAT DOES NOT EXIST.** That
  mis-reading was about to buy a day of work and a contracts bump for something
  already shipping. Documenting it changed nothing about its behaviour: it
  returns a bare array of `CanonicalOrder`, unwrapped, **unbounded** (no limit,
  no cursor) and **oldest-first**. Both properties are written into the spec
  rather than fixed, because adding pagination while claiming to document
  existing behaviour is a behaviour change wearing a documentation label.
- **THE CLOUD WAS NOT HONOURING THE WIRE TYPE IT CLAIMED, IN TWO PLACES.**
  `display_number` was in neither the INSERT nor the SELECT -- the edge mints
  it, `CanonicalOrder` carries it, both stores have the column, and the cloud
  discarded it on ingest and served NULL forever after, so the back office
  could not name a single order. And timestamps were serialised with the
  connection's `+05:30` offset, which `z.string().datetime()` REJECTS, so every
  order failed validation in every TypeScript client. Neither is a contract
  change; both are the cloud failing to serve shapes that already existed.
- **A FIDELITY TEST PROVES FIDELITY ONLY FOR THE FIELDS ITS FIXTURE POPULATES
  AND ONLY FOR THE SPELLINGS ITS FIXTURE USES.** 0.5.9 was a field the fixture
  left null; this is a field the fixture formatted differently from the server.
  Both stayed green throughout, because **nothing had ever read an order back
  from the cloud in TypeScript** -- the till reads the edge, and the drift test
  reads a fixture authored with a `Z`. The guard added here asserts the
  MARSHALLED BYTES, never the `time.Time`, which compares equal under both
  spellings.

Two cross-cutting rules the 0.4.x line established the hard way: contract-shaped changes cascade across crates that do not share a cargo workspace (see `docs/retro.md` 2026-08-15), so run `make check-seams` after changing any `pub` signature in `edge/` or `apps/pos/src-tauri`; and a migration that exists on disk but is absent from `edge/database/src/migrations.rs`'s `MIGRATIONS` list **never applies** — 0009–0011 sat dead for exactly that reason, and 0005 before them.

## Current milestone: MILESTONE 6 — Sync gaps, back office, aggregator framework (CLOSED); current work: DEMO BUILD
<!-- MILESTONE-MARKER: 6 -->

<!-- Checked by scripts/check-milestone-marker.mjs against .claude/current-milestone.
     This block said "MILESTONE 2 — Kitchen" for the whole of M3: every M3 builder
     loaded M2's scope and M2's EXCLUDES as primary context and nothing noticed for
     an entire milestone. The marker exists so that cannot recur silently.

     The marker stays at 6 deliberately. The demo build is not a numbered milestone
     and the check demands a bare integer; retitling this block rather than widening
     the guard was the ruling (2026-09-11). M6 itself is CLOSED and tagged
     `m6-complete` — its record is in "Completed milestones" below and in
     `docs/m6-acceptance.md`. Everything under "Scope" from here down is the DEMO
     BUILD, and it is what binds a builder today. -->

**The brief is `docs/demo-kickoff.md` and it is the first thing to read.**
Purpose is a **client demo**: presentability and reliability of **what already
exists**. **No new features beyond the one named exception below, no M6.1, no
pilot-only work.** `docs/pilot-readiness.md` stays untouched, and an item being
on it is not a reason to do it now. **Report demo blockers only.**

**The repo is the authority** — where a plan document and the repository
disagree, the repository wins, and say so out loud before acting.

**One standing prohibition is REVERSED for this work: seeding the cloud's
catalogue is now ALLOWED.** The M5/M6 prohibition existed to stop the P2/P3/P5
defects being hidden by a seeded cloud; those are fixed, so the ruling is lifted
and seed parity across cloud and edge is the first item of work. Read that
deliberately rather than from memory.

**Contracts stay FROZEN at v0.8.1. The demo build requires NO contract change**
— and a builder proposing one has found either a real blocker to escalate or a
scope leak. `WAITER` already exists in `device.kind`; `order.source` stays
`POS`.

### Three facts about the live stack that will otherwise waste an hour

- **sqlite 0035 has NOT been applied to the live edge database.** Postgres has
  it. The edge takes it at the next clean bootstrap, which the demo seed work
  does anyway. Nothing writes the two new `order.source` members, so the lag
  changes no behaviour.
- **Thirteen rows are permanently blocked in the live edge outbox, and they are
  M6 C3's FIXTURE, not a fault.** Each is `missing_reference (HTTP 422)` on the
  variant foreign key. The demo's clean reseed clears them. **Do not clear them
  by seeding only the cloud's catalogue against the current edge database** —
  that makes the symptom vanish while leaving the underlying gap unfixed
  (`docs/m6-phase-c-boundary.md` §3.1).
- **No exit path on this build seals the edge database.** Neither a window close
  nor `Ctrl+C` fires `RunEvent::Exit`, so a plaintext `edge.db` is left beside
  the `.enc` every time, and the `.enc` is only as current as the last
  successful seal. That is gap A6, it blocks a pilot, and it is why no
  trustworthy backup can be taken before a risky migration.

### Carried out of M6, still live, still binding

- **Phase A closed with FIVE of seven gaps landed — A1, A1b, A2, A3, A5 — and
  THREE carried: A4, A6, A7. Never report it as "Phase A complete".**
- **A7 is the one that matters: 78 rows on the live edge database have no route
  and can never be sent** (55 `kot`, 22 `stock_count`, 1 `invoice`, measured
  2026-09-07). `edge/sync/src/route.rs` maps only `order` and `table_session`.
  **The `order` stream replays end to end; nothing else replays at all.** The
  demo story does not depend on any other stream replaying — do not read a KOT
  or an invoice failing to reach the cloud as a new defect.
- **M6 C2 is PARKED**, trigger *any platform sandbox access granted*. M6 closed
  with it parked, deliberately. Do not re-run any M6 criterion and do not
  reconstruct a verdict from git history.

### Scope — the demo story, in order

The six steps the client is shown, plus the one approved addition:

1. Dine-in order at the till with a modifier, KOT on the KDS, kitchen bumps it.
   **1a (APPROVED ADDITION, see below): an order placed from the waiter's phone
   appears on the KDS and the hub.**
2. Bill: GST invoice, split cash + UPI, receipt printed to PDF and opened on
   screen.
3. Stock screen: ingredients deducted per recipe from that sale; low-stock
   warning visible.
4. Stop the cloud. Take and bill another order. Restart the cloud. Banner
   clears, order appears in admin.
5. Admin: orders, the received GRN, stock variance.
6. Optional, only if clean three times: fake ONDC order arrives, accepted on the
   till, billed.

### Work items, in order

The authoritative list is `docs/demo-kickoff.md`. Summarised, with the rulings
made on 2026-09-11:

| # | Work | State |
|---|---|---|
| **0** | `apps/captain` — go/no-go | **GO, REDUCED, WITH A CUT-OFF.** See below |
| **0b** | UPI QR for the bill amount on the invoice screen and the PDF receipt | In scope |
| **1** | **Seed parity** — ONE seed source loads identical menu, variants, recipes, inventory, supplier into BOTH cloud and edge. Bootstrap the edge from that clean seed (0035 applies here). **Assert after seeding: zero blocked rows, zero deduction gaps, banner empty** | First work, everything depends on it |
| **2** | Demo seed content — outlet with a real name and GSTIN-shaped placeholder, ~40 items with real prices, ≥10 recipes, opening stock for every ingredient, one supplier, one GRN received. **One command resets cloud + edge to this state** | With item 1 |
| **3** | Receipt — report what the file-sink emits today. If raw ESC/POS, add a rendered receipt (HTML→PDF or plain text) written beside it and opened on print, **same content as the bytes** | |
| **4** | Sync banner readable — full width, order id + item + reason legible from a metre | |
| **5** | Offline tick — if the till is visibly sluggish with the cloud down, widen the pump interval via config for the demo build **and record it** | |
| **6** | Presentability — window title and icon; **no dev labels, raw UUIDs or internal notes** on till, KDS or admin screens; IST timestamps; consistent naming. List every screen the story touches; screenshot each | |
| **7** | `docs/demo-script.md` — the six steps with exact clicks, expected screen and fallback. Include the cloud stop/start commands for step 4 | Tuesday |
| **8** | **Three full rehearsals from a clean reset, each timed.** Each failure fixed or the step cut. Record one clean run | Tuesday |

### Item 0 — `apps/captain`, APPROVED REDUCED SCOPE

**Why it overrides "it is not in the six steps":** the client's mental model is
**device-at-table → hub → kitchen**. Without it the demo shows him what he
already has. That is the purpose; a build that satisfies the words below while
missing it has missed the item.

**Scope, nothing more:**
- `apps/captain`: **pair** (paste token once, `localStorage`), **tables**,
  **menu + cart**, **send**. **No bill screen. No payment. No modifiers unless
  free.**
- POS HTTP listener on `HOLLER_CAPTAIN_BIND_ADDR` serving `dist` + a JSON API;
  **WAITER verifier**; **`device_id` override on create/add**;
  **`make check-seams` mandatory** — the override changes `pub` signatures
  across crates that do not share a cargo workspace.
- **`order.source` stays `POS`.** Not `TABLE_TAB` — that is ADR-025's customer
  tab, M8, and `scripts/check-order-source-drift.mjs` fails the build if
  anything emits it.

**The JSON API is written as a document BEFORE any code**, so the frontend and
the Rust listener can be built in parallel from one agreed shape.

**CUT-OFF: Monday 18:00 IST.**
- **Pass** = a WAITER device enrolled, a real phone on our hotspot places an
  order that reaches the KDS **and** the hub, **three times from a clean
  reset**.
- **Fail** = captain is **cut**, step 1a is removed from the demo script, and
  **no further work on it**.
- Either way, **Tuesday is items 7–8 only**: script, three timed rehearsals,
  then the recording.

Security gate review for a LAN-facing device stays **filed for pilot**; the demo
runs on our own hotspot.

### Facts a builder needs for item 0, already established (2026-09-11)

Established by inspection, not recall — cite the file, not this summary:
- The LAN server is **embedded in the POS process**
  (`apps/pos/src-tauri/src/state.rs:231,722`), not a sidecar.
  `edge/device/src/bin/kds_lan_server.rs` is a second, standalone launcher of
  the same library.
- It is **WebSocket only over a raw `TcpListener` + `tungstenite`** — **no HTTP,
  no static file serving** (`edge/device/src/server.rs`). Default bind
  `0.0.0.0:9310`, plaintext, `HOLLER_LAN_BIND_ADDR`.
- Its only inbound command is **`set_kot_status`**
  (`edge/device/src/contract.rs:165`). **There is no order create or append on
  the LAN surface today.**
- **`WAITER` is already a valid `device.kind`** in the frozen contract
  (`packages/contracts/src/types/identity.ts:203`,
  `backend/internal/outlet/device.go:18`). **No contract change is needed.**
  ADR-025's unbuilt `table_device` kind is the *customer* tab and is NOT this.
- `CachedCredentialVerifier::new(db, expected_device_kind, fallback)` is already
  parameterised; the POS passes `"KDS"` (`state.rs:735`). A second verifier
  passes `"WAITER"`.
- Every operation the captain needs already exists as a plain
  `fn ..._impl(&AppState, …)` with no Tauri coupling: `list_tables_impl`,
  `get_open_table_session_impl` (`commands/tables.rs`); `list_menu_items_impl`
  and friends, `is_available` on the row (`commands/menu.rs`);
  `create_order_impl`, `add_order_item_impl`, `get_order_impl`
  (`commands/orders.rs`); `send_order_to_kitchen_impl`
  (`commands/kitchen.rs:232`).
- **`add_order_item_impl` is already legal through DRAFT / CONFIRMED /
  SENT_TO_KITCHEN / PREPARING** (`#132-A`). Append-after-send is solved, not new
  work.
- `tiny_http` is present in `apps/pos/src-tauri/Cargo.toml` as a
  **dev**-dependency only. Serving `dist` means promoting it to a real
  dependency.
- **The known trap:** `create_order_impl` / `add_order_item_impl` attribute to
  `state.device_id` — the **till**. Without the override every captain order is
  recorded as authored by the till, and that reads correctly on every screen.

### EXCLUDES — demo build

**No new features** beyond item 0's reduced scope and item 0b. In particular:

- **No M6.1 work of any kind** — no Ed25519 signing, no registry subscribe, no
  public HTTPS ingress, no ONDC staging integration.
- **No pilot-only work.** `docs/pilot-readiness.md` is untouched. An item
  appearing on it is not a reason to do it now. That includes **A4, A6 and
  A7** — do not fix them for the demo.
- **No contract change.** Contracts are FROZEN at v0.8.1 and the demo needs
  none.
- **Captain: no bill screen, no payment, no modifiers unless free**, no customer
  tab, no `table_device` principal, no `TABLE_TAB` source.
- **No re-running of any M6 acceptance criterion** and no reconstructing a
  verdict from git history.
- Still out, as they were in M6: reporting depth (M7); multi-outlet and central
  kitchen, `TRANSFER_IN`, goods-in-transit (M8); captain/CRM/loyalty and the
  waiter Flutter app (M9) — **`apps/captain` is a LAN web page, NOT the M9
  Flutter waiter app**; batch and expiry alerting; menu-engineering analytics;
  supplier accounts posting, credit application and payment settlement (M7); **a
  live channel on any aggregator platform**.

### Scope until Wednesday — BINDING, set 2026-09-12

- **No new features.** Nothing lands that is not on the six-step path or on the
  captain / KDS / bill screens.
- **Scenario board: fix only FAILs that sit on the six-step path.** Every other
  FAIL is listed with a fix-or-cut verdict and left alone.
- **Polish pass is PHONE-FIRST**, in this order: captain page, KDS ticket,
  invoice screen, receipt PDF. Every empty state has words, every wait has a
  spinner, no raw ids, no dev labels. Screenshot each.
- **Any interaction over 1s on the phone or the till is a DEMO BLOCKER.**
  Measure captain send -> KDS render, and till -> bill open, and report the
  numbers rather than an impression.
- **The six steps are drafted MONDAY EVENING from the operator's phone runs.**
  Tuesday is rehearsals and the recording only.
- **Report demo blockers only.**

### Standing rules that still bind

- **A milestone does not close until its acceptance evidence is committed to the
  repository. The chat is not the record.**
- **A test invocation reporting zero tests executed is a FAILURE, not a pass.**
  Run test commands through
  `node scripts/assert-tests-ran.mjs <cargo|go|vitest> -- <command>`; never pipe
  a test command through `tail`, which reports `tail`'s status.
- **The backend runs in its OWN WINDOW** via `scripts/dev-up.ps1`, never as a
  session-owned background process. **Step 0 of any run verifies it by PID, not
  by the port answering.**
- **NO TEST OR PROBE MAY START, STOP OR BIND ANYTHING ON 8080, 9310, 9320, THE
  ADMIN PORT (5175) OR THE POS DEV PORT (5173). Scratch ports and scratch
  databases only.** In force from 2026-09-12 until the demo. **The rule is not
  a fixed list — it is every port the operator's own stack uses**; 5175 was
  added after a dev server left running from a screenshot pass was still
  holding it hours later, and the operator met it instead of their own.
  **Before killing anything, check the process START TIME**: on the sweep that
  found that one, three of the four listeners were the operator's, started
  minutes earlier. A test displaced the operator's running backend
  THREE TIMES in one day — the third time through `demo-reset.ps1
  -BackendPort 8099`, which does not reach the backend it launches (the API
  reads `PORT`, which the script never sets), so it bound 8080 regardless.
  **The rule is about the ports, not about intent:** every one of those runs
  was aimed at a scratch directory and a scratch database and still took down
  the live stack, because the port was never part of what "scratch" covered.
  A run that needs a backend starts its own on a scratch port with `PORT` set
  explicitly, against a scratch database, and stops it afterwards.
- **Escalate immediately:** anything that changes a frozen contract, changes what
  a stored number means, crosses the §50.1 authority split, or trades a demo
  step for scope.

### Rebuilding the stack from cold

1. `docker compose up -d postgres redis nats` — **not** `make dev`, and not a
   bare `docker compose up`: the compose file's `backend` service fails to build
   (`go build -o /out/api ./cmd/api` exits 1) and is not used here. Docker
   Desktop does not autostart on this box.
2. Backend in its own window:
   `.\scripts\dev-up.ps1 -SkipInfra -SkipSeed -NoKds -NoPos`, then record the
   **NEW** pid.
3. POS, from a terminal the operator owns; a Tauri window launched from a tool
   with redirected stdio never appears: `.\apps\pos\run-dev.ps1`, sign in
   `cashier@holler.test` / `holler123`.
4. Admin console: `cd apps\admin` then `pnpm dev`, `http://localhost:5175`, sign
   in `owner@holler.test` / `holler123`.

`apps\pos\.env.dev` is deny-ruled to the agent because it carries the edge
encryption key, so **the operator runs anything that needs it**, including the
bootstrap and the device token.

### FUTURE SCOPE, RECORDED SO IT IS NOT READ AS DEMO WORK

**Table-side ordering (customer tab) — ADR-025, PROPOSED 2026-09-10.** Some
outlets will put a tab on each table so customers order directly; others stay
staff-ordered, and **both modes must coexist within one outlet**. The tab is a
**LAN client of the till like the KDS, never a cloud client**; a new
**`table_device` principal kind** enforced at the **LAN boundary, not in the
tab's UI**; `ordering_mode`/`tab_enabled` as cloud config; binding rides the
existing `table_session` with **no new aggregate**; the tab is **append-only**.
**No code exists and none is demo scope.** Landing proposed for **M8**, trigger
*after the first pilot runs on `STAFF_ONLY`*. `apps/captain` is a **staff**
device and does not implement any part of it.

### PARKED — decided, do not re-raise

Both are hardware gates. **Parked 2026-08-20.** A fresh session should read these
as settled, not as open questions, and must not re-litigate them:
- **ESC/POS on paper** — an M3 exit gate. No printer exists in this environment;
  one is being sourced. The file-sink transport proves the byte stream, not that
  a device accepts it. **Demo work item 3 renders a receipt beside those bytes;
  that does not close this gate.**
- **Bare 4GB Windows 10 VM run** — ADR-013. The installer half is done
  (`bundle.windows`, offline WebView2 embed, static CRT, NSIS-only); the VM run
  itself needs a machine nobody has provisioned yet.
  `docs/adr/ADR-013-outlet-deployment-target.md` carries the addendum and the
  named fallback.

### Completed milestones

**M6 Sync gaps, back office, aggregator framework** is **CLOSED and tagged
`m6-complete`** (2026-09-11) at contracts v0.8.1. **Seven of eight criteria
observed on the shipping binaries: C1, C3, C4, C5, C6, C7, C8** — evidence in
`docs/m6-acceptance.md`, handover in `docs/m6-phase-c-boundary.md`. Record it
honestly, in these three parts:
- **M6 C8 is `SHAPE ONLY — no integration evidence`.** Both adapters run against
  fakes we authored, so it proves the contract shape twice and the integration
  zero times. Its integration half travels to M6.1 as an explicitly UNMET row
  (M6.1 C1), carried and never merged into C8.
- **M6 C2 is PARKED and M6 closed WITH it parked, deliberately.** Trigger: *any
  platform sandbox access granted*. It cannot be evidenced from our own logs —
  "never evidence a snooze from our own log" is the criterion's own wording — so
  M6 closed without it rather than with a fake pass.
- **Phase A closed FIVE of seven: A1, A1b, A2, A3, A5 landed; A4, A6, A7
  carried**, each in `docs/backlog.md` and `docs/pilot-readiness.md` with the
  trigger *before the first pilot*. **Never report it as "Phase A complete".**
  A7 is not cosmetic: `edge/sync/src/route.rs` maps only `order` and
  `table_session`, so 78 rows on the live edge database have no route and can
  never be sent (55 `kot`, 22 `stock_count`, 1 `invoice`, measured 2026-09-07).

**Do not re-run any M6 criterion and do not reconstruct a verdict from git
history.**

**M1 Core POS** and **M2 Kitchen** are complete. M2's acceptance item 5 — one real KDS↔edge socket session — **is met**, re-evidenced 4/4 against a real socket after ADR-017. Record it honestly: it stood recorded as met while its test bridge silently failed to **compile** for a period, so the `lan-integration` CI job was failing at `cargo build` and proving no socket session at all (`docs/RESUME.md` §5). The `rust-seams` job and `make check-seams` exist so a tenth such break fails fast.

**M3 Billing** is code-complete and functionally exercised, but **NOT acceptance-complete**: it is untagged and blocked on the two PARKED hardware gates above. `docs/RESUME.md` §2 and §6 carry the corrections. Two M3 defects are filed to M6 rather than fixed here, and a builder should not treat either as settled behaviour: `invoice.business_date` is bucketed by **UTC calendar day** (`business_date_from`, `apps/pos/src-tauri/src/commands/billing.rs`), which splits one trading night across two business dates and can reset a `DAILY` invoice series mid-service; and a `reset_policy` whose prefix lacks a matching date token yields duplicate invoice numbers, caught only by the UNIQUE index. `compute_business_date` (`edge/database/src/deduction/business_date.rs`) is the correct function and the stock ledger already uses it.

**M5 Procurement** is **CLOSED at contracts v0.6.3** — seven of seven criteria
observed on the shipping binaries, evidence in `docs/m5-acceptance.md`. Two
findings outlive the milestone. **An acceptance criterion satisfied by either of
two definitions cannot tell you which one you built** (criterion 7 passes under
both a lifetime and an on-hand cost average, and reports neither). And **a test
condition the environment cannot produce is not a weak test, it is no test** —
every "network disconnected" step since M1 was performed by switching WiFi off
against a cloud at `http://localhost:8080`.

**M4 Inventory & Recipes** is **complete and tagged `m4-complete`** — all seven acceptance criteria observed against the shipping binaries, none evidenced by a test harness. Criterion 1 was CONTESTED for four days and closed by `7e88d1c`: the till hardcoded `variantId: null`, so no sale the POS ever took wrote a ledger row, while the harness that evidenced the criterion selected a variant directly. **A deduction test proves deduction only for the path its caller takes.** Criterion 6 was falsified, not merely observed, and the falsification found a dropped field the 201-echo comparison structurally could not see.
## Response rules for agents
Inspect repo first, output a concise plan, then edit real files. If a task touches >15 files, stop and present the plan instead of proceeding. Report per milestone: Implemented / Verified / Performance / Remaining / Next.

- **A CRITERION A PRE-FIX BINARY ALSO PASSES IS NOT EVIDENCE FOR THE FIX.** Write the falsifier against the ALTERNATIVE EXPLANATIONS, not against the feature. M6 C4's first run satisfied its falsifier word for word — order created offline, app killed, row landed with the window open, operator touching nothing — and was thrown away, because the row appeared seventeen seconds after process start and **the startup drain predates the feature under test**. "The pump landed it" and "the drain that has always run at startup ran" are indistinguishable from that observation. The accepted run excluded the alternative STRUCTURALLY: the POS process stayed up across the whole sequence, so no process start could have occurred between the cloud returning and the row landing. Corollary: the falsifier's own premise can be wrong independently — C4's assumed a normal exit fires `RunEvent::Exit`, and on this build no exit path does.

- **A RECONSTRUCTED PRE-FIX BINARY REPRODUCES THE SHAPE OF A DEFECT, NOT ITS DURATION.** When nothing builds the pre-fix binary, removing the fix by hand is the right move (the C8 planted-branch precedent, never a worktree checkout — this repo has a data-loss incident against that). But **a reconstruction removes a fix; it does not restore the world the fix was written in.** Everything built since is still present and still interacting: removing A2's per-aggregate line reproduced head-of-line blocking for about fifty seconds, because A3's later retry budget puts a ceiling on what was originally unbounded. State the ceiling beside the result or the next reader takes "neighbour stranded" to mean what it meant in M5. Same mechanism, second-order: because the abandoned-row short-circuit runs before the in-tick blocked set, **later rows of a partly-refused aggregate are sent after one of its rows is abandoned** — per-aggregate ordering stops protecting an aggregate at exactly the point it starts failing.

- **NAME THE EXACT CONSTRAINT A FIXTURE DEPENDS ON, AND REPRODUCE IT IN ISOLATION, BEFORE BUILDING A RUN ON IT.** M6 C3's fixture was planned on the cloud's 2-row menu seed against the edge's 43, and **the run falsified that assumption**: an order for an item the cloud DOES hold was refused anyway, because the real key is `order_item_variant_id_fkey` — one variant row cloud-side against one per item on the edge. One rolled-back `INSERT` against Postgres would have named it in the first minute. Two consequences worth carrying: the mis-stated cause also mis-specified the PROBE for two hours ("is the order in Postgres" cannot see a refused item event, because `order` has no FK to `menu_item` and the create lands either way); and **a fix for one defect can uncover another with nothing announcing it** — before `7e88d1c` the till hardcoded `variantId: null`, which satisfied that FK trivially, so fixing M4's criterion-1 defect started sending real variants to a cloud that had none.

- **A CHECK IS FALSIFIED BY WHAT IT LETS THROUGH, NOT BY WHAT IT CATCHES.** Watching a check go red when it is written proves it can fire; it says nothing about the region it never looks at. The aggregator boundary check had been watched RED on a planted branch AND had found a real leak on its first run, and still had two holes, both found by planting a value and watching it **pass**: `packages/contracts` was outside its search roots and `.sql` outside its extensions, so the shared schema — where a platform name binds every consumer at once — was the one place it could not see; and `\b(ondc)\b` cannot match `AGGREGATOR_ONDC`, because `_` is a word character, so an enum member named for a platform, the likeliest form by far, went straight through. Either hole alone would have let it land in both stores with the build green. When a check then needs an exemption, bind it to its removal trigger in the same commit: **an exemption that outlives its reason is a silenced failure.**

- **To prove a UI-level concern is covered, enumerate the SINKS, not the surfaces.** A screen can be missed; a write path cannot. "Which screens take a quantity?" is a search over a list nobody maintains, answered by recall and confirmation bias. "What command accepts a human quantity, and what writes `stock_ledger_entry`?" is a search over a closed set the code already enforces. Confirming the M4 quantity-echo fix went that way: two Tauri commands accept a quantity, exactly one non-test `INSERT INTO stock_ledger_entry` exists, four origins reach it (`RECIPE`, `MODIFIER_DELTA`, `WASTAGE`, `COUNT_ADJUSTMENT`) — so there is no third entry screen. The same enumeration proved *where an incident came from*: `devseed.rs` writes no ledger rows at all, so a stocked item can only have been stocked by a count. Applies unchanged to permission checks, audit writes, print paths and sync emitters — anywhere the question is "have I found all of them".

- **A SUITE THAT RUNS NOTHING MUST BE AS LOUD AS A SUITE THAT FAILS. REPORT THE COUNT OF TESTS EXECUTED, NEVER "PASSED".** Zero executed tests and a green run are indistinguishable from the outside, and every runner here produces that state on a typo: `cargo test <filter>` prints `0 passed; N filtered out` and exits 0, `go test -run <typo>` prints `ok pkg [no tests to run]` and exits 0, `vitest run -t <typo>` skips all of them and exits 0 — all three confirmed by experiment, 2026-09-02. So does a command that never ran at all — a `-p` package outside any workspace, a missing manifest, a tool absent from PATH — the moment its exit status is swallowed, and **`cmd 2>&1 | tail -40` swallows it every time**, because a pipeline reports `tail`'s status. Run test commands through `node scripts/assert-tests-ran.mjs <cargo|go|vitest> -- <command>`, which fails the job on zero executions and is wired into every test step in `ci.yml`. Related but distinct guards, each for a different way to run nothing: `scripts/check-gated-tests.mjs` (a `required-features` target is not built, not run, and **not reported as skipped**) and CI's `assert no silent skips` (an unset `HOLLER_TEST_DATABASE_URL` turned every Postgres-backed test into a skip and printed `ok` for twelve packages).

- **A MILESTONE DOES NOT CLOSE UNTIL ITS ACCEPTANCE EVIDENCE IS COMMITTED TO THE REPOSITORY. THE CHAT IS NOT THE RECORD.** Every criterion needs a committed file naming what was observed, how the precondition was established and verified, who observed it, and on what date — `docs/m5-acceptance.md` is the template. A verdict that exists only in a session transcript is erased by a restart, and what replaces it is a reconstruction from git history stated with the confidence of a read: M5 criteria 1, 3, 4 and 6 were all observed on real screens and were then reported as unobserved by the next session, which was holding the commit made *because* of the run that observed them. **Same family as a test whose subject nothing else constructs — the fact existed, the record of it did not.** Corollaries: cite the artefact (screen, row, request log, PID) not the conversation; when two reports of the same run disagree, record the contradiction as UNRESOLVED with the query that settles it rather than picking one; and a criterion is not closed by an agent's summary of a run, only by evidence a later session can re-read.

- **A RESTART IS VERIFIED BY THE NEW PROCESS'S IDENTITY — PID or start time — NEVER BY THE PORT ANSWERING.** The old process answers identically, so "the health check passes" is consistent with "nothing restarted". This has already cost a debugging detour: a backend restart failed to bind 8080 while the original kept serving, so an in-memory rate-limit window that the restart was meant to clear persisted, and the unchanged symptom read as a credential fault. Kill by port, confirm the port is free, start, then confirm a NEW pid. Same shape as the ADR-020 post-seal drain and the 0.5.2 auto-fill guard: **the action reports success while doing nothing, and it reads correctly in review.** Applies to every service restarted in this project.

## Commit rules
- **Stage only the paths you claim to have changed.** `git add -A` and `git add .` sweep in whatever else is loose — a commit once swallowed fourteen unrelated untracked files, and the amend that removed them staged deletions of files that were already tracked. Name the paths, then check `git show --stat` before you move on. This rule came from the worktree data-loss incident and was written for builders; it applies to the orchestrator identically.
- Always commit with `git commit -s`. This appends a `Signed-off-by:` trailer taken from the repo's `user.name`/`user.email`.
- Never add a `Co-Authored-By: Claude ...` trailer or a "Generated with Claude Code" footer. These were stripped from history and are disabled via `includeCoAuthoredBy: false`; sign-off replaces them rather than sitting alongside them.
- **PUSH AFTER EVERY COMMIT.** Not at the end of a track, not when the work looks finished — every commit. **The reflog is not a backup:** one `gc`, one crash, one machine that has already crashed twice this month, and an unpushed commit is gone with no trace that it existed. A blind push still protects the work, so an unreadable CI verdict is not a reason to hold it. 4,100 lines of landed procurement code survived a `git reset HEAD~1` on 2026-08-29 only because the working tree happened to still hold them.
- **BUILDERS STAGE AND COMMIT. NOTHING ELSE.** Every history-rewriting git command is DENIED in `.claude/settings.json` — `reset`, `rebase`, `commit --amend`, `checkout --`, `restore`, `push --force`, `clean`, `branch -D`, `stash drop`/`clear`, `gc`. Three git-destruction incidents share one family: the worktree stale-base loss, the verifier's `checkout --`, and an agent's `git reset HEAD~1` that discarded a **parallel agent's** commit rather than its own. Denial is the same move as denying the sandbox toggle: the instruction was already written down and was followed right up until it wasn't. If history genuinely needs rewriting, that is an orchestrator decision made with a human, never a builder recovering from its own mistake.

## Contract & constraint review rubric
Before proposing any contract change or .claude/ edit, self-review against:
- IDs: app-generated UUIDv7/ULID per §74 — never DB-side random defaults
- No nullable columns in primary keys
- Every aggregate single-authority per §50.1 — no split-authority columns; split the aggregate instead
- No credential material (hashes, tokens) in audit values, logs, or wire types
- Uniqueness constraints tenant-scoped, not global
- Additive changes to frozen contracts require version bump + ADR
Present the proposal WITH your self-review findings, then wait for approval.
