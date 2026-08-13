# e2e-scenario-harness

An autonomous, randomized, seeded end-to-end scenario harness for the full
POS↔kitchen lifecycle. Commissioned because five separate defects in one
week (`docs/retro.md`, 2026-08-10/11 entries) passed every existing green
suite and were found only by a human using the product. This harness exists
to be the thing that would have caught them.

## Architecture (non-negotiable, per the track brief)

Drives the real shipped layers, never a reimplementation of them:

- **POS side** — `tests/e2e-scenario/harness` (Rust bin) links
  `holler_pos_lib` (`apps/pos/src-tauri`, `path` dependency) and calls the
  exact `commands::*::*_impl` functions the Tauri IPC layer calls (the
  same surface `apps/pos/src-tauri/tests/critical_offline_flow.rs` already
  proved reachable this way).
- **LAN server** — the same `holler_edge_device::server::start` the POS
  embeds in production, wired to the same `Arc<Mutex<Db>>`/`Hub` the
  `*_impl` functions notify through (mirrors
  `apps/pos/src-tauri/src/state.rs` exactly).
- **KDS side** — `tests/e2e-scenario/orchestrator` (TypeScript/vitest, the
  actual test runner and top-level orchestrator) imports the real
  `apps/kds/src/lib/{lanConfig,connectionController,lanClient}` and
  `apps/kds/src/store/kdsStore` modules directly and drives them over a
  genuine `WebSocket` (Node's own global, not a fake) — the same pattern
  `tests/integration/kds-lan` established, extended with a richer
  bidirectional protocol.
- **Bridge** — the Rust binary and the TS orchestrator talk line-delimited
  JSON-RPC over stdio (`tests/e2e-scenario/orchestrator/src/bridge.ts`
  spawns the harness via `cargo run`, exactly as
  `tests/integration/kds-lan/bridge.ts` spawns `kds-lan-bridge`).

Every run seeds a fresh scratch database via the real `devseed` binary
(`edge/database/src/bin/devseed.rs`), pointed at a temp directory via
`HOLLER_E2E_DATA_DIR` — never `%APPDATA%`, never a real `edge.db.enc`. The
harness then augments that seeded template with the extra fixtures the spec
requires that `devseed` does not provide (a second station, a multi-station
item, a deliberately unrouted item) via `holler_edge_database::repo::*`
calls — never raw SQL, never editing `devseed` itself. Every scenario gets
its own cheap copy of that sealed template file.

## Running it

```powershell
cd tests\e2e-scenario\orchestrator
pnpm install
pnpm test              # CI's own fixed-seed, 50-scenario reduced run
pnpm run:full -- --seed 12345 --count 200   # full manual run
```

See `docs/DEV_SETUP.md`'s "e2e-scenario-harness" section for CI wiring and
first-run compile cost.

## Scenario generator

Deterministic (`mulberry32`, seeded) — `src/rng.ts`. Every scenario's own
seed, and the run's base seed, are printed and recorded in the report; any
failure is reproducible with the same `--seed`.

Action vocabulary (`src/runner.ts`): create draft (always the first action);
add item (same item repeatedly is a normal outcome of the random pick, not a
special case); remove item; change order type; set/change/clear table;
confirm; a coverage probe that attempts to add an item after confirm (see
Findings); send to kitchen; send again (idempotency); a probe for an illegal
`NEW -> SERVED` transition (and a second, later-state illegal-transition
probe) before any legal walk moves a ticket off `NEW`; a legal walk through
`ACKNOWLEDGED -> PREPARING -> READY -> SERVED`, split randomly between
POS-driven (`transition_kot` over the bridge) and KDS-driven
(`requestStatusChange` over the real WebSocket); acking an unknown/stale KOT
id; disconnecting and reconnecting the KDS client mid-sequence; and a
crash-and-recover step at a random point (mid-DRAFT or post-send).

Fixtures cover all three order types, ≥2 tables, ≥2 stations, an item with a
variant and modifiers (though modifiers cannot actually be attached to an
order line — see Findings), a multi-station item, and a deliberately
unrouted item, per the spec.

## Crash simulation

The harness force-kills (`SIGKILL`/`TerminateProcess`) the whole Rust bridge
process and spawns a fresh one against the same scratch directory, which is
asked to resume the scenario. See `bridge.ts`'s `crashAndResume` doc comment
for why this is a real process kill rather than an in-process trick: an
earlier in-process "swap the `Db` for a throwaway and `mem::forget` the
real one" approach leaked the OS-level SQLite file handle within the still-
alive harness process, and Windows then kept the plaintext file locked
against the very `Db::open` call meant to recover it — an artifact of the
process still existing, which a real crash never has. A genuine process kill
sidesteps the problem entirely and is a more faithful simulation besides.

## Invariants (`src/runner.ts`, checked after every scenario)

1. **State machine legality** — every order/KOT status value is legal;
   every attempted transition either succeeds only when legal or is
   rejected; illegal-transition probes must be rejected.
2. **KOT conservation** — every routable order item appears on exactly one
   KOT **per station it is routed to** (a multi-station item legitimately
   appears on more than one KOT by design — checked per
   `(order_item_id, station)`, not per item alone); no KOT references an
   unknown item; re-send never duplicates.
3. **KDS fidelity** — every KOT with a routable station reaches the
   subscribed KDS client within 2s (latency recorded).
4. **No-station items** — sending must produce an explicit, surfaced
   outcome. An order made entirely of unrouted items must reject with
   `NOTHING_TO_SEND_TO_KITCHEN`; a mixed order (some routable, some
   unrouted) must reject with `UnroutedKitchenItems`/`UNROUTED_KITCHEN_ITEMS`
   naming the unrouted item(s), and must create zero KOTs for either kind of
   line. See Findings: this was silent for the mixed case until M3 Track A.
5. **Money** — subtotal always equals Σ(line totals) in integer paise,
   checked after every mutation and against the final durable row.
6. **Durability** — after crash+recover, the committed orders/items/KOTs
   are byte-identical (via a normalized JSON comparison) to their pre-crash
   state.
7. **Outbox** — no duplicate outbox row ids, nothing ever marked
   `published_at` (no publisher runs in this harness), and at least the
   minimum set of events the scenario is known to have caused directly is
   present (not an exact count — `transition_kot_status_with_outbox` also
   emits an internal `OrderReady` row this model does not re-derive; an
   under-count still fails, which is the property that matters).
8. **Status echo** — a KDS-driven status change is reflected through the
   POS's own read command (`list_kots`) within 2s.

## Findings (coverage gaps and product defects — not fixed by this track)

Recorded in every run's report (`orchestrator/src/report.ts`), deduplicated.
The report is a scratch artifact, not a tracked file — it writes to
`$TMPDIR/holler-e2e-scenario-reports/REPORT-<seed>-<timestamp>.md` by
default; set `HOLLER_E2E_REPORT_DIR` to redirect it (e.g. for a CI artifact
upload step). Each run's CLI output prints the exact path used. As of this
track's own verification run:

- **Modifiers are unreachable at the order-item level.**
  `commands::orders::NewOrderItemRequest` carries no modifiers field —
  `apps/pos/src-tauri/src/commands/orders.rs` states this explicitly.
  `MenuItemModifier` rows exist and price deltas are seeded (via the
  Masala Chai fixture, mirroring `devseed`), but no shipped command can
  attach one to an order line.
- **`cancel_kitchen_items_with_outbox` has no Tauri command.** Unreachable
  from the shipped surface — per the track brief, not faked and not added
  here.
- **No shipped command can add an item to an order once it has left
  DRAFT.** `add_order_item` enforces DRAFT-only. Partial add-then-send /
  KOT `#132-A` amendments are therefore also unreachable, for the same
  underlying reason as the cancellation gap above.
- ~~**PRODUCT DEFECT — `zero-station-item-send` (named regression).**~~
  **CLOSED at M3 Track A.** `send_order_to_kitchen_with_outbox_inner`
  (`edge/database/src/lib.rs`) used to silently skip a line item with no
  station routing when the order *also* had routable items: the call
  succeeded, tickets were created for the routed lines, and the unrouted
  line simply never appeared on any KOT — no error, no per-item outcome
  field, nothing in the response that distinguished "sent everything" from
  "sent some." An order made entirely of unrouted items already got an
  explicit `NOTHING_TO_SEND_TO_KITCHEN` error; only the mixed case was
  silent. This was invariant 4's exact "silence is a FAIL" case, and the
  harness caught it on essentially every run whose randomized fixture pool
  happened to mix a no-station item with a routable one.
  Fixed by rejecting the whole `send_order_to_kitchen_with_outbox` call
  outright when *any* unticketed line is unrouted — `DbError::
  UnroutedKitchenItems`, naming every affected item, with **zero** `kot`
  rows written for any line on that call (never a partial send). The
  `zero-station-item-send` named regression (`run.ts`) now asserts the
  fixed behaviour rather than merely reproducing the bug; it stays in the
  named-regression set as the standing guard against this shape recurring.

## Self-falsification

Before trusting this harness, one invariant's real subject was deliberately
broken and the harness was confirmed to catch it — see the verification
report for the exact procedure (a scratch mirror of `edge/database` +
`edge/device` + `edge/printer` + `apps/pos/src-tauri` +
`tests/e2e-scenario/harness`, outside the repository, with
`LEGAL_KOT_TRANSITIONS` widened to permit `NEW -> SERVED`, built and run via
`bridge.ts`'s `HOLLER_E2E_FALSIFY_MANIFEST` override). The break was caught
immediately and specifically by invariant 1 on nearly every scenario that
exercised a KOT, with correct cascading detail (subsequent legal
transitions from the now-corrupted `SERVED` state were, correctly, also
rejected). `git status --porcelain edge/ apps/pos/src-tauri` was empty
throughout and after — the real crates were never touched.
