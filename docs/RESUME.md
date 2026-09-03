# M5 resume state — 2026-09-02

> **M5 IS CLOSED at contracts v0.6.3 (2026-09-02); contracts are now v0.6.4, see below. ALL SEVEN ACCEPTANCE CRITERIA
> ARE OBSERVED against the shipping binaries, none by a test harness.**
> **The evidence is `docs/m5-acceptance.md`. Read that file. Do not reconstruct
> the verdicts from git history — this session did exactly that after a restart
> and reported four observed criteria as unobserved, while holding the commit
> (`262e03a`) made *because* of the run that observed them.**
>
> Criteria 1, 3, 4 and 6 were observed on real screens on 2026-09-02 by the
> operator; 2, 5 and 7 on the dates in that file. Criterion 1 is the first time
> the "network disconnected" precondition was ever established in this project:
> backend stopped **by PID**, `scripts/check-cloud-unreachable.ps1` agreeing on
> three probes, after the same script was watched printing `STOP` with the cloud
> up.
>
> **§2(a) below is SUPERSEDED and kept only as the record of what was open before
> the pass.** Everything it lists as pending is either observed (see the
> acceptance file) or filed in `docs/backlog.md` as a pilot blocker. The
> carry-forward list is in `docs/m5-acceptance.md` and in the CLAUDE.md milestone
> block. **Next work is the M6 kickoff, not another acceptance run — criterion 6
> must not be re-run.**
>
> Close-out verification, executed 2026-09-02 after `262e03a`: `edge/sync` 56
> passed 3/3 consecutive runs, `edge/database` green, `edge/device` 11 passed,
> `edge/printer` 45 passed, and all three seam manifests `cargo check` clean.
> Two traps that cost time and will again: **there is no workspace `Cargo.toml`
> at the repository root and `make` is not on PATH in the Bash tool**, and both
> failures can exit **0** through a pipe, so a green-looking line can prove
> nothing. That is now structural: **every test step runs through
> `node scripts/assert-tests-ran.mjs`, which fails on zero executed tests**, in
> `ci.yml`, in all three builder agent files and in the verifier's rubric.
>
> The `stale_connection.rs` failure was **not** a flake and **not** the ADR-013
> hardware finding it was reclassified as. Measured: it failed on an idle machine
> ~1 run in 30, in 0.00s, on `os error 10054` — the test's own fake server
> answered after a single `read`, and closing a socket with unread bytes RSTs the
> connection and **discards the reply already in the send buffer**. Fixed by
> draining the whole request in both fake servers in that file; 0 failures in 200
> idle runs and 0 in 100 under 48 busy loops on 24 cores. No timeout was ever
> involved, and no retry budget was ever at risk — transport failures are
> classified transient and charge nothing.

Contracts are **FROZEN at v0.6.4** (ADR-023 added `sync_outbox_block` for M6 A3;
ADR-021 remains the M5 baseline); migrations run
through **sqlite 0031 / postgres 0031**. **ALL 16 CI JOBS ARE GREEN** as of
`310d3a1` (run 33335138157, 2026-08-30) — the first fully green run in the
repository's readable history, and the first `e2e-scenario` pass since at least
2026-08-12. All four standing red jobs are fixed and CI-confirmed (§2b).
Clearing CI was the precondition for the M5 acceptance pass: an acceptance
verdict against an unreadable build is worth nothing. **That precondition is now
met, and the acceptance pass in §2(a) is the next work.** Read this file first, then
`docs/backlog.md` (THE ONLY BACKLOG), then
`docs/adr/ADR-019-m5-procurement-contracts.md` **including its three addenda**,
then the 2026-08-29/30 entries in `docs/retro.md`.

**Every M5 track has landed: T1, T2, T3, T4, T5a, T7a, T7b**, and the acceptance
pass is COMPLETE — seven of seven, `docs/m5-acceptance.md`. (The sentence here
previously read "MID-FLIGHT and has produced no verdicts yet"; that was true when
written and was still being read as current after the pass finished, which is the
whole reason the acceptance file exists.)

**`apps/admin` does not exist and is M6.** T5 was written as "add supplier and PO
screens" and is really "create the web admin application, then add them" — a
milestone, not a track. Until it lands, **purchase orders are raised through the
API**, and acceptance criterion 5 says so rather than being quietly re-scoped.

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

**Every command in this table now runs through
`node scripts/assert-tests-ran.mjs <runner> -- <command>`, which FAILS when a
command executes zero tests** — a filter matching nothing, a gated-out target, a
suite that skipped everything, or a command that never ran at all all exit 0
otherwise (CLAUDE.md; `docs/retro.md`, 2026-09-02). **Record the count executed,
never "passed".** And never pipe a test command through `tail`: the pipeline
reports `tail`'s status, which is how two suites that could not run at all were
reported green on 2026-09-02.

| Suite | Result | Command | Measured |
|---|---|---|---|
| `apps/pos` | **230** | `pnpm test` | **2026-09-02** |
| `apps/kds` | **30** | `pnpm test` | **2026-09-02** |
| `packages/contracts` | **67** | `npx vitest run` | **2026-09-02** |
| `edge/database` | **316** | `cargo test` | **2026-09-02** |
| `edge/sync` | **56** | `cargo test` | **2026-09-02** |
| `edge/printer` | **45** | `cargo test` | **2026-09-02** |
| `edge/device` | **11** | `cargo test` | **2026-09-02** |
| `apps/pos/src-tauri` | **80** | `cargo test` | **2026-09-02** |
| `apps/pos` (superseded) | 206 | `pnpm test` | 2026-08-28 |
| `edge/database` (superseded) | 262 | `cargo test` | 2026-08-24 |
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

**(a0) THE PASS WAS REPLANNED ON 2026-08-31: `edge/sync` HAS NO HOST.** Nothing
that ships calls it, in either direction — see `docs/backlog.md` and
**ADR-020**, which rules that the worker is hosted **in the POS Tauri process**
(ADR-013: one executable on a 4GB box beats clean separation) and must **drain
the outbox on graceful shutdown and again on next launch before anything else**,
so the guarantee is "your day reaches the cloud at both ends of every trading
day" rather than "syncs while the till is open".

Two consequences for this section, both of which change what may be claimed:

- **M1's offline-replay acceptance item is UNEVIDENCED and always was.** "WiFi
  off → order → WiFi on → verify cloud-side" could not be performed: `local_outbox`
  is written by many paths and drained by none, and `repo::mark_outbox_published`
  has zero callers outside `edge/sync` and tests. The second half of that sentence
  had nothing to make it happen. Re-run it once the host lands.
- **Criterion 6 is BLOCKED, not pending.** A harness is the only thing that can
  currently trigger a replay, and M5 forbids harness evidence.

**CAVEAT ON CRITERIA 1, 2, 3, 4 AND 7 — record it with the observations, never
after.** These five are offline-only and may be banked before ADR-020 lands. But
they run against **LOCALLY-SEEDED EDGE DATA**, so they evidence offline behaviour
and say **nothing** about config transport. **Two claims, two pieces of evidence.**
Keeping them apart is what stops "the pickers populated" being read as "the pipe
works" — and after both seeders have run under matching ids, a row's presence is
not evidence that it travelled. Proving transport needs an empty start and a real
pull, which is why it waits on the host.

**(a1) CRITERION 2 IS OBSERVED, BOTH WAYS (2026-09-01).** Driven through
`crashpoint --grn`, which calls `Db::record_goods_receipt` -- the same entry point
the POS uses -- against a copy of the real sealed edge database, and read back by
an independent reopen with `sqlite3`:

| Run | `goods_receipt_note` | `grn_line` | `stock_ledger_entry` |
|---|---|---|---|
| abort at `after_grn_before_ledger` | 0 | 0 | 0 |
| abort at `after_ledger_before_commit` | 1 | 1 | 1 |

Positive row: `PURCHASE | 5000000000 micro | unit_cost 4 paise | entry_seq 21 |
origin GOODS_RECEIPT | business_date 2026-08-23`. Both aborts terminated
abnormally (`0xC0000409`), not by a clean exit.

**The second crash point was added for this run and is the point of it.** Before
it, criterion 2 rested on "0 rows everywhere" plus a `UNIQUE` violation -- and 0
rows is exactly what a receipt path that silently wrote nothing would also
produce. `AFTER_LEDGER_BEFORE_COMMIT` fires after the commit and before the seal,
the only window in which committed rows are on disk and readable: aborting inside
the transaction rolls back and proves as little as no control at all, and a clean
exit seals and takes the plaintext with it. **An absence means something only when
the same reopen can be shown to find the rows when they were written.**

Caveat, per (a0): this is offline behaviour against locally-seeded edge data. It
says nothing about transport.

**(a2) CRITERION 5 IS OBSERVED, INCLUDING THE AMEND PATH (2026-09-01).** Against
the API, per the criterion's own scoping -- `apps/admin` does not exist and is M6,
so purchase orders are raised through the API and that is the honest statement of
what M5 delivers.

Seeded for it: role `BUYER` with `po_approval_limit_paise` 5,000,000 (Rs 50,000)
holding `procurement.manage` + `procurement.approve`, on user `buyer@holler.test`;
role `OWNER` at 50,000,000 (Rs 500,000) with `procurement.approve` and
**deliberately no user** -- `RolesAbleToApprove` selects role rows, so a role with
no holder is enough to be named. Two ceilings are required, not one: with a single
role the refusal is correct and names nobody, which is the half of the message
that tells the caller what to do next.

Sequence, all observed:

1. PO raised at 2,500,000 paise (Rs 25,000), **under** the buyer's ceiling ->
   `PENDING_APPROVAL`.
2. Buyer approves -> `APPROVED`, `approved_by_user_id` set, `approved_at` set.
3. Amend upward to 10,000,000 paise (Rs 100,000) -> **`PENDING_APPROVAL`, and
   `approved_by_user_id` / `approved_at` both cleared.** Confirmed by an
   independent PostgreSQL read, not only the response body. This is the
   security-shaped hole T5a closed, and it was previously evidenced only by a Go
   test.
4. Buyer re-approves -> **403**, with all three section 64 elements present:

```
{"code":"po_exceeds_approval_limit","message":"this purchase order exceeds your
role's approval limit: this purchase order totals 10000000 paise and your role's
approval limit is 5000000 paise. Next: ask one of these roles to approve it
instead: [Owner]","total_paise":10000000,"limit_paise":5000000,
"can_be_approved_by_roles":["Owner"]}
```

A fifth thing fell out of the run and is worth keeping: **the amend route refuses
to grant approval.** `PATCH` with `status: "APPROVED"` is rejected with *"may only
be reached through POST /procurement/purchase-orders/{id}/approve"*, so the
revocation in step 3 cannot be undone through the same call that triggers it.

Caveat, per (a0): cloud-side only. Says nothing about config transport.

**(a3) ADR-020 IS IMPLEMENTED: `edge/sync` HAS A HOST (2026-09-01).** The first
time in five milestones that anything shipping constructs a `SyncWorker`.

- `apps/pos/src-tauri` now depends on `holler-edge-sync`. Before this, NO
  `Cargo.toml` in the repository did.
- The worker lives in `AppState` beside the `LanServerHandle`, built from
  `HOLLER_CLOUD_BASE_URL` + `HOLLER_TENANT_ID` + `HOLLER_DEVICE_TOKEN`. All three
  or none: a worker with a URL and no credential 401s every request and burns
  retry budget doing it. Absence disables sync and is **never fatal to startup**,
  the same rule the LAN server follows.
- `Mutex<Option<SyncWorker>>`, because `SyncWorker` keeps its enrollment flag in a
  `Cell` and is `Send` but not `Sync`, while Tauri managed state must be `Sync`.
  Wrapped at the consumer rather than changing that `Cell`: the sync crate
  documents itself as driven by one caller, and this host is that caller.
- Drains on launch (`AppState::open`) and on `RunEvent::Exit` **before**
  `shutdown_in_place`.

**Both ADR claims falsified, not asserted** —
`apps/pos/src-tauri/tests/adr020_outbox_drain.rs`, 2/2 against a real encrypted
file database and a real `tiny_http` cloud:

1. **Ordering.** Same state, same worker, same cloud: **3 rows published before
   the seal, 0 ingest calls after it**, with three rows deliberately left pending
   so "published nothing" cannot be read as "had nothing to publish". **The ADR
   was wrong about the failure mode and is corrected:** a post-seal drain does not
   silently do nothing, it **panics** -- `Db::connection` is
   `expect("edge database handle used after shutdown")` (`lib.rs:208`), observed
   firing. Worse and louder, same conclusion: nothing replays.
2. **Boundedness.** Pointed at a refused port with 5 rows pending, the drain
   returns rather than hanging, and **all 5 rows survive** -- giving up is not
   discarding.

`cargo check --all-targets` clean on all three seam manifests.

**What this does NOT do, and must not be read as doing:** the config pull still
has no caller, so the inbound half is unhosted and the
inventory-config-push backlog item stays open. There is also no periodic pump
yet -- a till open all day whose uplink returns mid-service does not replay until
it closes. Criterion 6 is now *reachable*; it is not yet *observed*.

**(a4) CRITERION 7 IS OBSERVED, AND 0.6.3 FIXED THE ARITHMETIC UNDER IT
(2026-09-02).** The live figure — 13 paise/g over three receipts — matched the
invoices exactly. Checking WHY it matched found two problems the criterion could
not see.

1. **Per-receipt rounding.** `unit_cost_paise` is a RATE, rounded to whole paise
   once per receipt, and the average summed that rate. The error is ±0.5 paise on
   a per-gram figure, so it scales inversely with price: **+20% at 2.5 paise/g**,
   one-directional per item, worst on cheap staples. The acceptance dataset
   passed only because 10, 10 and 18 divide evenly — it was chosen to make the
   average vary, not to make the rounding fail. Fixed by ADR-021 / contracts
   0.6.3: the ledger stores `line_total_paise` and the division happens once.
2. **An undocumented definition.** The averaging query is UNBOUNDED, so what is
   implemented is a **lifetime cumulative purchase-weighted average, not weighted
   average cost of stock on hand**. Only half of that was ever decided. NOT
   folded into 0.6.3; filed in `docs/backlog.md` against the first pilot.

**Criterion 7 is definition-neutral as written** ("after two receipts at
different prices") so it passes under either definition and cannot report which
one shipped. That is the retro line for this milestone: an acceptance criterion
satisfied by either of two definitions cannot tell you which one you built.


**(a) THE M5 ACCEPTANCE PASS IS MID-FLIGHT AND NOTHING IS OBSERVED YET.** Zero of
seven criteria are acceptance-observed. Rows 3 and 4 are PARTIALLY observed —
T4 drove the shipping screens in real Chromium against the dev server and saw the
`4 SACK -> 200000g` echo and eight gap rows with eight distinct titles — but
**Tauri IPC was stubbed**, so no edge write was exercised. Rows 2, 5, 6, 7 have
test evidence only, and M5's own rule says **none may be evidenced by a test
harness**. The planned pass is:

1. Seed supplier + PO in the cloud, sync down, **confirm the receiving pickers
   POPULATE** rather than falling back to typed UUIDs. Nothing has ever
   demonstrated this: the config fix is verified only by a static guard
   comparing json tags to struct fields, and **a guard can be green while
   nothing has crossed the wire**. Empty pickers is a worse defect than the one
   that was fixed.
2. Network off, receive 4 SACK against that PO, confirm `PURCHASE` ledger rows at
   the converted 200,000 g. **Then criterion 2 in the same receipt** — kill the
   POS between the GRN write and the ledger post, reopen, confirm they agree.
3. Receive with no PO: receipt stands, gap recorded, gap screen names the reason.
4. Reconnect, confirm replay, and **watch whether the offline attempts burned
   retry budget**. If they did, offline is being classified as permanent, which
   would mean a disconnected outlet quietly strands its own receipts — the exact
   failure the per-entry budget exists to prevent.

**(b) CI: ALL FOUR RED JOBS DIAGNOSED AND CONFIRMED GREEN (2026-08-31).**
Run **33335138157** on `310d3a1` is green on all 16 jobs. Every claim in this
section is now verdict-backed rather than local-run-backed.
`gh auth` is done (account `gauravd2k1`), so `gh run list --repo
gauravd2k1/Holler` works.

> **CORRECTION.** The table that stood here recorded `cloud-replay` and
> `edge-style` as "fixed `1e6455b`". **They were not.** Both were still red on
> every run after that commit, including the one on this file's own push. The
> claim was written from a local run and never checked against a verdict — the
> precise failure this whole section exists to prevent, committed inside the
> section warning about it.
>
> A full sweep of all 16 jobs across the 37 runs in the blind window then found
> the other two entries understated as well. **`e2e-scenario` has no green run
> in the last 57 runs, back to 2026-08-12** — not "red since 27 Aug", but never
> observed passing at all.

| Job | Red for | Root cause | Fixed by |
|---|---|---|---|
| `e2e-scenario` | **36 of 37** runs; no green run since at least 2026-08-12 | **THREE faults stacked, each hidden by the one in front.** (1) The harness minted its own `BAR` station, colliding with devseed's on `UNIQUE (station.outlet_id, station.code)`, and died at startup. (2) Behind it, Node 20 has no global `WebSocket`. (3) Behind that, **the job had no build step**: the orchestrator spawns the harness with `cargo run`, which compiles on demand *inside* the 180s ready timeout, and `rust-cache` does not save on failure — so the job failed, saved no cache, and compiled cold next run. **A closed loop: fixes (1) and (2) were both correct and both landed into a job that would time out regardless.** | `66749b0`, `c0caeab`, `310d3a1` — **all three confirmed green, run 33335138157** |
| `lan-integration` | **35 of 37** | Node 20 has no global `WebSocket`; the suite deliberately takes no `ws` dependency and needs Node 22. All four tests died on `ReferenceError`. | `47eec2f` |
| `cloud-replay` | **27 of 37** | `1e6455b` added the three 0.6.0 provenance columns to `ranged.rs` but not to `edge_row_as_wire`, the test's hand-written mirror of it. | `a83ea22` |
| `edge-style` | **26 of 37** | `tests/support/` compiles into every test binary; unused helpers are dead code, and CI clippy runs `-D warnings`. | `66749b0` |
| `backend` | 5 of 37 | Transient: 0.6.0 schema landed before the backend caught up. Self-healed at `042d83e`. | — |
| `contracts` | 7 of 37 | Transient: event-type guard. Self-healed at `9e9bcde`. | — |
| `backend-style` | 1 of 37 | Transient. | — |
| the other 9 jobs | **0 of 37** | Clean throughout the window. | — |

**Four lessons the sweep produced, all worth more than the fixes:**

- **A DEADLOCK CAN MAKE CORRECT FIXES LOOK WRONG.** `e2e-scenario` needed three
  fixes and each was invisible until the one in front of it landed. Worse, the
  third fault meant the job could not go green no matter what else was repaired
  — so a fix that produced no change in the verdict was not evidence the
  diagnosis was wrong. **When a job has never been green, do not assume the
  current error is the only one; assume it is the first of N.**
- **ONE ROOT CAUSE SPANNED TWO JOBS THAT LOOKED UNRELATED.** A WebSocket
  `ReferenceError` in one and a UNIQUE constraint on `station.code` in the other
  were one Node pin apart. Fixing the visible failure in `e2e-scenario` is what
  made the second one visible — the constraint violation killed the harness
  before any scenario reached a socket. **Do not assume the visible failures are
  the whole bill, and do not assume distinct symptoms are distinct causes.**
- **A SWALLOWED STDERR COSTS DAYS.** The harness child's stderr is `inherit`ed
  and absorbed by vitest, so no cargo output ever reaches the job log. A cold
  build was therefore indistinguishable from a hung harness and was reported as
  the latter. And the bridge's two timeouts — startup (180s) and per-request
  (30s) — emitted the SAME sentence, so the failure could not say which phase
  it was in. Both are fixed; each timeout now names its phase.
- **`e2e-scenario` FAILS QUIETLY.** The run completes, all 50 scenarios execute,
  the invariant count reads zero violations, and the `WebSocket` errors land in a
  "fatal (harness-level, not invariant)" bucket. The summary looks like a passing
  run that happened to fail. **An invariant whose subject never occurred is worse
  than no invariant**, and for eleven days every KDS-touching scenario had none.

Seven jobs still pin Node 20 and pass on it; that was left alone deliberately
rather than swept. Do not read a green local suite as a green build — see the
`lan-integration` entry in §5, where a hand-run 4/4 on Node 24 stood as evidence
while CI on Node 20 proved nothing.

**(c) `cloud-replay` caught contracts 0.5.9 happening A THIRD TIME.**
`edge/sync/src/ranged.rs`'s ledger replay payload never carried `source_grn_id`,
`source_purchase_return_id` or `source_stock_transfer_out_id`. The columns have
been in both stores since 0.6.0 and the edge writes them on every receipt — so
the edge's own copy was right while **the cloud stored NULL for every replayed
procurement movement**. 0.5.9's rule is quoted inside ADR-019 ("the
additive-change consumer list reaches THE WIRE TYPES, not just the schemas") and
the wire type was still missed, because the list was walked for the schemas and
the repository and this serialiser is neither. **A rule written down is not a
rule enforced.**

**THE HOLE THAT REMAINS, and it is 0.5.9's other lesson:** that test's fixture is
a WASTAGE entry where all three columns are legitimately null, so it now passes
on three nulls agreeing with three nulls. It proves the fields are on the wire,
**not that a populated value survives**. A criterion-6 fixture with a real GRN
row is still needed.

**(d) The two ADR-013 hardware gates in §4 remain open.** They block **M3**
acceptance.

**(e) Seeded reorder levels are placeholders** that make 28 of 32 items read LOW.
Set real levels before any rollout or demo.

### Environment facts a fresh session will otherwise rediscover

- **The repo is PUBLIC** (`private: false`). Branch protection is therefore
  available and **is applied**: `allow_force_pushes: false`,
  `allow_deletions: false`, `enforce_admins: true`. Actions minutes are
  unmetered for public repos, so **do not gate the `windows-latest` crash job to
  save minutes** — there are none to save, and it guards criterion 2.
- **History-rewriting git is DENIED** in `.claude/settings.json` (`reset`,
  `rebase`, `commit --amend`, `checkout --`, `restore`, `push --force`, `clean`,
  `branch -D`, `stash drop/clear`, `gc`). Verified binding: `git reset --soft
  HEAD` and `curl` were both refused at the permission layer. The list matches on
  command PREFIX, so a trailing `--force` is not caught locally — the server-side
  protection is what covers that spelling.
- **PUSH AFTER EVERY COMMIT.** The reflog is not a backup. On 2026-08-29 a
  `git reset HEAD~1` run by one agent discarded a PARALLEL agent's commit
  (~4,100 lines); it survived only because the working tree still held it.
- **Postgres is not running by default.** Docker Desktop must be started
  manually, then `docker compose up -d postgres`. Backend tests need
  `HOLLER_TEST_DATABASE_URL=postgres://holler:holler_dev@127.0.0.1:5432/holler?sslmode=disable`
  — and **`HOLLER_SKIP_PG_TESTS=1` hides real failures**: it masked four
  `internal/payments` tests that T7a's `billing.manage` check broke.
- **`LNK1104: cannot open file ...exe` is McAfee, not your code.** Re-run; two or
  three retries reach green.
- **The POS's pnpm store can hold a STALE `@holler/contracts`** — a hard copy
  whose `index.ts` exports a file the copy does not contain. Symptom: contract
  symbols "missing" that plainly exist. Fix: `pnpm install --offline` in
  `apps/pos`, and `vite --force` if the dev server still throws "does not provide
  an export".

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

## 5. M2 acceptance item 5 — RED AGAIN as of 2026-08-30

> **CORRECTION, 2026-08-31 — now DIAGNOSED and FIXED (`47eec2f`).** Everything
> below was true when written. The `lan-integration` job was red in **35 of the
> 37** runs in the blind window, and its "real socket session" step proved no
> socket session at all.
>
> **The cause: CI pinned Node 20, and the suite needs Node 22.** It deliberately
> takes no `ws` dependency — a library socket would no longer prove the one
> thing T10 exists to prove — so it uses Node's own global `WebSocket`, which is
> only unflagged from 22. All four tests died on `ReferenceError: WebSocket is
> not defined`.
>
> **The requirement was written down and nothing enforced it.** `kds-lan.test.ts`
> says "available without any dependency since Node 22" in a comment three lines
> above the call. So the suite passed 4/4 by hand on a developer box running Node
> 24 — which is where the "re-verified 4/4" claim below came from — while CI on
> Node 20 failed every run. **"It passes locally" and "the build is green" drifted
> apart, and neither reader could see the other.**
>
> That is the same criterion, failing in the same job, for the second time. The
> first time it stood recorded as met while its bridge silently failed to
> compile. Both times the job was red for a reason nobody was reading, and both
> times the criterion was recorded from a green run somewhere else.
>
> Fixed by pinning this job to Node 22 and adding `requireGlobalWebSocket()` at
> both raw socket sites, which fails with the runtime version and an explicit
> instruction NOT to add `ws` to get past it. **An environment requirement that
> is not checked is an environment requirement that is not met.**
>
> Item 5 is CI-evidenced green again as of `66749b0`. Note the same Node 20
> defect was independently failing `e2e-scenario` — see §2(b).


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

**Contracts are FROZEN at v0.6.4**, cross-checked against
`packages/contracts/package.json` by `scripts/check-milestone-marker.mjs` — which
caught this very line claiming 0.6.2 after the bump, and failed CI for it.

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
