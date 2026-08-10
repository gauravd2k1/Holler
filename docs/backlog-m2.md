# Milestone 2 backlog

Items deferred from Milestone 1 with a decision behind each. This is not a wish list — everything here was found by a verification pass or an implementation and consciously postponed rather than forgotten.

---

## Planning inputs
Review docs/competitive.md during M2 planning — one open decision (waiter app
milestone) and four spec additions already filed to their landing milestones.

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

## Correctness and hardening

- **No quantity control on a cart line.** Found by the T9 verifier while judging a different disclosure — the builder described a remove-then-add quantity path, and the verifier could not find it because *no quantity adjustment feature exists at all*. Tapping the same menu item twice produces two separate lines of quantity 1, not one line of quantity 2. A cashier ringing three samosas gets three lines.
  Not a regression (the durable-cart work did not remove anything) and not a correctness bug — the order total is right either way. But it is wrong for a POS, and it interacts with the durable cart: closing it needs an `update_order_item_quantity` in `edge/database`, because doing it as remove-then-add is two separate durable writes with a window between them where a crash loses the line. That window is exactly what the cart-persistence work exists to eliminate, so **do not implement quantity adjustment as remove-then-add.**

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

  **When `device_token` verification turns on, move it out of the query string** — an `Authorization` header, or a first-frame auth message before the snapshot. The transport comment in `lan.ts` names query-string logging as the hazard, and it applies with full force the moment the value stops being worthless: a secret in a query string is a secret in every proxy and access log on the path. Query-string carriage is acceptable *only* while the token is unverified and therefore carries no authority. Closing enrollment without also moving the parameter would convert a documented non-secret into an undocumented leaked secret.
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

## Testing

- **Postgres integration tests have never run.** Every `HOLLER_TEST_DATABASE_URL`-gated test skips in this environment, so all cloud SQL — including `PostgresRefreshStore`'s rotation and family revocation — is code-reviewed, not executed.
- **`make test` covers only Go.** The Rust and TypeScript suites are run directly. Either extend the target or stop calling it the project test command.
