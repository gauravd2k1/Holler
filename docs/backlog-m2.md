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

- **POS cart persistence.** The cart lives in browser memory until Send, so a crash mid-order loses it. `add_order_item`/`remove_order_item` return `UNSUPPORTED_DB_OPERATION` at the Tauri layer even though the `edge/database` API now exists. Wire them through. Offline order *creation* is atomic and correct; this is about in-progress work on a system whose premise is that the shop floor never loses work.
- **`dto::MenuItem.schema_version`.** The Rust DTO omits it, so the POS injects the constant before Zod-parsing — a shim at the exact boundary meant to catch drift. The verifier confirmed the spread order makes it non-masking, but one field on the Rust struct fixes it properly.
- **`ReplayTransition` version reuse.** Treats `version <= stored` as a duplicate and silently returns current state. Correct under §50.1's single-writer monotonic versioning; becomes a silent-drop risk the moment multi-device edges exist (a waiter tablet and a POS transitioning one table). Needs an ADR before M2 introduces more writers.
- **`ON CONFLICT (id) DO NOTHING` in ordering.** Correct idempotency for identical replays, but silently no-ops if a different payload reuses an existing id, masking a content mismatch instead of surfacing it.
- **Menu category read command.** `list_menu_items` is the only menu read surfaced, so the POS renders categories by raw UUID. `edge/database` now has the list functions; they need Tauri commands.

## Security

- **Redis-backed rate limiter.** The login limiter is in-memory: it does not survive restart and does not share budget across instances. The verifier judged this materially different from the refresh-store violation — it degrades a defence-in-depth mitigation rather than disabling reuse detection — so it was acceptable for a single-instance M1 backend. ADR-012 names Redis as the intended store. Wire it before any multi-instance or production rollout. The fixed-window implementation also permits a 2× burst across a window boundary.
- **Edge DB key provisioning.** `HOLLER_DB_KEY_HEX` supplies the encryption key via environment variable. Fail-fast on absence is right, but an env-var key is weak key management for something ADR-011 calls encryption at rest. Wants an OS keystore or TPM.
- **Device enrollment.** No flow exists. `tenant_id` and `device_id` are supplied at worker construction with nothing to verify them against, so a mis-enrolled node would silently mislabel every outbound envelope.
- **SQLCipher.** Page-level encryption was unavailable because the vendored OpenSSL build needs a Perl toolchain absent from this environment. The current AES-256-GCM sealing leaves a plaintext working file for the process lifetime. Provisioning the toolchain would let a follow-up swap it behind the crate's existing API — but per ADR-013 the *outlet install* must stay free of any such requirement.

## Contracts

- **`ItemRemoved` is unroutable.** It is frozen in `OUTBOX_EVENT_TYPES` and `edge/database` emits it, but OpenAPI has no ingest route — only `POST /orders/{id}/items` for adds. So a line removal can be written to the outbox and can never reach the cloud. Needs a route plus envelope schema. **The drift check does not catch this**, and that is its real limitation: it verifies a literal *appears* in Rust, not that the event is actually deliverable. Worth considering whether the check should cross-reference OpenAPI routes, or whether that is the job the generated Rust binding should do.
- **Rust binding for `packages/contracts`.** Deferred until a fourth Rust consumer. Until then `scripts/check-event-type-drift.mjs` greps literals in both directions. That check had a real bug — it omitted `edge/database`, the crate that actually builds outbox payloads — which is the argument for generating a binding rather than grepping for one.
- **Deferred `CanonicalOrder` columns.** `packaging_paise`, `delivery_charge_paise`, `aggregator_discount_paise`, `merchant_discount_paise`, `customer`, `delivery_address`, `rider` land in M6; `preparation_time_minutes` in M2. Synthesized values are pinned by the order-level round-trip test, so persisting one will fail that test until the ADR-011 0.2.4 table is updated.

## Testing

- **Postgres integration tests have never run.** Every `HOLLER_TEST_DATABASE_URL`-gated test skips in this environment, so all cloud SQL — including `PostgresRefreshStore`'s rotation and family revocation — is code-reviewed, not executed.
- **`make test` covers only Go.** The Rust and TypeScript suites are run directly. Either extend the target or stop calling it the project test command.
