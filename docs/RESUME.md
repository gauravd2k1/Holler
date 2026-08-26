# M4 resume state — 2026-08-24

`main` is at **`75801c1`**. Read this file first, then
`docs/adr/ADR-018-m4-inventory-contracts.md`, `docs/m4-planning.md`, and the
2026-08-23 / 2026-08-24 entries in `docs/retro.md`.

**This file replaced an "M3 resume state" header that was four days and one
milestone stale.** Its M3 content is carried forward below rather than deleted:
M3 is still not acceptance-complete, and the defects it listed are still open.

---

## 1. M4 acceptance — the seven criteria, with evidence named per row

CLAUDE.md's rule for this milestone: *every item is an observed behaviour, not
an implemented API, and none may be evidenced by a test harness* — an acceptance
run exercises the binaries that ship (`docs/retro.md`, 2026-08-11).

**Five of seven are met. Two are met at the edge and unobserved at the surface
the criterion actually names.** The table says which is which; do not read
"green" as "accepted".

| # | Criterion | Verdict | Evidence — where, and what it exercises |
|---|---|---|---|
| 1 | Offline sale from the **real seed menu** deducts every ingredient at recipe × line quantity, plus chosen-modifier deltas, and nothing for modifiers with no delta row | **MET** | `edge/database/tests/seed_offline_sale.rs` — drives `cmd/devseed`'s real catalogue, not a fixture. No network is on the path by construction: SQLite and Rust only. CI job `edge`. 1/1, 2026-08-24. |
| 2 | Kill the POS between confirm and deduction → order and ledger agree on reopen | **MET** | `edge/database/tests/crash_durability.rs` against `src/bin/crashpoint.rs` — a **real `abort()` of a real child process** at `after_confirm_before_deduct`, judged on reopen. Gated behind `--features crash-points`; CI job `crash-durability`, on **windows-latest**, because WAL recovery is OS-specific and outlets run Windows (ADR-013). 2/2, 2026-08-24. |
| 3 | Physical count produces a variance report whose arithmetic is checked against an **independently computed figure** | **MET** | `edge/database/src/stock/variance.rs::variance_matches_an_independently_computed_figure` — the report's numbers recomputed by a second route and compared, not spot-checked. Surface `apps/pos/src/components/StockCountScreen.tsx`, routed at `router.tsx:101`. TypeScript formats `variance_percentage_bps`; it never recomputes it. |
| 4 | An ingredient crossing its reorder level is **visible to a human on the POS** | **EDGE MET / SURFACE UNOBSERVED** | `LowStockBanner` is mounted in `PosScreen.tsx:160` and `OrderListScreen.tsx:94` — the two screens a till is actually on — and `CurrentStockScreen` is routed at `router.tsx:80`. `isLowStock` / `lowStockLines` unit-tested in `apps/pos/src/domain/__tests__/inventory.test.ts`, including the rule that a null `reorder_level_micro` is *unconfigured, not zero*. **Nobody has watched the banner appear in a running POS.** See §2a. |
| 5 | An item sold with no recipe completes the sale, records a gap, and appears on the "items sold with no recipe" report | **EDGE MET / SURFACE UNOBSERVED** | Sale completion and `stock_deduction_gap` are edge-side and tested (`edge/database/src/deduction/ledger.rs`, `apps/pos/src-tauri/src/commands/inventory.rs`). The report is `StockDeductionGapsScreen.tsx`, routed at `router.tsx:108`. Same gap as criterion 4: the screen is reachable in code and has not been seen rendering. |
| 6 | Ledger entries created at the edge replay to the cloud and **read back identically** | **MET, AND FALSIFIED** | `edge/sync/tests/cloud_replay.rs`. Builds and spawns the real `cmd/api` against real PostgreSQL, logs in, enrolls a device through the real ADR-017 route, and drives `SyncWorker::pump_ranged_streams` at it over a real socket — no `tiny_http` stand-in anywhere. The entry is *earned* through `Db::record_wastage`, so `entry_seq` comes from the real counter (asserted to be 1, not 0). Read back **twice**: the 201 echo (wire fidelity, through Go's types) and the PostgreSQL row re-serialised (storage fidelity), both whole-object byte-compares against a key-sorted canonical form. Gated `--features cloud-e2e`; CI job `cloud-replay`. 2/2, green twice, 2026-08-24. |
| 7 | Stock reads stay bounded after a sealed snapshot — **measured, not asserted** | **MET** | `edge/database/src/stock/snapshot.rs::stock_reads_stay_bounded_after_a_sealed_snapshot` — counts **SQLite VM steps** taken by the shipped read over 5 sealed days vs 400, same unsealed tail. No clock is timed, so the figure is identical on a fast laptop and a 4GB spinning-disk till and no regression can hide behind a generous margin. A dropped `entry_seq >` term makes the number climb with history, and the test fails naming both figures. |

### Criterion 6 was falsified, not merely observed

Green is not evidence until you have watched it go red for the right reason.
`entry.Note` was replaced with a typed nil in the cloud's INSERT — one field
dropped **server-side, after the echo** — and:

- the **storage** comparison failed, printed both objects whole, and named `note`;
- the **201 echo** comparison **passed**, because the handler echoes the entry it
  was handed and never reads the row back.

That is the entire argument for keeping two checks rather than one. Had the test
asserted only the echo, a GST-relevant column could go missing under a green
tick. Restored, green twice.

The falsification pass also found a defect in the harness itself — both tests
built `cmd/api` to one path on parallel threads, a race that can only fire when
the Go sources change, i.e. only during falsification. Fixed with
`OnceLock::get_or_init`; written up in `docs/retro.md` 2026-08-24.

### Test counts, all measured on this machine on 2026-08-24

| Suite | Result | Command |
|---|---|---|
| `edge/database` | **262** | `cargo test` |
| `edge/database` crash durability (criterion 2) | **2** | `cargo test --features crash-points --test crash_durability` |
| `edge/sync` | **42** | `cargo test` |
| `edge/sync` cloud replay (criterion 6) | **2** | `cargo test --features cloud-e2e --test cloud_replay` |
| `edge/printer` | **45** | `cargo test` |
| `edge/device` | **11** | `cargo test` |
| `apps/pos/src-tauri` | **70** | `cargo test` |
| `apps/pos` | **182** | `pnpm test` |
| `backend` | **13 packages ok, 0 fail** (267 top-level test funcs across 19 pkgs) | `go test -count=1 ./...` with `HOLLER_TEST_DATABASE_URL` set |

**NOT re-run on 2026-08-24, and therefore not evidence:** `apps/kds`,
`packages/contracts` (TS + Go), and the e2e scenario harness. Their last known
figures are in git history; treat them as stale until someone runs them and
writes the date next to the number.

**The `LNK1104` retry is expected on this box and is not a code error.** McAfee
holds a lock on each freshly-linked test binary; compilation has already
succeeded when it fires, and two or three re-runs reach green because cargo
caches every binary that did link. Every figure above was taken from a run that
linked cleanly.

---

## 2. What still blocks M4 acceptance

**(a) Criteria 4 and 5 have never been observed in a running POS.** Both name a
human seeing something. What exists is: the component mounted on the right
screens, the route registered, and the pure logic unit-tested — none of which is
the criterion. `apps/pos` has **no dev-server smoke test** (`.github/workflows/ci.yml`
says so in a comment at the `pos` job), so the two runtimes this repo has already
been burned by twice — the dev server and the browser — are unguarded for the
POS. CLAUDE.md's rule applies verbatim: build-green ≠ dev-works, and a frontend
change must be reported with the runtime it was observed in.

Closing this needs `pnpm tauri dev`, a seeded low-stock item and a seeded
no-recipe item, and a human looking at the screen. It is the cheapest remaining
item in the milestone.

**(b) `gh` is installed but not authenticated.** `gh auth login` has not been run
on this machine, so `gh run list` still cannot read this repo's CI. The whole
point of installing it — see `docs/retro.md` 2026-08-23, *"Install the CLI that
reads your own CI"* — is unmet until that one interactive command is run. Until
then a push is still fire-and-forget, which is the root cause of the four pushes
that spent a day producing no verdict. **Report the CI verdict in the same
message as the commit; a push whose result nobody read is not a push.**

**(c) The two ADR-013 hardware gates in §4 are still open.** They block M3's
acceptance, not M4's. Listed for completeness, not as M4 blockers.

---

## 3. Closed: `source_stock_count_id` reaches the cloud (contracts 0.5.9)

**Found while proving criterion 6, fixed the same week.** Contracts 0.5.5 added
the column; it existed in **both** stores, was on the edge model, and **was
sent** by `ledger_entry_payload`. The cloud had never heard of it — absent from
`contracts.StockLedgerEntry`, from the INSERT, from the SELECT — and the payload
decode is a lenient `json.Unmarshal`, so it was **silently discarded rather than
refused**. Every count-sourced adjustment replayed without its provenance, and
migration 0024's column was NULL for every row.

**0.5.9 landed the field** — Go struct, Zod schema, OpenAPI, fixture, and both
halves of `backend/internal/inventory/repository.go`. No migration: the column
has been in both stores since 0.5.5. It was **not** deferred to 0.6.0, because
the ledger is append-only: every adjustment replaying before the fix loses its
provenance permanently, and no later pass can recover it. See the ADR-018
addendum dated 2026-08-27.

**Why criterion 6 was green while this was broken, which is the part worth
keeping.** The echo comparison could not see it: the handler returns the struct
it decoded, so a field the struct lacks is missing from *both* sides. The
storage comparison could not see it either — its fixture was a wastage entry,
on which every count-provenance field is legitimately null, and a null
round-trips through a nonexistent field perfectly. **Green on absent data, in
the test written to prove fidelity.** So the fixture now carries a
count-sourced COUNT_ADJUSTMENT earned through the shipping count API, and
`packages/contracts/fixtures/` gained a second ledger fixture populated where
the first is null, round-tripped in both drift suites. The pinning test is
deleted, having done its job.

> A fidelity test proves fidelity only for the fields its fixture populates.

---

## 4. ADR-013 — STILL OPEN. Two hardware gates block M3 acceptance

**M3 is code-complete and functionally exercised. It is NOT acceptance-complete.
Do not mark it accepted until both of these clear. Neither can be closed by any
test, harness or emulation — both need physical hardware.** Parked 2026-08-20,
revisit ~2 September 2026; a fresh session must read them as settled, not
re-litigate them.

**(a) Real thermal printer, ESC/POS verified on paper.** The file sink replaces
only the final write; everything upstream is the shipping path, so the bytes are
real, but nothing proves a printer accepts them. Untested: vendor ESC/POS
dialect differences, whether the 80mm layout fits **58mm** paper, the **cutter**,
**codepage** and non-ASCII glyphs, USB and Bluetooth-SPP timing, paper-out and
mid-print disconnect. The spool's retry/backoff has never met a device that
failed for a physical reason.

**(b) Bare 4GB Windows 10 target, for the resource envelope.** The installer half
is done (`bundle.windows`, offline WebView2 embed, static CRT, NSIS-only); the VM
run itself needs a machine nobody has provisioned yet. Untested: the installer
completing **offline**, memory headroom under WebView2 at 4GB, SQLite
open/decrypt latency on a spinning disk, cold start, and crash recovery after a
real power cut. Full checklist in `docs/backlog-m2.md` "Clean Windows 10 VM
validation"; `docs/adr/ADR-013-outlet-deployment-target.md` carries the addendum
and the named fallback.

---

## 5. M2 acceptance item 5 — was silently red, now re-evidenced

**Item 5 ("one real KDS↔edge socket session") had been failing since ADR-017 and
nobody knew.** `tests/integration/kds-lan-bridge` stopped compiling when
`server::start` gained a `DeviceTokenVerifier` and `MenuItem` gained
`tax_profile_id` / `hsn_sac`. The `lan-integration` CI job was failing at
`cargo build` — not proving a socket session at all — while item 5 was recorded
as met.

Fixed: one Argon2id-hashed `device_credential_cache` row seeded, a real
`CachedCredentialVerifier` wired, and the token published on the bridge's ready
line so the driving test presents a genuine credential rather than bypassing the
check. **Re-verified 4/4** against a real socket (`cd tests/integration/kds-lan
&& pnpm test`).

**Still to do:** CLAUDE.md's milestone block should record item 5 as genuinely
evidenced **and** note the period it was falsely green.

Guard added so a tenth break fails fast: **`rust-seams` CI job + `make
check-seams`** compiles every cross-workspace Rust consumer. The repo is
deliberately several cargo workspaces, so `cargo check` in the crate you edit
proves nothing about its callers — this had broken nine times.

---

## 6. Open defects a new session must not lose

Closed since the last resume: invoice enqueue path, split-bill unreachable,
per-line discounts unreachable, `devseed` seeds no printer, the blocking
contiguity check on both ranged streams, the sync-config test that needed a clean
database, and the fail-fast CI job shape that hid four pushes of verdicts.

**Mint-counter wrap: FIXED, not open.** `format_order_display_number`
(`edge/database/src/repo.rs`) uses bijective base-26 blocks, so the formatter
never repeats for any index up to `i64::MAX`, plus a per-business-day counter
reset. Regression test `formatter_never_repeats_past_the_old_wrap_point` drives
past the old collision point (25975, where `#Z999` rolled to `#A1`).

**Contracts are FROZEN at v0.5.9.** Cross-checked against
`packages/contracts/package.json` by `scripts/check-milestone-marker.mjs`, which
fails the build on disagreement — this line was written at 0.4.7 and went stale
within two days.

| Where | What |
|---|---|
| `backend/internal/{auth,menu,tables}` | Never call `postgres.Migrate`; they seed into an assumed schema. CI works around it with a `go run ./cmd/devseed` step. Real fix: have them migrate. |
| `packages/contracts/openapi/openapi.yaml` | **Nothing machine-checks it** against handlers or TS/Go types. Drift check is TS↔Go only. It silently drifted on three `MenuItem` fields for two versions. |
| `edge/database/src/lib.rs` | `Db::connection()` is plain `pub`; three sibling crates hold it. `payment` is trigger-protected (0.4.5); `cash_shift` is not and cannot be — OPEN→CLOSED is a legitimate UPDATE. |
| `apps/pos/src/store/cashShift.ts` | Shift recovery runs on `BillingScreen` mount, not app-global startup. |
| `edge/database` (`payment_allocation`) | Assumes one payment settles at most one invoice. A tender spanning two invoices is unmodelled. |
| `edge/database` | `PAID_IN`/`PAID_OUT` emit no outbox event; they ride inside `CashShiftOpened`/`Closed`. Visibility latency, not a money defect. |
| `edge/sync/src/config.rs` | Empty `device_credentials` is not an error, unlike empty `users` — "none enrolled" and "cloud forgot" are indistinguishable. |
| `edge/database/src/invoice/numbering.rs` | `{OUTLET}` token derives from `outlet.name`; no `outlet.code` in the frozen contract. |
| `edge/database/src/repo.rs` | Display-number reset buckets by **UTC** day, not outlet-local business day. Same limitation in `business_date_from` (`commands/billing.rs`). |
| config authoring | A non-`NEVER` `reset_policy` whose prefix lacks the matching date token yields duplicate invoice numbers across periods. Caught by the UNIQUE index; not validated at config-write time. |
| `backend/internal/compliance` | Writes gate on `outlet.manage`; **no `billing.manage`** exists in the frozen `Permission` enum. Whoever may rename a table may set the GSTIN printed on every invoice. |
| `apps/pos/src-tauri/tests` | `cargo fmt --check` reports pre-existing diffs; not CI-enforced (crate is not built on Linux, ADR-013). Same for the two test bridges under `tests/`. |
| `tests/e2e-scenario` | The HSN check is folded into `9_tax_reconciliation`, so a tax arithmetic error and a missing compliance field share one invariant id. |
| `apps/pos` | **No dev-server smoke test.** Blocks criteria 4 and 5 (§2a). Filed in `docs/backlog-m2.md` with the constraint that matters: it must drive `pnpm dev`, not the build — `optimizeDeps` is dev-server-only and `vite build` never reads it. |
| `tests/e2e-scenario` | `cancel_kitchen_items_with_outbox` still has no Tauri command; `#132-C` cancellation is unreachable from the shipped surface. |

### Operational gate — read before any rollout

`menu_item.hsn_sac` is NULL on every row of every existing edge database, and the
edge **rejects invoice issuance** when any line's code is NULL or blank. **No
outlet can issue any invoice until its catalogue is configured.** That is correct
and deliberate (no fallback: a wrong code that looks configured is worse than a
missing one), but catalogue configuration must be part of any rollout.

Same shape applies to printing: an outlet with **no `BILL`-role printer** cannot
print a bill, and `print_invoice` fails loudly by name rather than queueing into
nothing.

---

## 7. Process notes

Carried forward, all still binding:

- **When you add a check, ask what it makes invisible if it fails.** Steps in a
  GitHub Actions job are fail-fast, so a cheap check in front of an expensive one
  withdraws the verdict from everything behind it. Style lives in `*-style` jobs
  beside the test jobs, never in front of them. That question is now written at
  the top of `ci.yml`.
- **A gated target nothing invokes is a target that does not exist.**
  `required-features` hides a target from `cargo test` *and* from
  `cargo clippy --all-targets`, and it is not reported as skipped.
  `scripts/check-gated-tests.mjs` fails the build if any gated feature or test
  target is not named on a `ci.yml` run line. Two gated targets today:
  `cloud_replay` (`cloud-e2e`) and `crashpoint` (`crash-points`).
- **Falsify before trusting, then check what actually failed.** A red test during
  falsification is not confirmation — the failure must be the assertion under
  test, at the field you broke. Twice now the first red was the harness.
- **A contract change is a multi-crate change.** Enumerate consumers, build them,
  list them in the ADR. Run `make check-seams`.
- **Build-green ≠ dev-works for the Tauri/web apps.** The build output, the dev
  server and the browser are three runtimes; a failure in one is invisible from
  the others. Say which runtime a frontend change was observed in. First move on
  a blank Tauri window: check `node_modules/.vite` mtime against its source, and
  check the Network tab, not only the console.
- **Anything touching a persistent store must mint unique ids or make its own
  database.** CI's fresh service container supplies a clean state that no test
  states as a requirement, so such a test is green in CI and red for every human.
- **An invariant nobody has watched fail is not a gate**, and a green invariant
  whose subject never occurred is worse than no invariant. Count the shapes.
- **Verify the runner, not the file.** A migration on disk but absent from
  `MIGRATIONS` never applies (0009–0011 sat dead; 0005 before them).
- **Never quote a number without the command and the date.**

Docker is not started automatically after a restart. On this box Docker Desktop
itself must be launched first (`Docker Desktop.exe`), then
`docker compose up -d postgres redis nats`.

---

## 8. Repo hygiene

Untracked and **not created by any Holler track** — decide whether they are
wanted: `.vscode/`, `.github/copilot-instructions.md`, `.github/instructions/`,
`HOLLER_DEV_MENU_SPEC.md`, `imgs/og.png`, `holler-website-v8.html`, and a set of
`website/holler-website-v*.html` plus `website/holler-site/`.

Prune: five `worktree-agent-*`, `wip/edge-database-stash`, and
`wip/t13-retry-partial` (`ca6c44a`, does not build — T13 was redone from scratch;
the branch is dead).

Dev conveniences, both opt-in and off by default:
`scripts/dev-bootstrap.ps1 -WithBilling` (seeds tax/fiscal/series/discounts/
printers — required before the POS can issue any bill) and
`-PrinterFileSinkDir <dir>` (routes prints to files).
