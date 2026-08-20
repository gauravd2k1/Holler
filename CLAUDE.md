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
- Dev machine here: Windows laptop (i7-9750H, 16GB, GTX1050). Cap concurrent agent sessions at 3.
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

## Contracts status: FROZEN at v0.5.0 (Milestone 4 inventory + recipes applied; migrations through sqlite 0018 / postgres 0019)
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

v0.5.0 also closed four M2/M3-era defects that shared one shape — structural guarantees written as comments and enforced on at most one side: UTC business-date bucketing (`outlet.day_start_time` now defines it), and missing append-only enforcement on `payment` (PostgreSQL), `audit_event` and `cash_movement`. `invoice` gained real immutability too: every column frozen, one legal `ISSUED→CANCELLED` transition. Three lints in `edge/database/src/migrations.rs` hold that ground — every APPEND-ONLY/IMMUTABLE claim must have a trigger, every single-store migration must be declared with a reason, and `DEFAULT gen_random_uuid()` may only decrease.

Two cross-cutting rules the 0.4.x line established the hard way: contract-shaped changes cascade across crates that do not share a cargo workspace (see `docs/retro.md` 2026-08-15), so run `make check-seams` after changing any `pub` signature in `edge/` or `apps/pos/src-tauri`; and a migration that exists on disk but is absent from `edge/database/src/migrations.rs`'s `MIGRATIONS` list **never applies** — 0009–0011 sat dead for exactly that reason, and 0005 before them.

## Current milestone: MILESTONE 2 — Kitchen
Scope: KOT, station routing, printer abstraction, KDS, LAN realtime delivery, order status — all built against the frozen `packages/contracts/` shapes.

Acceptance — every item is an observed behaviour, not an implemented API. None of these count as met by a passing unit suite, and **none may be evidenced by a test harness**: an acceptance run exercises the binaries that ship. If the only thing that starts a component is a test, that component is not wired, whatever its tests say (`docs/retro.md`, 2026-08-11 — this has now happened twice).
1. ~~POS → kitchen propagation below target latency on LAN~~ **MET 2026-08-12**: 150–183ms across multiple sends, POS on the laptop → KDS on a phone over real WiFi, against the <250ms target (`docs/spec/kitchen.md`). Status round-trip confirmed in the same session.
   Note the margin honestly: the e2e harness measures P50 13ms / P95 24ms over a real TCP socket on one machine, so **real WiFi adds roughly 140ms** — the pass is genuine but the headroom is ~30%, not an order of magnitude. A busier network, more screens, or weaker hardware could erode it, so this is a criterion to re-measure at an outlet rather than treat as settled.
2. **Crash mid-order → the cart survives.** Kill the POS with lines in the cart and reopen: the in-progress order is still there. An API capable of preventing the loss does not count; the loss not happening counts (see `docs/retro.md`, 2026-08-10).
3. Cloud sync round-trip: an order and its KOTs created at the edge reach the cloud and read back correctly.
4. The `HOLLER_TEST_DATABASE_URL` suites actually execute — including T7's `TestBuildRouter_SyncConfigEndToEnd`. These have never run; a skip is not a pass.
5. One real KDS↔edge socket session: the Rust server and the TypeScript client connected to each other, a ticket appearing and transitioning. Both ends are currently tested only against their own fakes.
6. Offline login from synced credentials — a cashier authenticating against users pulled through `/sync/config`, **not** dev-seeded data.

**EXCLUDES:** aggregator KOTs, expo screen polish, label printers, waiter app.

Milestone 1 is complete. Items consciously deferred out of it are in `docs/backlog-m2.md`, including a gate that must clear before M2 ships: nothing has ever been built or run on the bare Windows 10 target ADR-013 specifies.

Note: the pre-0.5 placeholder migrations under `backend/migrations/` are gone — `packages/contracts/postgres/` is the sole schema source, and `postgres.Migrate` globs every `*.sql` there, so a new contract migration needs no backend wiring.

## Response rules for agents
Inspect repo first, output a concise plan, then edit real files. If a task touches >15 files, stop and present the plan instead of proceeding. Report per milestone: Implemented / Verified / Performance / Remaining / Next.

## Commit rules
- Always commit with `git commit -s`. This appends a `Signed-off-by:` trailer taken from the repo's `user.name`/`user.email`.
- Never add a `Co-Authored-By: Claude ...` trailer or a "Generated with Claude Code" footer. These were stripped from history and are disabled via `includeCoAuthoredBy: false`; sign-off replaces them rather than sitting alongside them.

## Contract & constraint review rubric
Before proposing any contract change or .claude/ edit, self-review against:
- IDs: app-generated UUIDv7/ULID per §74 — never DB-side random defaults
- No nullable columns in primary keys
- Every aggregate single-authority per §50.1 — no split-authority columns; split the aggregate instead
- No credential material (hashes, tokens) in audit values, logs, or wire types
- Uniqueness constraints tenant-scoped, not global
- Additive changes to frozen contracts require version bump + ADR
Present the proposal WITH your self-review findings, then wait for approval.
