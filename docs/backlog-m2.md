# Milestone 2 backlog

Items deferred from Milestone 1 with a decision behind each. This is not a wish list — everything here was found by a verification pass or an implementation and consciously postponed rather than forgotten.

---

## Planning inputs
Review docs/competitive.md during M2 planning — one open decision (waiter app
milestone) and four spec additions already filed to their landing milestones.

**For Milestone 3, see `docs/m3-planning.md`** — device enrollment as first
track (pilot blocker), Tracks A and B folded into the M3 graph with B ahead of
any billing math, and UI polish deferred post-M3.

## Gating — must clear before M2 ships

### Clean Windows 10 VM validation (ADR-013)
ADR-013 fixes the outlet target as bare Windows 10, 64-bit, 4GB RAM, spinning disk. **Nothing has ever been built or run on such a machine.** Until it has, "runs on Windows 10" is design intent, not verified fact — the ADR says so explicitly, and this entry exists to convert one into the other.

Validate on a VM with **no developer tooling, no preinstalled WebView2, no internet during install**:
- The POS installer completes offline (it must embed the WebView2 and VC++ runtimes, not download them — the current `tauri.conf.json` has no `bundle.windows` section, so it defaults to downloading the bootstrapper and would fail here).
- The app launches and a cashier can log in and create an order with the network disconnected.
- The encrypted-at-rest database opens, and the crash-recovery path works after a hard power cut (pull the VM's plug, not a clean shutdown).
- Measure open/decrypt time on a spinning disk — the crate decrypts the whole database to a working file on open, which is the design's known cost.

M2 ships kitchen features to this same target, so validating the target before adding to it is the cheaper order.

---

## Deferred with a trigger (filed 2026-08-20, T0)

- **MSI/WiX installer — dropped, not failed.** `bundle.targets` is `["nsis"]`. The MSI target was never verified end to end (the WiX toolchain download timed out twice), and two half-verified installers are worse than one verified one. **Trigger: a deployment that requires MSI specifically** — Group Policy / SCCM push, or an enterprise customer whose IT will not run an NSIS executable. It returns with its own clean-VM verification, not by flipping `targets` back to `"all"`.

- **`DEFAULT gen_random_uuid()` retrofit — 8 columns, all in `packages/contracts/postgres/0001_init.sql`.** §74 requires app-generated UUIDv7/ULID and the contract rubric forbids DB-side random defaults. These predate the rubric. They are not cosmetic: a DB-side default is a second id authority, it mints UUIDv4 (unsorted, indexing badly on exactly the tables that grow fastest), and on an edge-authoritative row it is actively wrong, because the id is minted at the outlet and replayed. **Trigger: the next migration that touches each table** — one `ALTER ... DROP DEFAULT` per table, and the application already supplies every id, so nothing else changes. Waiting is safe because the debt cannot grow while it waits: `postgres_db_side_uuid_defaults_only_ever_decrease` (`edge/database/src/migrations.rs`) is a ratchet that fails if the count rises **and** fails if the baseline is stale after a retrofit.

- **Cross-tenant menu isolation rests on per-query convention, not the database. Trigger: M8, OR BEFORE A SECOND PRODUCTION TENANT, whichever comes first.** Confirmed 2026-08-20: there is **no** `ROW LEVEL SECURITY` and **no** `CREATE POLICY` anywhere in `packages/contracts/postgres/`. Isolation for the whole menu tree — `menu_item`, `menu_item_variant`, `menu_item_modifier`, and now `recipe`, `recipe_ingredient`, `modifier_ingredient_delta` — is a join every query must remember to write.

  **The milestone framing was wrong and is corrected here.** The blast radius is multi-**tenant**, not multi-outlet, and the product is multi-tenant *today*: one forgotten join leaks a menu now. M8 only adds more queries that could forget. So the trigger is the second production tenant, whichever side of M8 that falls.

  This is why `recipe` deliberately does **not** carry its own `tenant_id` (ADR-018 §2): a lone tenant column no constraint forces to agree with the tree is a second answer to "whose row is this". The fix is RLS on the tree, not a column on one table.

  **The documentation is accurate.** ADR-006 says isolation is "enforced at the application/query layer" via "repository-layer conventions"; §57 of the master prompt and `SYSTEM_ARCHITECTURE.md` are equally careful. Nothing in the repo oversells this.

  **The hole is a missing test, in exactly the wrong context.** ADR-006 promises "automated cross-tenant access tests", and they exist for `ordering`, `outlet`, `kitchen`, `tenant` and `cmd/api` (`TestPostgresRepository_CrossTenantOrderLookupIsNotFound`, `TestSyncConfig_CrossTenantOutletIsNotFound`, `TestEnrollDevice_RejectsCrossTenantOutlet`, and more). **`backend/internal/menu` has four test files and not one cross-tenant test** — the single context whose tree every recipe hangs off.

  **SCOPE SPLIT (2026-08-20).** This entry covers the **retrofit** only: cross-tenant tests for the pre-existing menu tables (`menu_item`, `menu_item_variant`, `menu_item_modifier`), which is M1-era code and stays filed against the trigger above.

  **M4's own three tables are not deferred.** `recipe`, `recipe_ingredient` and `modifier_ingredient_delta` ship cross-tenant tests **in T4**, as part of the track that writes them. Adding three tables to an untested isolation boundary is precisely how the boundary stays untested — the new code does not get to inherit the old code's exemption.

- ~~**Four structural guarantees written as comments and enforced nowhere.**~~ **ALL FIXED at contracts 0.5.0.** `payment` (PostgreSQL had the words and no trigger since 0.4.5), `audit_event` ("Local append-only audit" — an audit log that can be edited is not an audit log, and it is the table you reach for precisely when you suspect an edit), `cash_movement`, and `invoice`.

  `invoice` was nearly closed the wrong way: an earlier draft proposed rewording its comment, on the grounds that a mutable `ISSUED→CANCELLED` status makes blanket immutability impossible. It got the `stock_count` treatment instead — **every column immutable, exactly one legal transition** — because without it anyone with a psql prompt could change an invoice total, which is a worse exposure than the `payment` one: an invoice is the document handed to the customer and filed with GST.

  Enforcement: `sqlite/0009`, `sqlite/0018`, `postgres/0018`, `postgres/0019`. Held by two lints, both watched to fail first: `every_append_only_claim_has_a_trigger_behind_it` and `invoice_immutability_trigger_covers_every_column`. `UNENFORCED_IMMUTABILITY_CLAIMS` is now **empty and must stay empty** — an entry in it is a claim the schema makes and does not keep.

  Kept here rather than deleted because the fix does not retire the record: this is the finding about how M2/M3 were verified, recorded as one correction in `docs/RESUME.md` §2.

- **P1 — `printer_role` never reaches an edge from the cloud, so no real outlet can print a bill. SCHEDULED IN T4 (M4), not deferred** — see `docs/m4-planning.md` §2.6, which also adds the guard this second instance earns.** Found 2026-08-21 by `scripts/check-openapi-go-drift.mjs` on its first run, which flagged the missing OpenAPI schema; the delivery gap turned up on the next look. Contracts 0.4.7 added `printer_role` to SQLite, PostgreSQL, Go and TypeScript — and `syncConfigResponse` (`backend/cmd/api/syncconfig.go:134`) has sixteen fields and **no `printer_roles`**. A grep for `printer_roles` across `backend/` and `packages/contracts/go` returns nothing; the only writers are `edge/database/src/repo.rs` and `devseed`.

  So an outlet that syncs from the cloud receives **zero** printer roles. Since a printer with no role row is a candidate for neither path — deliberately, so absence is never read as consent — `print_invoice` fails loudly by name at every such outlet. It works in development **only** because `devseed` writes the roles locally.

  Exactly the shape of the `/sync/config` empty-`users` defect already filed above: the contract shape exists, the delivery path does not, and nothing fails until a human tries to use the product. Closing it needs a `printer_roles` field on the bundle, a kitchen-context export to populate it, and the edge applying it — **and the OpenAPI field must land with the implementation, not before it**, or the spec acquires exactly the kind of claim nothing verifies that this week's work has been about removing.

- **P1 — `outlet.day_start_time` is read and never written, by anything.** Contracts 0.5.1 added the column and defined `business_date` in terms of it; `repo::upsert_outlet`'s column list never included it, `edge/sync`'s config apply does not set it, and `syncConfigResponse` does not carry it. So **every outlet is pinned to the schema default `'00:00'` forever, whatever the cloud says** — and an outlet trading past midnight still mis-buckets its business day, which is the defect 0.5.1 existed to fix. Found by T2 while making the timezone infallible; it declared the gap in `DayStartTime`'s doc comment rather than leaving it unstated.

  **Severity, stated precisely:** half of 0.5.1 works. `outlet.timezone` IS written, so `business_date` is the correct outlet-local calendar date and the original defect — dinner service booking to the previous day — is genuinely fixed. What is broken is only the configurable cutoff: an outlet wanting a 4am boundary cannot have one. **An unusable config knob, not live wrong data.**

  **Third instance of "the contract shape exists, the delivery path does not"**, after empty `users` and `printer_role` — and the *second* instance of the additive-consumer-list rule, landed one version before the change that prompted the rule. Note this one would slip past T4's planned guard as specified: `day_start_time` is a **column on `outlet`**, not an aggregate, so "every config aggregate the edge needs appears in `syncConfigResponse`" does not reach it. **The guard must cover config fields the edge reads, not only aggregates.**

- **Three `unwrap_or`-on-a-parse defects in `edge/`, found by a directed sweep.** Each substitutes a plausible valid value for an invalid stored one, silently — the shape ruled worse than a panic, because a panic is visible and plausible-but-wrong data is not.
  - `edge/printer/src/adapter.rs:314` — a malformed `printer.connection_kind` silently becomes `Network`, which can **misroute a print job to the wrong transport**. The most serious of the three. **Retriggered from "next touch" to the hardware run (~2 weeks):** the first test against a real printer must exercise the real transport path, not a silent fallback that happens to work because the dev printer is a network one.
  - `edge/device/src/server.rs:461` — an unparseable KOT status is silently treated as not-terminal, so a malformed ticket stays in the live KDS snapshot instead of being flagged.
  - `edge/database/src/invoice/numbering.rs:145` — a negative stored `padding_width` silently becomes 6. A numeric conversion rather than a parse, but the same invalid-config-to-silently-substituted-valid-value shape.

  Reported by T2 under instruction to report and not fix, so each stays a one-line change with a test rather than an unreviewed drive-by. **Trigger: the next task that touches each crate.**

- **`chrono-tz`: do NOT narrow it with `filter-by-regex`. Decided 2026-08-21, recorded so nobody re-proposes it.** The embedded IANA database costs 1-2MB against a 209MB installer, which is noise. The real objection is behavioural: `filter-by-regex = "Asia/"` makes a **valid IANA name fail to parse because of a build flag**, and since 0.5.3 `OutletTimezone::parse` treats a parse failure as a hard rejection at config apply. A build-time size optimisation would become a runtime config rejection for any outlet outside the filter — silent at build, loud at the outlet, which is the coupling this milestone has spent its time removing.

- **`TestBuildRouter_SyncConfigEndToEnd` uses a fixed user id and fails on any repeat run against a persistent database** (`router_integration_test.go:130`, duplicate key on `app_user_pkey`). The exact fixture class `1cc087c` already fixed across auth/kitchen/ordering — mint ids per run. Found blocking T4's full-suite verification; confirmed pre-existing by diff. **Trigger: next touch of `cmd/api` tests.**

- **Config deletion is unrepresentable across every delta-synced join-table family.** Found by T4b/T4c: every `*Since` export ships per-row deltas gated on `config_version`, and absence of a row in a delta means "unchanged", so a cloud-side deletion (a role revoked, a station un-routed, an item un-snoozed from a station) NEVER reaches an edge. Consistent across `printer_role`, `station_printer`, `menu_item_station` and the rest — a systemic property of the sync model, not one family's bug. Needs a design pass (tombstones, or periodic full-set reconciliation) with an ADR. **Trigger: M5 planning, or the first pilot config change that revokes anything, whichever comes first.** Related: the `compliance_versions`-empty-while-invoices-exist warning is an `eprintln!` to stderr — same visibility problem as the M2 print-failure lesson; fold into T5/T6 error surfacing if cheap.

- **Air-gapped build machine.** The WebView2 offline package is downloaded from `go.microsoft.com` at *build* time and embedded into the installer. Install-time is offline, which is what ADR-013 requires and what is proven. **Trigger: a build environment without egress** — a customer-hosted or regulated build. The fix is vendoring the runtime package into the repo or an internal artefact store.

---

## Correctness and hardening

- **P0 REGRESSION — order type and table lock on the first item tap, and dine-in can never reach the kitchen.** Found in the first manual run on real hardware, not by any test.
  `PosScreen.tsx:83` sets `canEditOrderShape = orderId === null`, and the durable-cart change (`4b0c560`) creates the DRAFT order on the **first item added**. So one tap locks both the order-type buttons and the table dropdown. For `DINE_IN` that is terminal: `canSendOrder` requires a `table_id`, the dropdown can no longer set one, and Send stays disabled permanently — a dine-in order cannot be sent to the kitchen at all. It also reads as "TAKEAWAY/DELIVERY are disabled", which is the same bug seen from the other side.
  The comment at `PosScreen.tsx:81` states the constraint honestly ("no command to amend them on an already-created order") — the defect is that the lock was acceptable when the order was created at Send, and became a blocker when creation moved to the first tap. Two correct changes composed into a broken one.
  **Fix is an `update_order_shape` on a DRAFT order** (`order_type`, `table_id`) in `edge/database`, a Tauri command, and unlocking the controls while the order is DRAFT. Do not fix it by deferring order creation — that would undo crash durability.
  Neither the durable-cart tests nor its verification caught it: nothing exercises "add an item, then change the order type", and each change reads correctly in isolation.

- **P1 — A mixed order sends silently when one line has no station.** Found by the e2e scenario harness on ~30% of randomized runs; confirmed in product code by verification, not just by the harness.
  `edge/database/src/lib.rs:528-531` silently `continue`s past a line with no station route, while the guard at `542-546` only errors when **every** line is unrouted. So an order of three routable items plus one unrouted item sends "successfully", the unrouted line never reaches a kitchen, and nothing anywhere says so. The all-unrouted case errors correctly, which is why this looked covered.
  A cashier is told the order went to the kitchen. One dish never appears. This is the same shape as the print-visibility defect below — the failure is real, the signal is absent.
  Composition-dependent, so no scripted test would have found it; it took randomized mixing.

- **P2 — the e2e harness's CI job cannot go red on an invariant failure.** `tests/e2e-scenario/orchestrator/src/scenario.test.ts:32` asserts only that no *harness-level* fatal error occurred; invariant failures are deliberately not asserted, because known product defects (the two above) would otherwise make the job permanently red.
  Defensible today and honestly documented in the file. But it means the CI job is a smoke test for the harness, **not a regression gate on the product** — a newly introduced invariant violation would pass CI silently, which is the precise failure mode this harness was built to end.
  Fix when the known defects close: assert per-invariant pass counts, or baseline the current failures so any *new* one goes red. Do not leave it asserting only fatals once there is nothing left to excuse.

- **P1 — A KOT that can never be queued for print is invisible to staff.** Found in the first successful send-to-kitchen run on real hardware: four KOTs logged `failed to queue KOT <id> for print: no active printer routed for station MAIN_KITCHEN` to stderr, and **nothing surfaced in the UI**.
  `apps/pos/src-tauri/src/commands/kitchen.rs:136` only `eprintln!`s the error. Because no printer is routed, `queue_kot_for_print` fails *before* inserting a `print_job` row — so `spool::list_failed_jobs` returns nothing and `PrintFailureBanner` renders nothing. A cashier gets no indication that four tickets never printed.
  This is not the spool being broken: T2's retry/visibility path works correctly for jobs that were enqueued and then failed. The hole is jobs that could **never** be enqueued, whose most common real cause is precisely this one — a station with no printer configured. `docs/spec/hardware-printing.md` requires print failures be visible to staff, and a stderr line in a GUI process satisfies nobody.
  Fix shape: either insert a `print_job` in a FAILED state carrying the routing error so the existing banner picks it up, or surface routing gaps as their own visible condition. Do not fix it by silencing the log.

- **P2 — `devseed` seeds no printer or `station_printer` row.** So the entire print path is unexercised on any dev machine, which is why the P1 above went unnoticed until the first real send. Adding a dev printer (a network one pointed at a dead address is fine, and arguably better — it exercises the retry/failure path rather than a happy path nobody has) would make the spool reachable in development.

- **P1 — No quantity control on a cart line. Confirmed in the wild.** Tapping the same menu item twice produces two separate lines of quantity 1, not one line of quantity 2. The first manual run on real hardware produced a five-line stack of Masala Chai where the cashier wanted quantity 5.
  Originally found by the T9 verifier while judging an unrelated disclosure — the builder described a remove-then-add quantity path, and the verifier could not find it because *no quantity adjustment exists at all*. Filed then as "wrong for a POS" on inspection; raised now because it is observed behaviour on the shop-floor path, not a design opinion.
  Not a regression and not a correctness bug — the order total is right either way. But closing it needs an `update_order_item_quantity` in `edge/database`: **do not implement it as remove-then-add**, because that is two separate durable writes with a window between them where a crash loses the line, which is precisely the failure the cart-persistence work exists to eliminate.

- ~~**POS cart persistence.**~~ **CLOSED at M2** — `4b0c560`. Lines are durable as the cashier taps, startup recovers the active draft, and the parallel `create_order` path is gone. Verified against the crash rather than the API: the test drops the whole `Db`/`AppState` with no graceful close, reopens a second independent database against the same sealed file, and asserts the read-back quantities, prices and notes. The history of why this took two attempts is in `docs/retro.md`, 2026-08-10.

  <details><summary>Original entry (kept for the retro's sake)</summary>

  **POS cart persistence — STILL OPEN, deliberately not closed at M2.** The Tauri layer was wired and tested at M2 (`add_order_item`/`remove_order_item` no longer return `UNSUPPORTED_DB_OPERATION`), but **no screen calls them**: the cart still round-trips through a single atomic `create_order`, so **a crash mid-order still loses the cart**. Confirmed by the T5 verification pass, not assumed.
  This entry was kept open on purpose as a record of a real distinction: the item's *wording* ("wire them through") was satisfied while its *purpose* — that the shop floor never loses in-progress work — was not. Closing it needed an in-cart-edit UI writing each line to a persisted DRAFT order as the cashier taps. Judge it against the crash, not against the API surface.

  </details>
- **`dto::MenuItem.schema_version`.** The Rust DTO omits it, so the POS injects the constant before Zod-parsing — a shim at the exact boundary meant to catch drift. The verifier confirmed the spread order makes it non-masking, but one field on the Rust struct fixes it properly.
- **`ReplayTransition` version reuse.** Treats `version <= stored` as a duplicate and silently returns current state. Correct under §50.1's single-writer monotonic versioning; becomes a silent-drop risk the moment multi-device edges exist (a waiter tablet and a POS transitioning one table). Needs an ADR before M2 introduces more writers.
- **`ON CONFLICT (id) DO NOTHING` in ordering.** Correct idempotency for identical replays, but silently no-ops if a different payload reuses an existing id, masking a content mismatch instead of surfacing it.
- **Menu category read command.** `list_menu_items` is the only menu read surfaced, so the POS renders categories by raw UUID. `edge/database` now has the list functions; they need Tauri commands.

## Security

- **Redis-backed rate limiter.** The login limiter is in-memory: it does not survive restart and does not share budget across instances. The verifier judged this materially different from the refresh-store violation — it degrades a defence-in-depth mitigation rather than disabling reuse detection — so it was acceptable for a single-instance M1 backend. ADR-012 names Redis as the intended store. Wire it before any multi-instance or production rollout. The fixed-window implementation also permits a 2× burst across a window boundary.
- **Edge DB key provisioning.** `HOLLER_DB_KEY_HEX` supplies the encryption key via environment variable. Fail-fast on absence is right, but an env-var key is weak key management for something ADR-011 calls encryption at rest. Wants an OS keystore or TPM.
- **Device enrollment — HARD TRIGGER: blocks any pilot deployment.** No flow exists anywhere. Three separate holes are the same missing mechanism, and closing it must close all three together:
  1. **Edge sync worker.** `tenant_id` and `device_id` are supplied at worker construction with nothing to verify them against, so a mis-enrolled node would silently mislabel every outbound envelope.
  2. **`/sync/config`.** The one route carrying Argon2id hashes is gated on an ordinary human bearer token with `user.manage` — an enrolled edge node and a logged-in browser session are indistinguishable to the backend. Confirmed by grep across `backend/`: no device token, no certificate, no enrollment path exists. The frozen OpenAPI description for `EdgeUserCacheEntry` already claims delivery is "only over TLS, only to an enrolled edge node"; two thirds of that sentence is currently aspirational.
  3. **KDS LAN port** (`edge/device`). The handshake verifies only that the `device_id`/`outlet_id` pair matches a registered device row, and there is no TLS. Device ids are UUIDs, not secrets, and they travel in the WebSocket query string where they land in any proxy or access log. On a restaurant LAN without VLAN segmentation — common — anyone who captures or guesses one id can drive `set_kot_status` for that outlet: marking food SERVED (the kitchen believes a dish left when it did not) or CANCELLED (killing a live ticket), with no server-side signal distinguishing them from the real screen.

  Defensible while this is a dev-only build. **Not shippable to a real restaurant**, including a single-outlet pilot. Minimum close: a per-device enrolled credential presented on both the cloud sync path and the LAN handshake, plus network-segmentation guidance in the outlet setup documentation. `lan.ts` reserves an optional `device_token` handshake param so this lands without a further contract shape change.

  ~~**When `device_token` verification turns on, move it out of the query string**~~ — **CLOSED, and this entry was stale.** Verified 2026-08-20 during M4 planning: `edge/device/src/server.rs` takes only `outlet_id`/`device_id` from the query string and serves nothing until a first-frame `{"type":"auth","device_token":…}` is verified (`server.rs:35,56,271`); `apps/kds/src/lib/lanClient.ts:61` sends it as the first frame, and both `lanConfig.test.ts:105` and `smoke.spec.ts:50` assert the token never appears in the URL. It landed with ADR-017. The original reasoning is kept below because it is still the right rule for the next secret that needs a transport.

  Original text: an `Authorization` header, or a first-frame auth message before the snapshot. The transport comment in `lan.ts` names query-string logging as the hazard, and it applies with full force the moment the value stops being worthless: a secret in a query string is a secret in every proxy and access log on the path. Query-string carriage is acceptable *only* while the token is unverified and therefore carries no authority. Closing enrollment without also moving the parameter would convert a documented non-secret into an undocumented leaked secret.
- **SQLCipher.** Page-level encryption was unavailable because the vendored OpenSSL build needs a Perl toolchain absent from this environment. The current AES-256-GCM sealing leaves a plaintext working file for the process lifetime. Provisioning the toolchain would let a follow-up swap it behind the crate's existing API — but per ADR-013 the *outlet install* must stay free of any such requirement.

## Contracts

- ~~**`ItemRemoved` is unroutable.**~~ **CLOSED at contracts 0.3.0 (ADR-014 §5)** — `DELETE /orders/{id}/items/{itemId}` now accepts it, envelope-wrapped like every other edge→cloud replay. `KOTCreated` had the same hole and got `POST /orders/{id}/kots` in the same change.
  **The underlying limitation is still open:** the drift check verifies a literal *appears* in Rust, not that the event is *deliverable*. Both holes were found by reading, not by the check. Cross-referencing frozen event types against OpenAPI routes — or generating the Rust binding rather than grepping for one — remains unbuilt.
- **Rust binding for `packages/contracts`.** Deferred until a fourth Rust consumer. Until then `scripts/check-event-type-drift.mjs` greps literals in both directions. That check had a real bug — it omitted `edge/database`, the crate that actually builds outbox payloads — which is the argument for generating a binding rather than grepping for one.
- **Deferred `CanonicalOrder` columns.** `packaging_paise`, `delivery_charge_paise`, `aggregator_discount_paise`, `merchant_discount_paise`, `customer`, `delivery_address`, `rider` land in M6. ~~`preparation_time_minutes` in M2~~ — **its column landed at 0.3.0 (ADR-014)** in both stores; the round-trip pin moves from the synthesized `NULL` to the column, which the edge track must update when it starts persisting a value. The M6 fields are still synthesized and still pinned.

## Found during M2 execution (2026-08-10)

- **No composition root — blocks the M2 acceptance criterion.** `backend/cmd/api/main.go` mounts `/health` and nothing else. No bounded context's `Handler.Mount` is wired: not kitchen, and not ordering, menu or tables either. Consequently the composite `GET /sync/config` route **does not exist anywhere in the backend**, even though contracts v0.3.0 made `stations`, `item_stations`, `printers` and `station_printers` REQUIRED fields on it.
  Confirmed by the T6 verifier as a pre-existing repo-wide gap, not a T6 defect — kitchen exposes `Service.SyncConfigBundle` as its contribution and correctly stayed out of cross-context assembly.
  Why it blocks: station routing is cloud→edge config. Without the route, the edge can never learn which item routes to which station, so it cannot generate a ticket, so POS→KDS propagation cannot be demonstrated end to end. Scheduled as its own integration task in M2, not deferred.

- **Printed KOTs carry the order's raw UUID.** No short display-number field exists anywhere in contracts, so `KotOrderContext.order_display_number` falls back to `order.id`. CLAUDE.md §Money/time/identifiers requires human-facing numbers be short (`Order #A184`) and that sequential PKs never be exposed as security identifiers. A cook reading a UUID aloud across a kitchen is not a workable ticket, so this is an ergonomics defect rather than cosmetic. Fixing it is a **contract change** — a short per-outlet display number on the order aggregate — so it needs orchestrator sign-off, an ADR note and a version bump, not a builder-side patch.

- **Tauri commands enforce no backend permissions.** Authorization on the POS is frontend-only gating; `confirm_order` and every kitchen command included. Noted by the T5 verifier as a pre-existing pattern, not a regression that track introduced — recorded here so it is not mistaken for a decision. Related: no kitchen-specific permission exists, so kitchen actions reuse `order.modify`; a line cook marking food ready is not obviously the same authority as a cashier modifying an order.

- **pnpm can silently resolve a stale contracts version.** During M2 the pnpm virtual store held a `@holler/contracts` 0.2.5 symlink while the workspace was frozen at 0.3.0; `pnpm install --filter` cleared it. Worth confirming CI cannot hit the same state — a workspace that can quietly build against an OLD frozen contract defeats the point of freezing it.

- **USB and Bluetooth printing is unproven on real hardware.** `NetworkTransport` (TCP) is exercised against a real listener, but USB and Bluetooth share a `PathTransport` tested only against a file standing in for a device path. That proves the open/write/flush sequence and nothing about serial handshake, USB enumeration, or Bluetooth pairing and backpressure. No printer hardware exists in this environment, so the limitation was accepted rather than faked — but "printing works" is currently true only for network printers. **Fold this into the ADR-013 clean Windows 10 VM gate above**: both are the same problem, that nothing has been exercised on the hardware an outlet actually runs.

- **`/sync/config` returns an empty `users` array — this threatens MILESTONE 1's acceptance, not just M2's.** The route is wired and the other eight fields are correct, but `users` is always `[]`. `packages/contracts/go/identity.go` deliberately carries no `EdgeUserCacheEntry` mirror ("no struct here carries credential material") and `internal/auth` exports nothing returning a `password_hash` outside its unexported `credentialRow`. **A freshly-synced edge node therefore receives zero cached credentials and cannot authenticate a cashier offline** — confirmed by the T7 verification pass, not inferred. ADR-011 makes that cache the mechanism by which offline login works at all, and M1's acceptance is that a cashier can still create orders with the network disconnected; today that only holds via dev-seeded data, not via any production path.
  The OpenAPI schema for `EdgeUserCacheEntry` already exists (`openapi.yaml`), so the missing pieces are the Go mirror plus an outlet-scoped, permission-flattened `auth` export. **Contract change — orchestrator only.**
  Worse, it fails silently: an edge that syncs today gets no signal it received zero credentials, only downstream login failures. Whoever wires the edge sync worker against this route needs to treat an empty `users` as an error rather than an empty set.

- **KDS LAN port authentication** — merged into the **Device enrollment** entry under Security, which now carries the hard trigger. Judged a real defect by the T3 verifier, not acceptable-by-scope.

- **`lan.ts` froze message shapes but not transport.** The KDS client and the `edge/device` server were built independently against it and did not interoperate: the server requires `outlet_id`/`device_id` handshake query params, the client connected to its configured URL verbatim. Neither violated the contract, which is the point — a contract that specifies payloads but not the handshake does not actually pin the interface. Being closed by a `lan.ts` amendment documenting path, query params and the 400 behaviour.

- **`POST /kots/{kotId}/status` has no HTTP-level 422 assertion.** The create route asserts the envelope-mismatch status code directly; the status route shares the identical `requireKotEnvelope` path and is covered at service level only. The code path is proven, the wire contract is not. Cheap to close on the next backend touch.

## Found during the M4 manual POS pass (2026-08-27)

Four defects found in roughly twenty minutes of driving the shipped POS by
hand, while observing M4 acceptance criterion 4. **None block M4** — all four
are M1/M2 ordering surface, filed rather than fixed mid-milestone.

Every suite in the repo passes over all four. Nobody had driven the ordering
screen by hand since M1.

- **DINE_IN accepts an order with NO TABLE SELECTED.** This is not an
  online-order provision — online is DELIVERY or an aggregator channel, both of
  which have their own order types. A dine-in order with no table cannot be
  found by a waiter, cannot be added to, and at billing nobody knows which
  table to close. It also strands the `table_session` aggregate that ADR-011
  deliberately split out from `restaurant_table` for exactly this purpose.
  Either DINE_IN requires a table, or there is an explicit, named
  "counter / no table" option the cashier chooses deliberately. **Silently
  accepting an empty selector is the worst of the three.** Unlike a stock
  block, refusing here costs nothing: it is one tap on a channel that
  inherently has a table.

- **The cart does not clear after a successful send, and the line sticks.** The
  REFUSAL itself is correct and must stay — `SENT_TO_KITCHEN` must not be
  silently amendable. Three things around it are wrong:
  a. the cart should empty when the order goes to the kitchen;
  b. the `-`, `+` and Remove controls stay enabled on a non-amendable order
     while Send correctly greys out — the screen knows the order state, the
     per-item controls do not read it;
  c. the error is developer text. `order <uuid> is not amendable: status is
     SENT_TO_KITCHEN` in small red type at the bottom of the screen is not
     something a cashier reads or can act on. §64 is binding here: every error
     must say whether intervention is needed and what it is.

- **"Beverages" appears TWICE in the category list.** Both rows are real and
  seeded on purpose, which is why no test caught it: `CATEGORY_ID`
  (sort_order 1) is the legacy fixture whose exact ids, price and routing
  `tests/e2e-scenario/harness` pins, and the T0b seed menu carries its own
  "Beverages" (sort_order 8) from `HOLLER_DEV_MENU_SPEC.md`. The seeder
  documents the collision at `edge/database/src/bin/devseed.rs:62-75`. Merging
  them would break the harness, so the fix is to disambiguate — rename the
  legacy fixture category, or scope the harness to its ids rather than a name.
  A cashier seeing one category name twice cannot tell which to tap.

- **"Kitchen Prep (internal — not sold)" appears in the ORDERING screen.** Its
  own name says it is not sold. The category exists because
  `recipe.menu_item_variant_id` is NOT NULL (migration 0015), so even a pure
  sub-recipe must bind to a menu item/variant — see `INTERNAL_CATEGORY_ID`,
  `devseed.rs:126`. Nothing in the contract marks a category or item
  non-sellable, so the till lists prep components as orderable food. Fixing it
  properly is a **contract change** (an `is_sellable` or equivalent on the
  category or item) and therefore orchestrator-only; filtering the one known id
  in the POS would be a patch over a modelling gap that will recur the moment a
  second internal category exists.

## Found on the gaps screen (2026-08-27, M4 criterion 5 pass)

- **THE POS NEVER ATTACHES A VARIANT, so no sale from the shipped till ever
  deducts stock.** `variantId: null` is hardcoded at both `addItem` call sites
  in `apps/pos/src/components/PosScreen.tsx` (lines 131 and 233) — the only
  file in the POS that mentions variants at all. There is no picker. A recipe
  binds to `menu_item_variant_id` (NOT NULL, migration 0015), so resolution
  returns `GapReason::NoVariant` for *every* dish, including the 22 that carry
  recipes. Nothing falls back to `menu_item_variant.is_default`, which
  contracts 0.5.0 added and no code reads — CLAUDE.md's own rule: a column
  nothing reads is a column that does not exist.
  **This contradicts M4 acceptance criterion 1** as an observed behaviour.
  Criterion 1's evidence is `edge/database/tests/seed_offline_sale.rs`, which
  selects a variant directly and therefore cannot see this; the milestone's own
  rule is that no criterion may be evidenced by a test harness. Found because
  Palak Paneer and Paneer Butter Masala appeared as `NO_VARIANT` on the gaps
  screen despite both carrying `["Half", "Full"]` and a recipe on "Full".

- **The gaps screen is titled "Items Sold With No Recipe" and every row says
  `NO_VARIANT`.** Different problems with different fixes: `NO_VARIANT` means
  no sellable variant exists and the *menu* is wrong; `NO_RECIPE` means the
  variant exists and a *recipe* must be written. A manager acting on this
  screen needs to know which, and today the title asserts the answer while the
  data contradicts it. Same name-asserts-a-property class as the M2-era
  entries above. Either split the screen by reason or retitle it to something
  reason-neutral ("Sales With No Stock Deduction") and group by reason.

- **Timestamps render as raw UTC ISO on a screen a restaurant manager reads.**
  `2026-08-27T08:47:10.615Z` is 14:17 IST. CLAUDE.md is explicit: UTC storage,
  outlet timezone stored separately, rendered local. The gaps screen renders
  the stored value verbatim. Outlet-local and human-formatted, and check the
  sibling inventory screens for the same.

- **Confirm the deliberate no-variant seed items are actually the ones
  appearing.** T0b leaves 11 items with no variant on purpose (Samosa, Pani
  Puri, Aloo Tikki Chaat, Seekh Kebab, Egg Bhurji, Jeera Rice, Steamed Rice,
  Laccha Paratha, Filter Coffee, Gulab Jamun, Gajar Halwa) and 6 with a variant
  but deliberately no recipe (Chana Masala, Mixed Veg Curry, Fish Curry, Mutton
  Biryani, Sweet Lassi, Packaged Fruit Juice). Verified 2026-08-27: Palak
  Paneer, Paneer Butter Masala and Fish Curry are **not** in either list as
  no-variant items — Palak Paneer and Paneer Butter Masala carry recipes, Fish
  Curry should have read `NO_RECIPE`. The seed's recipe coverage is as
  reported; the defect is the hardcoded null above, not thin seed data.

## Found in the M4 manual deduction pass (2026-08-27)

- **The stock-count quantity input never names its unit.** The label reads
  "Counted quantity (whole units)". The unit is grams for MASS, millilitres for
  VOLUME and pieces for COUNT (`human_quantity_to_micro`,
  `apps/pos/src-tauri/src/commands/inventory.rs`). A manager counting 10 kg of
  spinach types `10` and records 10 grams; one counting in kilograms out of
  habit is off by 1000 with no feedback. Label the actual unit per item and
  echo it beside the input. Contributing factor to a 90,000 g spinach entry
  during this pass, which was data entry rather than arithmetic — the ledger
  showed three adjustments (3000 g, 7000 g, 90,000 g), each an exact x10^6 of
  what was typed.

- **Seeded reorder levels are arbitrary and make the banner meaningless.** With
  every item near zero, 28 of 32 read LOW, so the signal that criterion 4 exists
  to prove is buried the first time anybody looks. Soda Water's `litres(5)`
  reads correctly as 5000ml now, but a 5-litre reorder point on soda in a
  restaurant is a placeholder, not a threshold. Sanity-check the whole seeded
  set against plausible restaurant pars before anyone else sees that screen.

## Testing

- **Postgres integration tests never clean up their rows.** Every tenant/brand/outlet/order row these suites insert stays forever. Harmless today — ids are minted per run since `1cc087c`, so nothing collides — but the database grows without bound across CI runs. Pre-existing, and deliberately left alone during the fixture repair to keep that change to one concern. Worth a `t.Cleanup` or a per-run schema before CI runs these on every push.

- ~~**Postgres integration tests have never run.**~~ **FIXED at `1cc087c` — all nine now execute and pass.** The entry below is kept because its diagnosis was wrong in an instructive way. The entry was wrong about the cause: the tests were never gated on a *missing* Postgres, they were gated on `HOLLER_TEST_DATABASE_URL` being unset while `holler-postgres-1` was up the whole time. Nobody had set the variable. Running them revealed nine failures, all **test-fixture bugs, not production bugs** — which is precisely why they went unnoticed: a suite that never executes cannot fail.

  | Package | Tests | Failure |
  |---|---|---|
  | `internal/auth` | `TestIntegration_CreateUserAndLogin`, 3 × `PostgresRefreshStore` | `column "tenant_id" of relation "outlet" does not exist` — the fixture inserts an outlet scoped by `tenant_id`, but `outlet` is `brand_id`-scoped (`postgres/0001_init.sql`) |
  | `internal/kitchen` | `TestPostgresRepository_StationPrinterKotLifecycle` | `CreateCategory: unauthorized` — fixture builds no principal |
  | `internal/ordering` | 4 × `TestPostgresRepository_*` | same missing-principal fixture bug |

  What this costs: `PostgresRefreshStore`'s **rotation and family-revocation logic has still never actually executed** — the security behaviour ADR-012 relies on is code-reviewed only, and the fixture bug is what has been hiding that. Same for ordering's idempotency-on-replay and the 0.2.4 canonical-field round-trip.

  Passing against real Postgres, verified: `TestBuildRouter_SyncConfigEndToEnd` (login → create table/category/item/station → pull `/sync/config` twice) and `TestIntegration_ListEdgeUserCache`, plus all of `menu`, `outlet`, `tables`, `tenant`.

- **CI never ran most of what Milestone 2 built.** `.github/workflows/ci.yml` covered `edge/database` and `edge/sync` but not `edge/printer` or `edge/device`, and `apps/pos` but not `apps/kds` — and nothing ran the cross-language LAN socket test. Fixed in this milestone; the lesson is that a crate absent from that file is a crate CI does not defend, which is the same shape as the drift check omitting `edge/database` until 0.2.3.
- **`make test` covers only Go.** The Rust and TypeScript suites are run directly. Either extend the target or stop calling it the project test command.

- **No POS dev-server smoke test — and it MUST hit the dev server, not the build.** (Filed 2026-08-20, after the second dev-only failure.) The POS rendered a white screen for a whole session while `pnpm build`, `tsc --noEmit` and all 165 unit tests stayed green. The cause was a stale Vite dep prebundle in `apps/pos/node_modules/.vite`, dated a day before the contracts it was built from.

  **The load-bearing detail: `optimizeDeps` prebundling is a dev-server-only mechanism.** `vite build` does not read that cache, so a build-based smoke test — including the headless-Chromium one used to *diagnose* this incident, which loaded `dist/` and rendered the login screen perfectly — cannot catch this class at all. A test that runs `pnpm build` and loads the output proves the source is sound and proves nothing about the thing a developer runs every day.

  What the test must do: start `pnpm dev`, load `http://localhost:5173` in headless Chromium, and fail on any console error or an empty `#root`. Roughly 40 lines; `apps/kds` already has Playwright installed, and `apps/kds`'s own `test:e2e` job is the closest existing model. Two wrinkles to handle honestly: there is no Tauri runtime in a plain browser, so `invoke` is absent — the test can only assert the pre-login shell mounts, which is exactly the surface that broke; and it should run against a **cold** `node_modules/.vite` at least once, since a warm correct cache hides the failure.

  Related, same root: `apps/pos` has no browser smoke test of any kind, which `CLAUDE.md` already records for the KDS's detached-global bug (`docs/retro.md`, 2026-08-11). That was also dev/browser-only and also invisible to every green suite. Two incidents, one shape.
