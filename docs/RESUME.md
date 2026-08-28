# M5 resume state — 2026-08-28

`main` is at **`6ecbb20`**, tagged **`m4-complete`**. Read this file first, then
`docs/M5_HANDOFF.md`, `docs/adr/ADR-018-m4-inventory-contracts.md`, and the
2026-08-27 / 2026-08-28 entries in `docs/retro.md`.

**This file replaced an "M4 resume state" header dated 2026-08-24.** M4 content
that is now closed has been collapsed to a line; everything still open is carried
forward verbatim. M3 is still not acceptance-complete and its defects are still
listed.

---

## 1. M4 is ACCEPTED and tagged — all seven criteria observed

CLAUDE.md's rule for this milestone: *every item is an observed behaviour, not
an implemented API, and none may be evidenced by a test harness* — an acceptance
run exercises the binaries that ship (`docs/retro.md`, 2026-08-11).

**Seven of seven met.** Criterion 1 was the last to close and was CONTESTED for
four days; the history is worth keeping and is below the table.

| # | Criterion | Verdict | Evidence — where, and what it exercises |
|---|---|---|---|
| 1 | Offline sale from the **real seed menu** deducts every ingredient at recipe × line quantity, plus chosen-modifier deltas, and nothing for modifiers with no delta row | **MET** | Closed by `7e88d1c` — the till now resolves a variant by **cardinality** (0 → null, 1 → silent, 2+ → mandatory picker), so a sale through the shipping binary writes ledger rows. Re-observed by hand in the running POS. Harness `edge/database/tests/seed_offline_sale.rs` still covers the resolver; it is corroboration, not the evidence. |
| 2 | Kill the POS between confirm and deduction → order and ledger agree on reopen | **MET** | `edge/database/tests/crash_durability.rs` against `src/bin/crashpoint.rs` — a **real `abort()` of a real child process** at `after_confirm_before_deduct`, judged on reopen. Gated behind `--features crash-points`; CI job `crash-durability`, on **windows-latest**, because WAL recovery is OS-specific and outlets run Windows (ADR-013). 2/2, 2026-08-24. Premise re-confirmed structurally on 2026-08-27: `deduct_stock_for_confirmed_order` is `pub(crate)` with exactly one call site, inside `confirm_order`'s transaction before `tx.commit()`. No serve path can insert. |
| 3 | Physical count produces a variance report whose arithmetic is checked against an **independently computed figure** | **MET** | `edge/database/src/stock/variance.rs::variance_matches_an_independently_computed_figure` — the report's numbers recomputed by a second route and compared, not spot-checked. Surface `apps/pos/src/components/StockCountScreen.tsx`, routed at `router.tsx:101`. TypeScript formats `variance_percentage_bps`; it never recomputes it. |
| 4 | An ingredient crossing its reorder level is **visible to a human on the POS** | **MET** | `LowStockBanner` mounted in `PosScreen.tsx` and `OrderListScreen.tsx`; `CurrentStockScreen` routed at `router.tsx:80`. **Observed rendering in the running POS 2026-08-27** — `pnpm tauri dev` / WebView2, screenshot filed. Reaching it required two fixes: the dev principal lacked `inventory.manage`/`inventory.count` (`a6e02d7`), and the Tauri `MenuItem` DTO was missing `tax_profile_id`/`hsn_sac`, which rejected every menu load. |
| 5 | An item sold with no recipe completes the sale, records a gap, and appears on the "items sold with no recipe" report | **MET** | Sale completion and `stock_deduction_gap` are edge-side and tested (`edge/database/src/deduction/ledger.rs`, `apps/pos/src-tauri/src/commands/inventory.rs`). Report is `StockDeductionGapsScreen.tsx`, routed at `router.tsx:108`. **Observed rendering in the running POS 2026-08-27.** The `NO_RECIPE` path itself became exercisable only after `7e88d1c`; before it every row read `NO_VARIANT`. |
| 6 | Ledger entries created at the edge replay to the cloud and **read back identically** | **MET, AND FALSIFIED** | `edge/sync/tests/cloud_replay.rs`. Builds and spawns the real `cmd/api` against real PostgreSQL, logs in, enrolls a device through the real ADR-017 route, and drives `SyncWorker::pump_ranged_streams` at it over a real socket. The entry is *earned* through `Db::record_wastage`, so `entry_seq` comes from the real counter (asserted to be 1, not 0). Read back **twice**: the 201 echo and the PostgreSQL row re-serialised, both whole-object byte-compares. Gated `--features cloud-e2e`; CI job `cloud-replay`. 2/2, 2026-08-24. |
| 7 | Stock reads stay bounded after a sealed snapshot — **measured, not asserted** | **MET** | `edge/database/src/stock/snapshot.rs::stock_reads_stay_bounded_after_a_sealed_snapshot` — counts **SQLite VM steps** taken by the shipped read over 5 sealed days vs 400, same unsealed tail. No clock is timed, so the figure is identical on a fast laptop and a 4GB spinning-disk till, and no regression can hide behind a generous margin. |

### Criterion 1 — why it stood CONTESTED for four days

`variantId` was hardcoded `null` at both `addItem` call sites in `PosScreen.tsx`.
A recipe binds to `menu_item_variant_id` (NOT NULL, migration 0015), so
resolution returned `GapReason::NoVariant` for **every** dish and no sale the POS
ever took wrote a ledger row. The milestone's headline behaviour had never
happened through the binary that ships.

The harness could not see it: `seed_offline_sale.rs` selects a variant directly.
Green and correct, testing a path the product does not take.

Resolution is by **cardinality, not by default**. `is_default` **preselects** in
the picker and never **resolves** — the naive fallback turns a stock defect into
a revenue defect, since Half at 18000 paise against Full at 32000 would sell as
Full whenever nobody chose, and print a wrong bill. **A wrong bill is worse than
a missing deduction.**

> A deduction test proves deduction only for the path its caller takes.

### Criterion 6 was falsified, not merely observed

`entry.Note` was replaced with a typed nil in the cloud's INSERT — one field
dropped **server-side, after the echo** — and the **storage** comparison failed
and named `note`, while the **201 echo** comparison **passed**, because the
handler echoes the entry it was handed and never reads the row back. That is the
entire argument for keeping two checks rather than one.

The falsification pass also found a defect in the harness itself — both tests
built `cmd/api` to one path on parallel threads, a race that can only fire when
the Go sources change, i.e. only during falsification. Fixed with
`OnceLock::get_or_init`; written up in `docs/retro.md` 2026-08-24.

### Test counts — command and date, or it is not a number

| Suite | Result | Command | Measured |
|---|---|---|---|
| `apps/pos` | **206** | `pnpm test` | **2026-08-28** |
| `edge/database` | 262 | `cargo test` | 2026-08-24 |
| `edge/database` crash durability (criterion 2) | 2 | `cargo test --features crash-points --test crash_durability` | 2026-08-24 |
| `edge/sync` | 42 | `cargo test` | 2026-08-24 |
| `edge/sync` cloud replay (criterion 6) | 2 | `cargo test --features cloud-e2e --test cloud_replay` | 2026-08-24 |
| `edge/printer` | 45 | `cargo test` | 2026-08-24 |
| `edge/device` | 11 | `cargo test` | 2026-08-24 |
| `apps/pos/src-tauri` | 70 | `cargo test` | 2026-08-24 |
| `backend` | 13 packages ok, 0 fail (267 top-level test funcs across 19 pkgs) | `go test -count=1 ./...` with `HOLLER_TEST_DATABASE_URL` set | 2026-08-24 |

`apps/pos` moved 182 → 190 (`7e88d1c`) → 200 (`d1881b1`) → 206 (`afb5aa0`).

**Every Rust and Go figure above predates the M4 close and should be re-run
before it is quoted again.** **NOT re-run at all since 2026-08-24, and therefore
not evidence:** `apps/kds`, `packages/contracts` (TS + Go), and the e2e scenario
harness.

**The `LNK1104` retry is expected on this box and is not a code error.** McAfee
holds a lock on each freshly-linked test binary; compilation has already
succeeded when it fires, and two or three re-runs reach green because cargo
caches every binary that did link.

---

## 2. What is open right now

**(a) `gh` is installed but not authenticated.** `gh auth login` has not been run
on this machine, so `gh run list` still cannot read this repo's CI. The whole
point of installing it — `docs/retro.md` 2026-08-23, *"Install the CLI that reads
your own CI"* — is unmet until that one interactive command is run. Until then a
push is fire-and-forget, which is the root cause of the four pushes that spent a
day producing no verdict. **Report the CI verdict in the same message as the
commit; a push whose result nobody read is not a push.**

**(b) The `m4-complete` tag is local and has not been pushed.**

**(c) The two ADR-013 hardware gates in §4 remain open.** They block **M3**
acceptance, not M4.

**(d) Seeded reorder levels are placeholders** that make 28 of 32 items read LOW.
Criterion 4's surface is correct; the data behind it is not meaningful. Set real
levels before any rollout or demo, or the banner trains people to ignore it.

**Closed since the last resume:** criterion 1 through the till (`7e88d1c`), stale
stock queries after a sale, negative stock never surfacing without a configured
reorder level, the 1000x VOLUME display defect and the test assertion that
encoded it (`d1881b1`), and the two unlabelled quantity inputs (`afb5aa0`).

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
provenance permanently, and no later pass can recover it.

**Why criterion 6 was green while this was broken.** The echo comparison could
not see it: the handler returns the struct it decoded, so a field the struct
lacks is missing from *both* sides. The storage comparison could not see it
either — its fixture was a wastage entry, on which every count-provenance field
is legitimately null, and a null round-trips through a nonexistent field
perfectly. **Green on absent data, in the test written to prove fidelity.**

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
real power cut. Full checklist in `docs/backlog.md` "Clean Windows 10 VM
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

## 6. Open defects — MOVED

**The register is `docs/backlog.md`, and it is the only one.** This section held
one of four overlapping lists; the table that lived here moved there wholesale on
2026-08-29, along with `docs/backlog.md` (deleted) and the M5 planning triage.
Nothing was dropped in the move — items were carried with their provenance.

**Do not re-open a list here.** Four registers is how an item gets triaged twice
and scheduled never, which is the failure that prompted the consolidation.

Closed since the last resume: invoice enqueue path, split-bill unreachable,
per-line discounts unreachable, `devseed` seeds no printer, the blocking
contiguity check on both ranged streams, the sync-config test that needed a clean
database, and the fail-fast CI job shape that hid four pushes of verdicts.

**Mint-counter wrap: FIXED, not open.** `format_order_display_number`
(`edge/database/src/repo.rs`) uses bijective base-26 blocks plus a per-business-day
counter reset; `formatter_never_repeats_past_the_old_wrap_point` drives past the
old collision point (25975).

**Contracts are FROZEN at v0.6.0**, cross-checked against
`packages/contracts/package.json` by `scripts/check-milestone-marker.mjs`.

---

### 6.1 Six permitted-but-unwritten `entry_type` values — M5's first schema task

`stock_ledger_entry.entry_type` permits `PURCHASE`, `TRANSFER_IN`,
`TRANSFER_OUT`, `RETURN_TO_VENDOR`, `PRODUCTION_CONSUMPTION`,
`PRODUCTION_OUTPUT`. **Nothing writes any of them.** That takes the "contract
permits it, nothing produces it" class to **eleven** across M4, from the five
`check-contract-field-consumers.mjs` was written against.

Measured 2026-08-28: all six appear in the consumer roots **only** in a doc
comment enumerating the CHECK constraint (`edge/database/src/model.rs:1248-1250`),
plus `"PURCHASE"` once in a test fixture
(`edge/database/src/stock/variance.rs:150`).

**Order matters, or the check ships inert:** narrow the corpus (exclude doc
comments and `#[cfg(test)]`) **first**, then extend the check to enum values,
**then** declare the six as exempt with `M5` (procurement, transfer) and `M8`
(central kitchen) named. Full item in `docs/M5_HANDOFF.md` 2.2.

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
- **Enumerate the sinks, not the surfaces**, to prove a UI-level concern is
  covered. A screen can be missed; a write path cannot. Now in CLAUDE.md; worked
  example in `docs/retro.md` 2026-08-28.
- **A gated target nothing invokes is a target that does not exist.**
  `required-features` hides a target from `cargo test` *and* from
  `cargo clippy --all-targets`, and it is not reported as skipped.
  `scripts/check-gated-tests.mjs` fails the build if any gated feature or test
  target is not named on a `ci.yml` run line. Two gated targets today:
  `cloud_replay` (`cloud-e2e`) and `crashpoint` (`crash-points`).
- **Falsify before trusting, then check what actually failed.** A red test during
  falsification is not confirmation — the failure must be the assertion under
  test, at the field you broke. Twice now the first red was the harness.
- **A wrong assertion is worse than no test**, because it makes the defect look
  verified. Derive the expected value from the spec, never from what the function
  currently returns.
- **A contract change is a multi-crate change.** Enumerate consumers, build them,
  list them in the ADR. Run `make check-seams`.
- **Build-green is not dev-works for the Tauri/web apps.** The build output, the
  dev server and the browser are three runtimes; a failure in one is invisible
  from the others. Say which runtime a frontend change was observed in. First
  move on a blank Tauri window: check `node_modules/.vite` mtime against its
  source, and check the Network tab, not only the console.
- **Anything touching a persistent store must mint unique ids or make its own
  database.** CI's fresh service container supplies a clean state that no test
  states as a requirement, so such a test is green in CI and red for every human.
- **An invariant nobody has watched fail is not a gate**, and a green invariant
  whose subject never occurred is worse than no invariant. Count the shapes.
- **Verify the runner, not the file.** A migration on disk but absent from
  `MIGRATIONS` never applies (0009-0011 sat dead; 0005 before them).
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
