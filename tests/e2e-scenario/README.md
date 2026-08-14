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

Action vocabulary (`src/runner.ts`): create draft (always the first action,
occasionally with a real modifier price delta attached — see below); add
item (same item repeatedly is a normal outcome of the random pick, not a
special case); remove item; change order type; set/change/clear table;
confirm; a `#132-A` amendment probe that adds an item to the CONFIRMED order
(see Findings); send to kitchen; send again (idempotency); a probe for an
illegal `NEW -> SERVED` transition (and a second, later-state illegal-
transition probe) before any legal walk moves a ticket off `NEW`; a legal
walk through `ACKNOWLEDGED -> PREPARING -> READY -> SERVED`, split randomly
between POS-driven (`transition_kot` over the bridge) and KDS-driven
(`requestStatusChange` over the real WebSocket); acking an unknown/stale KOT
id; disconnecting and reconnecting the KDS client mid-sequence; a
crash-and-recover step at a random point (mid-DRAFT or post-send); and
(T11b) billing — issue a GST invoice for the order and record a payment
sequence against it (usually a genuine two-tender split, occasionally with a
reversal), whenever the order left DRAFT with at least one line.

Fixtures cover all three order types, ≥2 tables, ≥2 stations, an item with a
variant and modifiers, a multi-station item, and a deliberately unrouted
item, per the spec. Modifiers are attached via real requests to
`create_order`/`add_order_item` (M3 Track B landed the wire field this
harness previously could not reach); billing fixtures (a GST-5% tax profile,
an outlet fiscal profile, an active `SALES` invoice series) are seeded by the
harness itself alongside the station/menu-item augmentation, since `devseed`
provides none of them (T11b).

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
9. **Tax reconciliation** (T11b) — every invoice line's
   `taxable_value_paise + cgst + sgst + igst + cess == total_paise`; every
   per-line component sums exactly to the invoice-level total of that
   component; and `taxable_value + Σtax components + round_off_paise ==
   grand_total_paise`, all in integer paise (never a float comparison). Every
   paise field checked must be a non-negative integer, except
   `round_off_paise` which may legitimately be negative.
10. **Payment settlement** (T11b) — forward tenders (positive `amount_paise`)
    plus any reversals (non-positive, `reverses_payment_id` set) recorded
    against an order never sum past that order's invoice `grand_total_paise`,
    checked both against what this scenario itself recorded and against the
    persisted `list_payments_for_order` row set independently.

## Findings (coverage gaps and product defects — not fixed by this track)

Recorded in every run's report (`orchestrator/src/report.ts`), deduplicated.
The report is a scratch artifact, not a tracked file — it writes to
`$TMPDIR/holler-e2e-scenario-reports/REPORT-<seed>-<timestamp>.md` by
default; set `HOLLER_E2E_REPORT_DIR` to redirect it (e.g. for a CI artifact
upload step). Each run's CLI output prints the exact path used. As of this
track's own verification run:

- **`cancel_kitchen_items_with_outbox` has no Tauri command.** Unreachable
  from the shipped surface — per the track brief, not faked and not added
  here.
- **Split-bill invoicing is unreachable from the shipped surface (T11b).**
  `holler_edge_database::Db::issue_split_invoices_with_outbox` exists but
  `apps/pos/src-tauri/src/commands/billing.rs` deliberately excludes it from
  M3's command surface (`docs/m3-planning.md`) — `issue_invoice` always
  bills the whole order at `split_count == 1`. The "a split bill's parts sum
  to the whole" money invariant this track was asked to add cannot be
  exercised end to end for the same reason cancellation and (formerly)
  modifiers could not.
- **A per-line discount is unreachable from the shipped surface (T11b).**
  `billing.rs`'s `build_invoice_lines` hard-codes `discount_per_unit_paise:
  0` for every invoice line — no command lets a cashier apply one.
- **PRODUCT DEFECT (harness bit-rot, closed by T11b): the harness itself
  had not compiled since device enrollment (ADR-017) and M3 Track B landed.**
  `edge/device::server::start` gained a required `DeviceTokenVerifier`
  argument, `MenuItem` gained a required `tax_profile_id` field, and
  `NewOrderItemRequest` gained a required `modifiers` field — none reflected
  in `harness/src/main.rs`, so `cargo build` failed outright. Separately, the
  harness's own `#132-A` amendment probe still asserted the *pre-Track-B*
  behaviour (`add_item` after CONFIRMED must be rejected with
  `ORDER_NOT_DRAFT`) after the product had been deliberately widened to
  allow exactly that — once the harness was made to compile again, this
  probe alone turned invariant 1 red on all 54/54 scenarios. Both are
  harness staleness, not product defects; `docs/RESUME.md`'s claim that
  "Not re-run this session: the e2e harness (54 scenarios)" is the reason
  neither was caught earlier — the harness had silently stopped being a
  gate. Fixed here: the LAN server is now wired with a real
  `CachedCredentialVerifier` against one harness-seeded KDS device
  credential (Argon2id-hashed, verified for real — see `hash_device_secret`
  in `main.rs`); `MenuItem`/`NewOrderItemRequest` construction sites are
  updated; and the amendment probe now asserts the current, documented
  `#132-A` behaviour (success while CONFIRMED/SENT_TO_KITCHEN/PREPARING).
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

**T11b repeated this for the two new invariants**, using the same
`HOLLER_E2E_FALSIFY_MANIFEST` scratch-mirror pattern (a `git worktree` at the
last known-green commit, since a concurrent Track A defect fix left the real
repo's `edge/database` mid-edit and non-compiling at the time — see "Known
limitations" below): `edge/database/src/tax/engine.rs`'s `round_off_paise`
computation was deliberately off by one paise. The break was caught
immediately by every scenario that reached billing, surfaced as invariant
9 (`9_tax_reconciliation`) — in this run even earlier, at `issue_invoice`'s
own SQLite `CHECK` constraint on `grand_total_paise`, so the corruption never
even reached a persisted row. Reverting the injected line restored an
all-green run (54/54 scenarios, invariants 1–10 all `checked && passed`).
`git diff edge/database/src/tax/engine.rs` was empty after the revert — the
real crate was left exactly as found.

## Known limitations (T11b)

- **The CI job now asserts per-invariant, not just harness-level fatals**
  (`scenario.test.ts`, since M3 Track A/T2 closed the last known invariant-
  level defect) — a single new violation anywhere fails the job. No baseline
  of known-failing scenarios exists because none is currently needed: every
  invariant, including the two T11b added, passes on the full 54-scenario
  CI run as of this track. If a future defect makes a specific invariant
  genuinely and durably fail, prefer fixing it; only fall back to a named,
  logged baseline (per the T11b brief) if the fix is out of scope for the
  track that found it — silently loosening an assertion is never the right
  call.
- **This run required a clean scratch mirror of the repo**, not the working
  tree, because a separate track was mid-edit in `edge/database` (a
  double-settlement fix) and left it non-compiling. `HOLLER_E2E_FALSIFY_
  MANIFEST` already existed for exactly this "point the bridge at a
  different checkout" need; T11b's own verification run used a
  `git worktree` at the last known-green commit rather than stashing the
  other track's uncommitted work. Once that track lands, re-run the suite
  against the real working tree before trusting it as the standing CI gate
  again.
