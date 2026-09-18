# RESTART HERE — post-demo planning session, 2026-09-17, HEAD `2b62795`

**THE DEMO RAN AND IT WENT WELL.** Shinjuku Yakitori, 2026-09-17, reported by
the operator. **The demo-period freeze ("no new features") is LIFTED.**

This corrects the previous version of this heading, which read *"THE DEMO DID
NOT RUN"* — written when the machine was shut down mid-rehearsal, before the
demo itself. The repository is the authority on the code; the operator is the
authority on what happened in the room.

**The planning that came out of the demo is `docs/m7-kickoff.md`, and it is the
first thing to read.** It maps the operator's feedback F1–F6 to milestones with
tracks, contract impact and acceptance criteria, and it is **a proposal awaiting
approval**, not a committed scope.

## CI STATUS OF HEAD — READ BEFORE CLAIMING ANYTHING IS GREEN

**`2b62795`, run `35219643824`: 15 of 16 jobs green. One red —
`e2e-scenario`.**

| Job | State | Why |
|---|---|---|
| `e2e-scenario` | **RED, DIAGNOSED, FIX AGREED** | the harness mints a second compliance version. Diagnosed in full below; scheduled as M7-B0 |
| `backend` | **GREEN — the previous session's UNRESOLVED question is now SETTLED** | see below |
| every other job | GREEN | |

**`backend` was recorded here as RED, NEW, CAUSE UNKNOWN on `045b68e`**, with
this exact failure:

```
--- FAIL: TestSyncConfig_DeviceCredentialsFlowThroughRealPostgres (1.06s)
    device_credentials_sync_test.go:203: GET /sync/config after enroll: expected 200, got 401
```

The query recorded to settle it was to re-run that job unchanged. **It is green
on `2b62795`, which touched only `docs/**` — so nothing in the backend moved
between the two runs. The test is FLAKY, not broken.** Recorded as settled
rather than quietly dropped, because the whole point of writing the
discriminator down was to be able to close it.

**It is still worth a row.** A flaky auth test is how a real 401 regression gets
re-run away, and this suite already carries two filed flakes (`edge/printer`'s
logo tests, `stale_connection.rs:160`'s wall-clock bound). It is not scheduled
here; it is named so the next person who sees it red does not start from zero.

## WHAT LANDED THIS SESSION, IN ORDER

| Commit | What |
|---|---|
| `56a6950` | `edge-style` clippy red: `result_large_err` on `ureq::Error` in the sync client's transport retry. Boxed; every match arm unchanged |
| `ca9ac49` | **FIX 5.** The cloud seeds no `stock_ledger_entry` rows; the outlet replays them |
| `aa79396` | The seeded-ledger assertion counts one outlet, not the whole table |
| `79b6d82` | Demo-day dates, the on-stage known list, the VV rows for the night |
| `9c110d7` | `dev-bootstrap` names the database it seeds; the scratch-db guard sees PowerShell callers |
| `a3e92e4` | **`docs/demo-runbook.md`** — one file, cold laptop to finished demo |
| `5c42bab` | The firewall `$subnet` is the hotspot adapter's network, not the machine IP |
| `3d3b659` | Every credential in one table; the admin console's start and origin |
| `46c176a` | The admin "Failed to fetch" is the browser, and the day-of answer is another browser |
| `491a071` | **`docs/operator-guide.md`** — four scenarios the client can self-operate from |
| `045b68e` | **POS menu screen: whole-menu search, internal sections hidden, rail 160px to 220px** |

### Fix 5 was ruled OPTION 2, not the option the plan recorded

**Read this before touching the seeders.** The plan in the older resume block
below says both stores write the same 1–45. The operator re-ruled it on the
finding below, and the ruling that shipped is: **the cloud seeds no ledger rows
at all.**

The collision was never the sequence — both stores already minted 1–45 in the
same order. It was the **row ids**. Ledger ingest is idempotent **by id** and
checks it *before* contiguity (`backend/internal/inventory/service.go:330`), so
the edge's replay of its own seeded rows missed on id, INSERTed, and hit
`UNIQUE (outlet_id, entry_seq)`.

**The seeded goods receipt DOCUMENT stays, and must.**
`stock_ledger_entry.source_grn_id` is a real FK to `goods_receipt_note(id)`
(`packages/contracts/postgres/0028_m5_procurement.sql:361`) and the receipt's
movements are the **first** marks replayed, so a cloud without the document
fails the replay on `entry_seq` 1 for a new reason. It cannot arrive by replay
instead: the edge seeder calls `Db::record_goods_receipt`, not the
`_with_outbox` variant, so it writes no outbox row and `pump_procurement` never
sees it. `source_stock_count_id` carries **no** FK (`postgres/0024:37`), which is
why the 38 opening-stock rows need nothing seeded despite `stock_count` being
unroutable (A7).

**Consequence to expect: a freshly seeded cloud holds an EMPTY ledger until the
till replays into it.** That is intended, not a missing step. If 45 rows are
present *before* the till has drained, fix 5 has regressed.

## THE FIRST THING TO DO NEXT SESSION

**Read `docs/m7-kickoff.md` and get the M7 scope approved.** Nothing below it
should start before that: F2 and F3 both need a contract bump and an ADR, and
B2's sink enumeration is a prerequisite for trusting anything F2 puts on a
screen.

**One loose change is in the tree and belongs to M7-B2:** `refetchInterval:
5000` on `useOrdersQuery` (`apps/pos/src/lib/queries.ts:95`), written when a
waiter's order failed to appear in the POS order list at the demo. `tsc` is
clean; it is **uncommitted and unverified in the Tauri release window**, which
per the four-runtimes rule means it is not verified at all.

### Still open from before the demo, unchanged

**Three VV rows are waiting on a person, and an agent cannot close any of
them.** The binary they need is already built and verified.

`docs/visual-verify.md` → **VV-017, VV-018, VV-019**, all `OPEN`, all from
`045b68e`:

- **VV-017** — select **Sushi Platter** in the left rail (a section with no Thai
  dish in it), type `Thai` in **Search menu…**. Results must appear **from other
  sections**, each card showing its **section name under the dish name**. Clear
  the box: back to Sushi Platter's two items with **no section labels**. The
  steps start on Sushi Platter deliberately — that exact sequence returned
  **nothing** before this commit, so passing it falsifies the old behaviour
  rather than merely exercising the new one.
- **VV-018** — scroll the rail top to bottom: **no "(internal -- not sold)"
  category** anywhere.
- **VV-019** — `Tartar & Carpaccio`, `Wok Poultry & Meat`, `Sushi Roll (4pc)`
  read in full, **no ellipsis**, cart width unchanged.

**All three are invisible to every suite** — 264 POS tests, `tsc`, `eslint` and
`pnpm build` pass either way. The Tauri window is the only judge.

**VV-016** (the fix-5 drain) is also open and is the one that proves fix 5 on
real hardware: after a clean `-Fresh` run, 45 ledger rows must arrive in the
cloud **carrying the edge's row ids**, attention list empty; then one wastage on
the till lands at **`entry_seq` 46**.

## THE BINARY IS BUILT AND VERIFIED — DO NOT REBUILD TO START

```
C:\Code\Holler\apps\pos\src-tauri\target\release\holler-pos.exe
built 2026-09-17 17:17:54
```

`scripts\check-release-binary.ps1` passed on **content**: it found
`index-COLAtMd0.css` and `index-D3y1FBCZ.js` — this build's own chunks — inside
the executable. Start with `-NoBuild`, which skips the compile and still runs
that check:

```powershell
$env:HOLLER_DB_KEY_HEX = (Select-String -Path apps\pos\.env.dev -Pattern '^HOLLER_DB_KEY_HEX=(.+)$').Matches[0].Groups[1].Value
$env:HOLLER_DB_KEY_HEX.Length          # must be 64
.\scripts\demo-up.ps1 -Release -NoBuild -DbKeyHex $env:HOLLER_DB_KEY_HEX -LanHost <hotspot-ip>
```

**No `-Fresh`** keeps the seeded state. Add `-Fresh` only to reset cloud + edge.

### THE KEY DIES WITH THE TERMINAL — this cost a run today

`$env:HOLLER_DB_KEY_HEX` does not survive a new PowerShell window, and
`demo-up` then fails at step `[5/10]` with the bootstrap's "no default key"
refusal. **The refusal's own advice — mint a new key — is WRONG on a machine
that already has a sealed database**, which is every machine after the first
bootstrap. Read the existing key back with the `Select-String` line above.
Nothing is damaged when this fires: the script refuses before touching disk.

## STATE OF THE MACHINE AT SHUTDOWN

- Docker `holler-postgres-1`, `holler-redis-1` and `holler-nats-1` were **up and
  healthy**; the edge and cloud both hold the demo seed. **No seed or reset was
  run today** — the database is whatever the last `-Fresh` left, plus the day's
  rehearsal.
- POS, KDS, captain and the backend were all **down**. The **admin dev server
  was still running** on 5175 (`pnpm dev`, started by hand).
- A POS process survived its window being closed (the known no-exit-handler gap)
  and had to be stopped so `tauri build` could overwrite the binary. Expect a
  plaintext `edge.db` beside the `.enc`; that is A6 and happens on every run of
  this build.

## OPEN ITEMS, NOT STARTED

**From the demo itself (2026-09-17), now planned in `docs/m7-kickoff.md` and
filed in `docs/backlog.md`:** F1 admin console serving and sign-in error
clarity; F2 prep-time timer per table (contract 0.9.0); F3 occupancy-triggered
dynamic pricing (recommended M8); F4 non-webview writers not reaching the
screen; F5 the competitive research note correcting the offline-moat claim; F6
the visual refresh. **F3 and F6's image storage are the only contract questions,
and neither is decided.**

**Also unfrozen now the demo is done:** the carried Phase A gaps **A4, A6 and
A7**. A6 (no exit path seals the edge database) and A7 (78 rows with no sync
route) are pilot blockers whose deferral reason has expired.

**Post-demo, agreed with the operator earlier:**

1. **A real "not sold" flag on the category.** `045b68e` hides the two internal
   sections by **matching the word "internal" in the name** — a stopgap,
   commented as one in `PosScreen.tsx`. A category renamed without that word
   silently returns to a customer-facing screen. The rule belongs on the row.
2. **Hide zero-rate (bar) categories behind a setting.** 16 categories and 100+
   drinks are one scroll away on the till, and the standing rule is *never bill
   a bar item*, because alcohol sits on a zero-rate profile and prints ₹0 tax.
   Nothing in code enforces it today.
3. **The e2e harness should reuse devseed's compliance version** instead of
   minting a second — that is the whole `e2e-scenario` failure, below.
4. **`demo-reset.ps1` must pass `PORT` from `-BackendPort`**, and the guard
   should plant `-BackendPort 8099` and assert 8080 stays free. Today
   `-BackendPort` is used only to kill and check the port, never to tell the API
   which one to bind, so `demo-reset` binds 8080 whatever it is asked for.
5. **Retro line:** check `StartTime` against your own history before assigning
   blame for a bound port.

### `e2e-scenario` — diagnosed, deliberately NOT fixed

All 33 failures are one identical error, and it is **not** an arithmetic
mismatch — `issue_invoice` is **rejected**, so the invariant is marked false at
`tests/e2e-scenario/orchestrator/src/runner.ts:756`:

```
no tax rules for profile 0191c000-...-0002 under compliance version 0191a000-...-0040
```

`0191c000-…-0002` is the **harness's own** tax profile
(`tests/e2e-scenario/harness/src/main.rs:86`) with its rules pinned to the
harness's own compliance version `…-0001`; `0191a000-…-0040` is **devseed's**.
The harness mints a *second* compliance version for the outlet, so
`resolve_compliance_version` picks devseed's, under which the harness's profile
has no rules. The harness's own comment — *"devseed itself seeds no tax_profile
… row"* — is stale; `seed_billing` does now.

**Not the zero-rate bar items** (`devseed.rs:2936-2940` gives
`TAX_PROFILE_ALCOHOL_VAT_ID` real rules at **0 bps** — rules present, rate zero)
and **not reachable by a printed bill** (`devseed.rs:2931-2934` states the
invariant: one compliance version, four profiles hanging off it).

## THE SEARCH DEFECT, AND WHY IT MATTERS BEYOND ITSELF

The till's search filtered **the selected category only**. Typing a dish name
returned nothing unless the cashier had already guessed its section — which is
the question search exists to answer.

**It was not a regression.** `git blame` puts those lines on `ccca59f`,
2026-08-07. **The data changed, not the code:** the seed held **43 items in 10
categories** until 2026-09-11; the client's own menu (`ab7ddef`, 2026-09-12)
made it **281 items in 48**. At ~4 items a section the scoping is invisible; at
48 sections it misses almost every time.

Worth carrying: **a limitation that appears only at production scale, found by
the first person to use the feature the way a real user would.** The fixture hid
it for five weeks and every suite stayed green throughout.

---

# Resume state — 2026-09-17 morning, HEAD `89c235a` (SUPERSEDED by the block above)

**THE DEMO IS DEFERRED to the week of 2026-09-21.** Scope, the six steps and
the excludes are unchanged. Until the demo week, work is **defect repair on the
six-step path**.

Read in this order: **`docs/demo-status.md` → "THE OPEN-DEFECT REGISTER"**
(D1–D27), then **`docs/visual-verify.md`** (a gate, not a log), then
`docs/demo-kickoff.md`.

## CI STATUS OF HEAD — READ THIS BEFORE CLAIMING ANYTHING IS GREEN

**`89c235a`: run in flight when the session ended.** On `b3060ad`, the commit
before it, **12 jobs green, 2 red**, with `crash-durability`, `e2e-scenario`,
`edge` and `rust-seams` still running:

| Job | State on `b3060ad` | Why |
|---|---|---|
| `backend` | **GREEN** (was red) | fixed `29e3ead` |
| `contracts` | **GREEN** (was red) | fixed `29e3ead` |
| `edge` | was green on `2bed8f6` | fixed `7d5daea` |
| `edge-style` | **RED** | clippy `empty_line_after_doc_comments` — **fixed in `89c235a`, unverified on CI** |
| `cloud-replay` | **RED** | **fix 5, not started.** `entry_seq` collision, described in full below |
| `crash-durability` | unknown on `b3060ad` | was red on `2bed8f6` for the CRLF identity hash — **fixed in `b3060ad`, unverified on CI** |
| `e2e-scenario` | unknown on `b3060ad` | fixed `a1d0d25`, then hit the same CRLF hash — `b3060ad` should clear it |

**First command tomorrow: `gh run list --limit 1`, then the per-job view.** CI
had zero successes in forty runs until this session; do not infer from local
green.

## WHAT LANDED TONIGHT

- `b3060ad` — **the identity hash ignores line endings.** It was over raw
  bytes, so an LF-authored catalogue could never match on a Windows runner
  (`a29e82d47f67` vs `b4417a7811eb` for the same unmodified file), and five
  `crash_durability` tests failed reporting a restaurant mismatch that did not
  exist. Also pins `seed/*.toml` and `seed/*.json` to `eol=lf` — not for the
  hash, but so the repo states its line endings.
- `89c235a` — `build_shared_catalogue`'s doc comment sits next to its function
  again. D10's block had been inserted between them; that was `edge-style`'s
  failure.
- **Fix 5: NOT STARTED. No part of it exists in the tree.** Nothing was
  stashed, nothing dropped, working tree is HEAD.

## FIX 5 — THE PLAN, VERBATIM, SO IT IS NOT RE-DERIVED

**The defect, confirmed by draining on 2026-09-16:** the demo cloud holds
seeded `entry_seq` 1–45 for the demo outlet, the demo edge's counter starts at
1, and **the first replayed stock movement 409s onto the banner on stage**.
Reproduced locally: `stopped=Some(Rejected { status: 409 })`.

**Root cause per §50.1:** `entry_seq` is edge-owned. The cloud seeder minting
it from its own high-water mark is split authority on the ledger.

**The fix, as ruled:**

1. **Opening-stock AND GRN ledger rows carry `entry_seq` in the shared
   catalogue.** GRN rows are treated exactly like opening stock — **same row
   ids and same seqs in both stores**.
2. **The seeded `goods_receipt` row itself is identical in both stores (same
   id)**, so the 7 ledger lines reference a receipt the edge holds too.
3. **Both stores write the same 1–45.**
4. **The edge's counter starts at seeded+1.**
5. **The cloud seeder NEVER assigns a seq.** `seedStockLedgerEntries`'s
   `MAX(entry_seq)+1` fallback is the split-authority path, so it is **deleted**
   rather than corrected — the same ruling as `devseed`'s `DATABASE_URL`
   default.
6. **Row-for-row test covers the ledger rows too.**
7. **`cloud-replay` asserts the wastage entry is seeded+1.**
8. **The 0.5.8 1-based rule gets its own test on an outlet with NO seeded
   ledger.** Option 2 (starting the test's edge counter above the cloud's mark)
   was **rejected**: it deletes the assertion 0.5.8 exists to protect.
9. **Pilot item filed:** the cloud never assigns `entry_seq` for any outlet;
   ledger import for a real outlet goes through the edge or carries
   edge-assigned seqs, **enforced by a check, not a README**.
10. **Retro line lands with the fix:** *"A plant that doesn't turn the test red
    proved nothing. Watch the plant fail before trusting the restore."*

**Falsification, required before reporting it done:** drain one wastage entry
against a freshly seeded cloud → **200, `entry_seq` = 46, banner attention list
empty**. Then **plant the old cloud-side seq assignment back → 409**.

### Measured facts — do not re-measure, these are from a fresh scratch seed

```
stock_ledger_entry, demo outlet: 45 rows, entry_seq 1–45
  GOODS_RECEIPT     7 rows, seq 1–7
  COUNT_ADJUSTMENT 38 rows, seq 8–45
goods_receipt_note: 0191a000-0000-7000-8000-000000000051, GRN/20260809/0001
```

**THE LIVE `holler` DATABASE IS ALREADY IN THE COLLIDING STATE** — 45 rows, seq
1–45, same outlet. This is not hypothetical for the rebuild Gaurav is running.

**Both seeders:**

| Side | File:line |
|---|---|
| Emitter (catalogue) | `edge/database/src/bin/devseed.rs:2067` `"goods_receipt"`, `:2068` `"opening_stock"` |
| Edge writer | `edge/database/src/bin/devseed.rs:2734` `write_goods_receipt`, `:2816` `write_opening_stock` |
| Cloud writer | `backend/cmd/devseed/seedwrite.go:434` `seedStockLedgerEntries` — **the fallback to delete** |
| Cloud callers | `seedwrite.go:377` (opening stock), `:421` (GRN lines) |

## THE QUEUE AFTER FIX 5

1. **D12 — observation only.** After the operator's reset, no order in admin
   shows a non-zero total with zero lines. That is VV-004.
2. **`docs/demo-script.md` step 5 wording** — say stock is shown **on the
   till**, not in admin. D9/D8b are not this demo (operator ruling).
3. **D24 — `check-seams` without `make`.** `make` is not on PATH in an agent
   shell, so the CLAUDE.md instruction to run it silently does nothing. Needs a
   script or a package.json target.
4. **M6 partial/unobserved sweep** — anything in the boundary report marked
   partial or unobserved gets an executed test or a VV row. C2 stays parked.
5. **Rehearsals** — three from a clean reset, each timed, then the recording.

## FOR GAURAV — OPERATOR ITEMS

**After fix 5 lands, reset and reseed the live cloud.** The exact sequence is
written at the end of this session's report and belongs here once it has been
run once; until fix 5 lands, a reseed reproduces the colliding state.

```powershell
# 1. bring the stack up (Docker Desktop does not autostart)
docker compose up -d postgres redis nats

# 2. full reset + reseed, cloud AND edge, from the demo seed
$env:HOLLER_DB_KEY_HEX = '<the key from apps\pos\.env.dev>'
.\scripts\demo-reset.ps1 -Force

# 3. start everything, REBUILDING the POS (step 6 does this now)
.\scripts\demo-up.ps1 -Release -Fresh -DbKeyHex $env:HOLLER_DB_KEY_HEX -LanHost <hotspot-ip>
```

**`-NoBuild` exists for the demo morning** — it skips the compile but still
runs `check-release-binary.ps1` and refuses a stale binary.

**VV rows waiting on a person.** An agent cannot close any of them:

| Row | What to look at |
|---|---|
| VV-012/013/014 | Kitchen panel: live status without a remount, error-after-refetch, verb buttons + coloured badge |
| VV-011 | Sync banner: muted "kept locally" line, attention list EMPTY |
| VV-015 | Admin Orders names the device — "Dev Till 1" |
| VV-001–004 | Banner empties after cloud restart; order not DRAFT in admin; `#A2` not `##A2`; no order with a total and no lines |
| VV-009 | **Closed FAIL** on `docs/evidence/VV-009.png` — the remount variant was never observed and is superseded by `97bc3dc` |

**Every one of these needs a build from `89c235a` or later.** `StartTime` is
not `BuiltAt`:

```powershell
Get-Process holler-pos | Select-Object Id, StartTime, @{n='BuiltAt';e={(Get-Item $_.Path).LastWriteTime}}
```

## THREE THINGS THAT WILL OTHERWISE COST AN HOUR

- **One cargo test shell at a time.** A stopped tool shell does not kill its
  `cargo` children; the surviving test binary holds the `.exe` and every retry
  fails `LNK1104` for ever. Kill orphans first, run one shell in the foreground.
- **Backend tests need a scratch database you create yourself** — the suite
  refuses `holler` in every shell:
  `docker exec holler-postgres-1 psql -U holler -d postgres -c "CREATE DATABASE holler_scratch_local;"`
- **A plant that does not turn the test red proved nothing.** Two plants failed
  to land today and reported passes; both are now applied by scripts that
  assert the substitution first.

---

# Resume state — 2026-09-16 (superseded by the block above)

**THE DEMO IS DEFERRED to the week of 2026-09-21.** Scope, the six steps and
the excludes are unchanged; only the dates moved, and `apps/captain`'s Monday
18:00 IST cut-off moved with them. Until the demo week, work is **defect repair
on the six-step path** — not presentability, not rehearsals.

Read in this order: `docs/demo-kickoff.md` (deferral note at the top),
**`docs/demo-status.md` → "THE OPEN-DEFECT REGISTER"**, then
**`docs/visual-verify.md`**, which is a gate and not a log.

## WHAT THIS SESSION DID, IN ONE LINE EACH

- `1c0de90` `df977f8` — **order replay was broken both ways.** The envelope's
  version is read from the live aggregate row at send time, so it sequences
  nothing: a transition arriving ahead was a permanent 409 that wedged the
  order's queue, and one arriving equal (the offline case) returned 200 and
  applied nothing, leaving orders DRAFT in the cloud with no error and no
  banner. The state machine is the whole guard now. **Contracts 0.8.3,
  ADR-028.**
- `df977f8` — `ORDER_ITEM_AMENDABLE_STATUSES` declared once and consumed in
  five places, with `scripts/check-order-amendable-drift.mjs` as the joint. The
  cloud's DRAFT-only rule was about to wedge the second Send on every table.
- `b328f21` `bece32f` — **an unroutable outbox row is no longer skipped in
  silence** (D8a). Recorded as `no_route`, blocked immediately, and shown as a
  **muted "kept locally" count**, apart from the attention list, which stays
  empty. A7 itself is untouched.
- `3fe5ed5` `94cb4ea` — **the scratch-database rule.** `go test` refuses any
  database not named `holler_scratch_*` in **every** shell; `demo-reset.ps1`'s
  scratch refusal is `CLAUDECODE`-only **by design**; what is unconditional
  there is that `-PostgresDb` is derived from `-DatabaseUrl`.
- `256d6f7` `d34cf6b` — **`demo-up.ps1` builds every run**, and `-NoBuild`
  skips the compile while still running `check-release-binary.ps1`.
- `e8a56fc` — the walkthrough now says which dishes move stock.
- `a0bb61e` `09178cc` — record corrections and three retro entries.

## THE FIRST THING TO DO

**Nothing in the code. Read `docs/visual-verify.md` and see which rows the
operator has cleared.** Eleven rows are open; three of them (VV-001, VV-002,
VV-003) are this session's order-replay and `##A2` work and have never been
looked at in the Tauri release window. **An agent cannot close a row** —
`run-dev.ps1` refuses under `CLAUDECODE` and a Tauri window launched with
redirected stdio never appears. Add a row and say UNVERIFIED; never a claim.

## THE NEXT PIECE OF WORK, AND ITS PRECONDITION

**D14 — the till shows a stale kitchen status until its Kitchen panel is
remounted.** The operator was asked for the VV-009 observation and **it had not
arrived when this session ended. Do not start D14 blind.** The answer decides
the fix and there are three possibilities:

1. reads READY immediately → no defect, close the row;
2. stale until the panel is collapsed and re-expanded → known staleness, the
   fix is a refetch policy;
3. **still stale after a real re-open → a cache-invalidation defect**, a
   different fix entirely.

`useKotsForOrderQuery` has no `refetchInterval` (`apps/pos/src/lib/queries.ts`)
and nothing pushes a KOT status from the LAN hub into the till's UI. Note
`App.tsx:13` sets `refetchOnWindowFocus: false`, so **a reading taken by
alt-tabbing proves nothing** and must not be accepted as the observation.

## THE QUEUE THE OPERATOR SET, IN ORDER

1. ~~D13 + D19~~ done · 2. ~~D8a~~ done · **3. D14 (blocked on VV-009)** ·
4. **D10** — the cloud holds 0 `restaurant_table`, 0 `station`, 0 `printer`;
find the gap in the emitter or the cloud seeder, fix it, and extend the
row-for-row CI test to those tables · 5. **D11** — the till's own device has no
cloud `device` row, so till-authored orders replay unattributable; seed it the
way the phone's is · 6. **D12** — observation only, VV-004.

**D9 and D8b are NOT for this demo** (operator ruling): step 5 is shown on the
till, not in admin, and the demo does not claim cloud inventory.
**`docs/demo-script.md` step 5 still needs updating to say so** — that is
outstanding. **If step 5 turns out to be unshowable on the till at all, stop
and tell the operator before building anything.**

**D20** (appended-line author) is a contract change → pilot backlog, no work
now. **D24** — the `check-seams` target needs to run without `make`, which is
not on PATH in an agent shell; a script or a package.json target, so it stops
being skipped.

## THREE THINGS THAT WILL OTHERWISE COST AN HOUR

- **`StartTime` is not `BuiltAt`.** A POS started fresh at 11:39 was running a
  00:40 build, and `run-dev.ps1 -Release`'s newer-than-dist check passed it
  because both were stale. Always report the file's `LastWriteTime`:
  `Get-Process holler-pos | Select-Object StartTime, @{n='BuiltAt';e={(Get-Item $_.Path).LastWriteTime}}`
- **One cargo test shell at a time.** A stopped tool shell does NOT kill its
  `cargo` children; the surviving test binary holds the `.exe` open and every
  retry fails `LNK1104` for ever. Kill orphans first, run one shell in the
  foreground. This cost three re-runs and ~40 minutes.
- **Backend tests need a scratch database you create yourself.** The suite
  refuses `holler` now, in every shell, because pointing it there overwrites
  the dev login hashes and the till then refuses a correct password:
  `docker exec holler-postgres-1 psql -U holler -d postgres -c "CREATE DATABASE holler_scratch_local;"`

## STATE OF THE OPERATOR'S STACK AT HANDOFF

Backend `api` and `holler-pos` were both running, rebuilt during the session —
the binary checked at 19:08 carries the current dist and the release check
passes on it. **It does not contain `bece32f`'s banner split**, so VV-011 needs
a fresh build before it can be read. Postgres/redis/nats are up in Docker. No
scratch databases are left behind; both were dropped.

---

# Resume state — 2026-09-15 (superseded by the block above)

**Demo is WEDNESDAY, at the client's restaurant.** The plan is
**`docs/demo-wednesday.md`** and it is the first thing to read — cast of
devices, what to run, seven acts with dialogue, the hard questions, 60-second
triage, Tuesday checklist. This block is a pointer; the repo is the authority.

Everything below is committed and pushed. Nothing of this session's is
outstanding.

## THE ONE COMMAND THAT CHANGED

```powershell
.\scripts\demo-build.ps1                                  # build everything
.\scripts\demo-up.ps1 -DbKeyHex <key> -Fresh -Release     # start everything
```

**`-LanHost` is gone.** It is resolved automatically and printed with its
adapter on every run. Pass it only to override. This is the whole point of the
last hour: the address was a moving target and chasing it by hand cost three
separate failures in one evening.

## THE DEFECT THAT WOULD HAVE KILLED THE DEMO

**`cargo build --release` DOES NOT PRODUCE A PRODUCTION TAURI APP.** It
produces a dev-mode app in the release profile whose window loads
`http://localhost:5173` and shows *"Hmmm… can't reach this page"*. Only the
Tauri CLI (`pnpm exec tauri build`) embeds the frontend.

**`-Release` had therefore NEVER rendered a window, on any build, through
three sessions.** Proven, not argued: the old binary contained
`http://localhost:5173` and did not contain the built asset filename.

**How it survived is the part to keep.** Every check was a check on the FILE —
it exists at the path the firewall rule names, it is newer than `dist`, its
SHA-256 (recorded twice), `Get-Process` shows `target\release`, the launcher
prints `build : RELEASE`. **Every one of those passes on a binary that cannot
draw a window.** Existence and identity checks standing in for a function
check, unnoticed because each individually looks like diligence.

`CLAUDE.md` now names **four runtimes, not three** — build output, dev server,
browser, **and the Tauri release window**. That omission is why the existing
rule was followed and still missed it: the 2026-09-14 screenshots were honest
browser-runtime evidence that explicitly disclaimed proving anything about the
Tauri window. Everyone's evidence was true and nobody's covered the gap.

Guard: **`scripts/check-release-binary.ps1`**, run by `run-dev.ps1 -Release`
and by `demo-build.ps1`. It requires the binary to CONTAIN the current `dist`
entry chunk's hashed filename. Note why it is that and not the obvious test —
a correct production binary still embeds the whole `tauri.conf.json` with
`devUrl` in it, so a check keyed on `localhost:5173` being present passes the
BROKEN binary and proves nothing. **Positive evidence, never absence.** It was
watched refusing the old binary and passing the new one, twenty minutes apart.

## PROVEN ON REAL HARDWARE THIS SESSION

- Release binary renders the till. UI pass visible in the Tauri window.
- **Captain page loads on a real phone over the LAN** (`:9320`), pairs, holds
  its token.
- **KDS reachable from another device** — both vite configs bound loopback
  only until this session, so no phone or second laptop could ever open the
  kitchen screen or the back office. `host: true` on both, `preview` too.
- Firewall rule created: TCP **9310, 9320, 5174, 5175**, **Private+Public**.
  Note it is PORT-scoped, not program-scoped: the KDS and admin pages are
  served by Node, so a rule naming `holler-pos.exe` leaves both screens dark.
- **The 60-second pairing window is REAL and was OBSERVED** — a valid token
  rejected with the same wording a wrong token gets, then accepted a minute
  later with nothing changed in between. Enrol every phone while the stack
  boots.
- **KDS latency: `wire 5 ms / render 4 ms`, n=1.** Against a 1s blocker
  threshold, not marginal. **But wire minus render is ~1 ms of transit, and
  1 ms is loopback** — that was a KDS on the till, no WiFi, no second device,
  no skew. The Act 2 number is still unmeasured.

## NEXT, IN ORDER — TUESDAY

1. **KDS latency from a SECOND DEVICE over the hotspot.** The number Act 2
   actually depends on. Several samples; read `worst`, not `last`.
2. **Till tap → bill open.** Instrumented and unread. `Ctrl+Alt+P` on the till.
3. **The KDS should take the till's address from `window.location`**, not from
   a baked env var. The address is baked at dev-server START, so a stale
   `.env.dev` or a server started before a network change serves a dead
   address for ever — and `lanClient` retries it silently. That is what bit us
   three times tonight (`.106`, then `10.214.149.115`). If the page loaded from
   `http://192.168.0.100:5174/`, the till IS at `192.168.0.100` — it cannot go
   stale. Deferred deliberately: a behaviour change on the demo path deserves
   tests and a rehearsal, not a 02:00 commit.
4. **Rehearse on the HOTSPOT, never home WiFi.** Different network class,
   different firewall profile, different addresses.
5. `docs/demo-wednesday.md` Part 6 checklist, top to bottom. The two items most
   likely to embarrass: **three phones sending at once**, and **killing mobile
   data without killing the hotspot** (practise that exact click).
6. Three timed rehearsals from a clean reset, then the recording.

## TWO SEED PROBLEMS ON THE TILL, NOT YET FIXED

Seen on the running till, both in front of the client on Wednesday:

- **A category called `Test fixtures…`** in the rail, beside `Kitchen Pre…`.
  A dev label on screen, which work item 6 forbids. Seed fix, not code.
- **The rail opens on alcohol** — Cognac, Shooters, Sake, wines. Those are the
  ones that must NOT be billed (VAT is inexpressible, 141 bar items sit at
  CGST 0 / SGST 0). Open on a food category.

Also cosmetic: the price and the `+ Modifier` link abut on a card
(`₹525.00+ Modifier`) — the link is absolutely positioned and the price grows
into it.

## SECURITY

The `-DbKeyHex` value was pasted into a chat transcript this session. It
encrypts the edge database, which holds staff credential hashes. **Rotate it**
(`dev-bootstrap.ps1 -RotateKey`) before anything real runs on that machine.

## STILL TRUE, STILL BINDING

- **No test or probe starts, stops or binds anything on 8080, 9310, 9320,
  5173, 5174 or 5175.** Nothing this session did; the screenshot and KDS
  probes used scratch port 5399 and read-only page loads.
- **`apps\pos\.env.dev` is deny-ruled to agents.** The operator runs anything
  needing it. `add-waiter.ps1` reads it in the operator's own shell.
- Contracts FROZEN at v0.8.2. Nothing this session touched them.
- **A7 still stands:** only `order` and `table_session` have sync routes. KOTs,
  invoices and stock counts do not replay. Do not read that as a new defect.

---

# White-label (parallel session, 2026-09-14)

**Handoff from a parallel session. Its work is COMMITTED AND PUSHED; nothing
of its own is outstanding.** Demo prep continues in another session — this
block is a pointer, not a second source of truth. The repo is the authority.

## 1. Commits (all pushed, all on `main`)

| hash | subject |
|---|---|
| `807552c` | feat(seed): onboarding a restaurant is writing one file, never editing code |
| `042dd06` | docs: seed/outlet.toml is a prerequisite, so every runbook that starts the stack says so |
| `d2cae64` | docs(backlog): two script arguments that name a target the step that matters never reads |
| `c621f6a` | fix(scripts): check-seed-drift could not parse, and seed twelve tables instead of two |
| `e77ff41` | docs: the captain-cache acceptance is about what it must still REFUSE, and the pump's lock spans cloud I/O |

## 2. WHAT THE WHITE-LABEL CHANGE ACTUALLY TOUCHES

`seed/outlet.toml` is the single source for the restaurant's name, legal
entity, address, `state_code`, pincode, GSTIN, FSSAI, invoice prefix, bill
footer, timezone, business-day start, UPI payee and logo. Both seeders read
it; the `RESTAURANT_NAME…INVOICE_FOOTER_TEXT` constants block in `devseed.rs`
is gone. Gitignored and per-installation; `seed/outlet.example.toml` is the
committed template and is what CI and the test suite read.

**Answering the question directly — did any order, bill, KOT, sync or captain
path change?**

- **Order, KOT, sync, captain: NO CODE TOUCHED.** Not one file under
  `edge/sync/`, `apps/captain/`, `apps/kds/`, `apps/pos/src/`,
  `apps/pos/src-tauri/src/` (`captain.rs`, `commands/`, `state.rs`) was
  modified by any of the five commits above. Verify with
  `git show --stat 807552c c621f6a`.
- **Bill: ONE RENDER-PATH FILE CHANGED — say this out loud rather than
  claiming "branding only".** `edge/printer/src/template.rs` gained
  `render_outlet_mark()` and a two-line change inside `render_logo_block`,
  so the restaurant's own mark renders beside the Holler mark when
  `logo_path` is set. **No `logo_path` is configured, and with none set the
  output is byte-identical to the pre-change build** — proven, not asserted:
  a receipt was rendered by the pre-change `template.rs` and by this one with
  identical inputs, and both the HTML and the ESC/POS bytes compared equal
  (`cmp`, 2026-09-13). The first compare differed by one always-emitted CSS
  rule, which is why that rule is now emitted only when there is a mark to
  style.
- **Everything else is seed, config, script or docs**: `devseed.rs` +
  `devseed/outlet_identity.rs` (new, seed-only), `backend/cmd/devseed/` (the
  cloud seed reader and its fixture), the three PowerShell runners,
  `seed/`, `scripts/check-seed-drift.mjs`, and documentation.
- **No contract change.** Contracts stay FROZEN at v0.8.2. Every field already
  had a column; `logo_path` is never stored — it is a render input passed as
  `HOLLER_OUTLET_LOGO_PATH`.
- **`schema_version` in `seed/demo-outlet.json` went 1 → 2**, adding
  `outlet_identity` and `outlet_source_sha256`. The cloud DECODES both and
  writes neither (the `station_code` precedent) — declared in `seedfile.go`
  rather than omitted because that decoder runs with
  `DisallowUnknownFields`. **The edge seeder refuses a catalogue whose digest
  disagrees with the identity file it was handed**, which is what stops one
  restaurant's name landing on `outlet`/`tenant`/`brand` and another's on the
  GST invoice.
- `c621f6a` also seeds **twelve tables instead of two**. T1/T2 keep their
  fixed legacy ids because `tests/e2e-scenario/harness/src/main.rs:73-74`
  pins both by value.

**Observed on the real stack, not in a harness** (operator run, 2026-09-13/14):
bills `SY/000001` and `SY/000002` carried every field from the file, including
`Place of Supply: Maharashtra (27)` — the cross-field GSTIN/`state_code` rule
on a legal document — and the ESC/POS stream held **0 non-ASCII bytes in 775**.
Live cloud parity against the run catalogue was **zero diff** across 16 shared
tables. A captain order was attributed to the WAITER device, not the till.

## 3. WORKING TREE AT HANDOFF — read before you commit anything

`origin/main` and `HEAD` are level; **every commit above is pushed and nothing
of this session's is uncommitted.**

**Two files are modified/untracked and they are NOT this session's work:**
`CLAUDE.md` (modified) and `docs/milestone-history.md` (untracked) — one
coherent change that trims `CLAUDE.md`'s tech-stack, directory-ownership and
test-command lists and extracts the closed-milestone record into the new file.

**They were deliberately NOT stashed or committed.** A parallel session was
editing them live, and stashing another session's in-progress work is the one
action here that could lose it. This session's own `CLAUDE.md` addition (step 0,
`seed/outlet.toml` must exist) is committed in `042dd06`, is present at the top
of "Rebuilding the stack from cold", and **the uncommitted diff does not touch
it** (`git diff CLAUDE.md | grep -c outlet.toml` → 0). Whoever owns that
restructure should commit it.

## 4. WHAT IS LEFT UNDONE ON WHITE-LABEL

1. **`logo_path` has never rendered on a real bill.** The absent case is proven
   byte-identical; the present case is unit-tested only.
2. **The identity-file REFUSAL path has never fired on the real scripts.** A
   missing/invalid `seed/outlet.toml` is tested in Rust, but no run has had the
   file absent, so `dev-bootstrap`/`demo-reset`'s refusal is read-verified only.
3. **Tier 2 is not built** — admin "Outlet settings" writing `outlet` +
   `outlet_fiscal_profile` in the cloud, delivered by the config pull, after
   which this file becomes the bootstrap default and stops being the source of
   truth. Filed in `docs/pilot-readiness.md` §B2 as a pilot blocker.
4. **The cloud still seeds no `outlet_fiscal_profile`.** It decodes
   `outlet_identity` and writes none of it, deliberately — so only the edge
   knows the GSTIN, and the admin console cannot show it.
5. **No white-label run has happened on the RELEASE binary yet.** Both operator
   runs were debug. The release rehearsal and its `HOLLER-PERF` number are
   outstanding and belong to the demo-prep session.

# Demo prep (this session, 2026-09-14) — restart here

**Everything below is committed and pushed. Nothing of this session's is
outstanding in the working tree.** The repo is the authority; where this block
and the repository disagree, the repository wins.

## HEAD at handoff

`e666af3` — `docs(resume): white-label handoff`. This session's own work is
`c246a9d`, `f3dcc71`, `52d8930`, `1ef2124`, `3b80a48`, `78c87f5`, `73cfb6a`,
`d1d3ebc`.

## The release binary — and when it stops being current

```
C:\Code\Holler\apps\pos\src-tauri\target\release\holler-pos.exe
```

Built **2026-09-15 01:01 IST** with `scripts\demo-build.ps1`, SHA-256
`7bb1e809d735b7e42e2a1e85d424e2eaf45e774e666852c66d57ca93dd581fca`.

**THE 19:01 BINARY AND EVERY RELEASE BINARY BEFORE IT COULD NOT DRAW A
WINDOW.** They were built with `cargo build --release`, which produces a
DEV-MODE app in the release profile: the window fetches its UI from
`build.devUrl` and shows "can't reach this page" with no Vite running. Only
the Tauri CLI embeds the frontend. `scripts\check-release-binary.ps1` now
refuses such a binary and `run-dev.ps1 -Release` runs it before launching.

**The digest identifies the FILE ON DISK, and NEVER the source it was built
from.** An MSVC link is not reproducible: the 01:43 and 09:29 builds came from
one commit, `78c87f5`, and hashed differently. So a CHANGED digest is not
evidence that the source moved, and an UNCHANGED one would not be evidence
that it had not. It tells you one thing only — whether the file the firewall
rule names is the file you hashed. The source-identity check is the `git diff`
command below, and it is the one to run.

**REBUILD IT IF HEAD'S PRODUCT CODE MOVES.** It moved at 19:01: `6f65313`
rewrote the POS order screen, and `apps\pos\dist` is embedded at LINK time, so
that needed a `cargo build --release` and got one. As of this build,

```
git diff --name-only 6f65313..HEAD -- apps/pos/src apps/pos/src-tauri/src \
  apps/captain/src edge packages
```

returns **zero files**. Run it against the new HEAD before every rehearsal; a
non-empty answer means the binary is stale and the window will show the
previous UI with nothing on screen saying so.

Two asymmetries that decide what a rebuild costs:

- **`apps\pos\dist` is embedded at LINK time** (`tauri.conf.json`'s
  `frontendDist`), so a POS frontend change needs `cargo build --release` after
  `pnpm build` or the window shows the previous UI silently.
  `run-dev.ps1 -Release` refuses when the binary is older than that dist.
- **`apps\captain\dist` is served from DISK per request** (`captain.rs:66-71`,
  an absolute path baked from `CARGO_MANIFEST_DIR`), so a captain rebuild takes
  effect with no relink — but moving or deleting that folder is a phone that
  loads nothing.

## Firewall rule

DisplayName: **`Holler demo - POS release (KDS 9310, captain 9320)`**. Inbound
TCP **9310,9320** — the KDS LAN WebSocket and the captain HTTP listener, the
only two the POS process starts — `-Profile Private,Public`, `-Program` the
exact release path above. **This is the only rule definition; the command and
the checks are `docs/demo-script.md` §0.0c and nothing here restates them.**

`Public` is there because Windows classifies a Mobile Hotspot adapter as Public;
`Private` because a rehearsal on home WiFi must exercise the same rule. Domain
is excluded: no demo network is domain-joined.

A rule bound to a `target\debug` path covers nothing, and a blocked inbound
connection produces no error anywhere. `docs/lan-setup.md` carries a separate
five-rule per-port set for the DEV stack (5174, 5175, 8080 as well, scoped by
`-RemoteAddress`); that set is not this one and the demo does not use it.

**THE RULE DOES NOT EXIST YET — CHECKED 19:05 IST ON 2026-09-14.** Zero enabled
inbound rules name the release binary. The two enabled `Holler POS` rules point
at `target\debug`, which `-Release` never starts. **This blocks the phone and
the KDS on the demo build and produces no error anywhere**, so it is the first
thing to fix before a rehearsal. The elevated command is in
`docs/demo-script.md` §0.0c and is the operator's to run.

## Starting the stack

```powershell
.\scripts\demo-up.ps1 -DbKeyHex <64-hex-key> -LanHost <hotspot-ip> -Fresh -Release
```

`-Release` is new this session (`d1d3ebc`) and is the demo form: it starts the
binary the firewall rule names, skips Vite and the 5173 guard, and prints the
resolved path and link time before the window opens. Without it, step 8 starts
`tauri dev` — the debug build, which the rule does not cover. `-UpiVpa` is
remembered between runs and now survives `-Fresh` (`c246a9d`).

Verify the launch by identity, never by the port answering:
`Get-Process holler-pos | Select-Object Id, Path, StartTime` — `Path` must end
`target\release\holler-pos.exe`.

## The run list — TUESDAY 09:00, each step's failure means something the next cannot

**Run them in this order.** Steps 0, 4b and 4c are one-shot proofs of things
that have never been observed on the real scripts; every other step is
repeated.

0. **Fire the `seed/outlet.toml` refusal, ONCE, before anything else.** It has
   never fired outside a Rust test (white-label §4.2) — the scripts' refusal
   is read-verified only, and a refusal that has never run is a refusal nobody
   has seen fail open. Rename the file, run `demo-up`, watch it stop and name
   the missing file, rename it back, and re-run. **Do this before the hotspot,
   not after:** it must abort before anything is reset, and if it does NOT
   abort, the run that follows has already reseeded the cloud against a file
   that was not there. Do it once; do not repeat it on the later resets.
1. Hotspot up, note its IP, `demo-up ... -Release -Fresh`.
2. Load the captain URL on the phone **before** pairing (isolates static
   serving from auth).
3. Pair, then reload the page — the token must survive.
4. One item, one table, Send. Watch the phone, the KDS and the hub together,
   and take the `HOLLER-PERF` delta (`captain_order_sent` →
   `kds_ticket_rendered`). **Over 1s is a demo blocker** — record the number,
   never an impression.
4b. **Second Send on the SAME table, without clearing it. Expected: the round
   APPENDS to the open order and the KDS shows a SECOND ticket carrying only
   the new round** — not a second order, and not a ticket repeating round one
   (`52d8930`, `78c87f5`). **A second ORDER appearing is the defect `52d8930`
   fixed, returning**, and it is the one failure on this list that invalidates
   step 1a of the demo story rather than delaying it. Stop and report either
   way; do not carry on to step 5 with it unexplained.
4c. **KDS bumps that ticket to READY. Then, on the TILL, leave the order and
   re-open its Kitchen view.** Expected: it reads READY. Nothing pushes a KOT
   status from the LAN hub into the till's UI and `useKotsForOrderQuery` has no
   `refetchInterval` (`apps/pos/src/lib/queries.ts:99`), so the till's only
   route to a fresh value is a REMOUNT — collapsing and re-expanding the
   Kitchen panel, or navigating away and back. `staleTime` is unset and
   `refetchOnMount` defaults true, which is why a remount should be enough.
   **Do NOT settle for alt-tabbing to the till and looking:** `App.tsx:13`
   sets `refetchOnWindowFocus: false`, so a focus change refetches nothing and
   a stale reading taken that way proves nothing about the cache.
   **If it is STILL stale after a real re-open, that is a cache-invalidation
   finding, not the known staleness** — record it on `docs/backlog.md` row
   145 as line 1a. **RECORD WHAT WAS OBSERVED, NOT WHAT WAS EXPECTED**, either
   way: this step exists to find out which of the two it is, and writing down
   the expectation is how the answer gets lost.
4d. **Look at the till's own screens in the TAURI RELEASE WINDOW and read
   two things back.** Both are code changes made on 2026-09-16 and neither has
   been seen anywhere but the build output and the dev server, which CLAUDE.md
   counts as two of four runtimes: (a) the **order list** and the **sync
   banner** must print an order number as `#A2`, never `##A2` — the `#` is part
   of the minted value and two POS surfaces were adding a second one; (b) with
   the cloud stopped and restarted (demo step 4), the **banner must empty** and
   the order must reach the admin **out of DRAFT**. The second is the
   at-least-once replay fix (ADR-028) observed end to end rather than in a Go
   test. **Report what the screen said, not what the fix intends.**
5. Repeat from a clean reset **three times** — that is the cut-off condition,
   not one success. Steps 1–4d each time; step 0 is not repeated.
6. Two phones, two tables, concurrently.
7. WiFi off and on mid-cart. **Expected: duplicate LINES, not a duplicate
   order** (`3b80a48`). Neither appearing contradicts the code — stop and
   report.
8. Till → bill on a captain order, for the second perf number.
9. **Confirm `logo_path` is still unset** before the bill is shown to anyone:
   `grep logo_path seed/outlet.toml` must return only the commented example
   line. With none set the receipt renders byte-identically to the
   pre-white-label build, which is the only rendering that has been proven
   (white-label §4.1). **Setting it would put a never-rendered path on the
   client's bill**, and the failure would first be visible on the printed
   receipt in front of the client.

## Open items, stated as open

- **The `HOLLER-PERF` number is NOT RECORDED.** The operator reported the phone
  check passed on the release binary but left the figure as a placeholder, and
  nothing was written down rather than a number being invented. Monday's perf
  row in `docs/demo-status.md` is still empty, and **over 1s on the phone is a
  demo blocker**.
- **The two POS edits of 2026-09-16 have NOT been seen in the Tauri release
  window** — the `##A2` double prefix and the post-reconnect banner/admin
  state. Build output and dev server only; run-list step 4d is where they get
  observed. An agent cannot do it: `run-dev.ps1` refuses under `CLAUDECODE`,
  and a Tauri window launched from a tool with redirected stdio never appears.
- **`seed/demo-outlet.json` is modified in the working tree and
  `edge/database`'s `seed_offline_sale` test fails because of it** — devseed
  refuses, naming a catalogue/identity sha mismatch and telling you to re-emit
  with `cargo run --bin devseed -- --emit-json ../../seed/demo-outlet.json`.
  The edit predates the 2026-09-16 session and was left alone; every other
  `edge/database` test passes.
- **The `seed/outlet.toml` refusal path has still never fired** on the real
  scripts — read-verified only (white-label §4.2, unchanged by this session).
- **`logo_path` stays unset**, so the bill renders byte-identically to the
  pre-white-label build. The present case is unit-tested only; setting it
  before the demo would put an unrendered path on the client's bill.

## What this session changed in the captain, in one line each

- `52d8930` — a table's open order is read from the ORDER ROWS; nothing in the
  shipped POS has ever written a `table_session` row, so the phone's append
  branch was unreachable and every Send opened a second order. Uncovered a
  second defect that had never been reachable: `/send` confirmed
  unconditionally and confirm rejects a non-DRAFT order.
- `78c87f5` — the OLDEST appendable order wins (pinned, falsified by reversing
  the sort), and the second ticket is asserted to carry only the new round.
- `1ef2124` — the lost-response retry that duplicates LINES is filed in
  `docs/pilot-readiness.md` §0, deliberately not fixed before the demo; the
  client re-reads `/api/tables` after a failed send.
- `c246a9d` — `-Fresh` no longer forgets the UPI payee, and `.env.local` is
  cleared when none resolves, so the bill screen and the printed receipt can no
  longer disagree about the QR.

`apps/pos/src-tauri/tests/captain_http.rs` holds 10 tests, all executed through
`node scripts/assert-tests-ran.mjs`.

---

# Resume state — 2026-09-13

> ## RESTART HERE — SESSION ENDED 2026-09-13, HEAD `8aa46dd`, WORKING TREE CLEAN AND PUSHED
>
> The block below this one is the 2026-09-12 record and is still accurate
> except where this block corrects it. **The repo is the authority; this is a
> pointer, not a second source of truth.**
>
> ### HEAD and what landed since the last restart block
>
> `8aa46dd feat(demo): the client's name reaches the menu and the bill header`,
> pushed; `origin/main` and `HEAD` are level and no tracked file is modified.
> The untracked entries in `git status` (`menu_imgs_gong/`, `pitch-deck/`,
> `.dev-prints/`, `.vscode/`, `~/`, `=ro`, a `~$` Excel lock file and similar)
> are the operator's and predate this session — none of them is pending work.
>
> Landed since `eda25dd`, newest last:
>
> - **The agent guard** (`scripts/agent-guard.ps1`), described below.
> - **`run-dev.ps1` and the bootstrap now print ONE launch sequence**, described
>   below.
> - **UPI plumbing**: `-UpiVpa` / `-UpiPayeeName` on `dev-bootstrap.ps1`,
>   remembered in the bootstrap state file, written into **both**
>   `apps/pos/.env.dev` and `apps/pos/.env.local`, and the invoice screen now
>   renders a **"UPI QR not configured"** panel instead of returning `null`.
> - **`8aa46dd`**: the cocktail "The Gong" is renamed **"House Signature"**
>   through a new `ITEM_RENAMES` table in `scripts/menu-to-seed.py`, applied to
>   the workbook rows before grouping, before the manifest projection and before
>   the Rust emit. The bill header's address, GSTIN, FSSAI and footer are now
>   constants beside `RESTAURANT_NAME` in `devseed.rs`: the invented street
>   "123 MG Road" is gone (the header is now Camp / Pune 411001), and the footer
>   no longer reads "dev fixture, not a real bill". **Monday's reset picks all
>   of this up.** 9 devseed tests executed and passed through
>   `scripts/assert-tests-ran.mjs`, including the manifest guard and the
>   cloud/edge row-for-row parity test.
>
> ### Stack state as this session ended — VERIFIED BY PID, NOT BY THE PORT
>
> | Port | Process | pid | Started |
> |---|---|---|---|
> | 8080 | `api` (backend, own window) | **23052** | 2026-09-12 09:08:57 |
> | 9310 + 9320 | `holler-pos` (LAN + captain listeners) | **66284** | 2026-09-12 23:45:56 |
> | 5173 | `node` (POS Vite) | **69892** | 2026-09-12 23:45:48 |
> | 5174 | `node` (KDS) | **77240** | 2026-09-12 23:41:29 |
> | 5175 | admin console | — | down |
>
> **All of it was left running deliberately, at the operator's instruction.**
> This supersedes the "pid 9360" and "pid 58148" lines further down this file.
> Postgres, Redis and NATS are up; Postgres carries migration 0035.
>
> ### The `.env.dev` repair — what happened and what the file now holds
>
> **An agent overwrote `apps/pos/.env.dev` and zeroed `HOLLER_DB_KEY_HEX`.** The
> test that did it was pointed at a scratch path, but the region of
> `dev-bootstrap.ps1` it executed recomputes `$envFile` from `$repoRoot`
> internally and ignored the scratch path it was handed. **Passing a scratch
> path to code that derives its own paths is not isolation.** The operator
> re-ran the bootstrap with `-RotateKey` and the file was rebuilt; a later
> bootstrap run then hit `file is not a database` on the plaintext leftover,
> which is now handled (see the quarantine note below).
>
> `.env.dev` and `.env.local` now both carry the UPI pair, and the **serving
> Vite has the payee baked into its bundle** — confirmed by fetching the
> transformed `src/domain/upi.ts` from the running dev server, the same probe
> that had shown it ABSENT before, so it is a real change and not a hopeful one.
> The file stays deny-ruled to agents: **the operator runs anything that needs
> it.**
>
> Related and already landed: `edge/database/src/crypto.rs` now **quarantines an
> unreadable plaintext leftover** as `edge.db.unreadable-<unix-ts>` when a sealed
> file exists, and refuses when none does. Three such files sit inertly in the
> edge data directory from when it fired on the operator's machine. **The
> plaintext leftover is NOT a recovery route and must never be offered as one**
> (operator's ruling): it is the data-loss bug itself. If the key is lost, the
> answer is a reset.
>
> ### The agent guard — `scripts/agent-guard.ps1`
>
> **INSTRUCTIONS ARE NOT A CONTROL.** Three times in one day an agent wrote to
> the operator's live stack despite an explicit, repeated, written instruction.
> The guard replaces the instruction with a refusal. Claude Code sets
> `CLAUDECODE=1` in every shell it spawns; under one:
>
> - `dev-bootstrap.ps1` and `demo-reset.ps1` **refuse unless BOTH `-RepoRoot`
>   and `-EdgeDataDir` are given explicitly AND both resolve outside**
>   `C:\Code\Holler` and `%APPDATA%\com.holler.pos`. A default is forbidden —
>   the whole failure mode is a default quietly resolving to the real path — and
>   `..\Holler\x` is caught by canonicalising before comparing.
> - `dev-up.ps1` and `apps\pos\run-dev.ps1` **refuse outright**: they exist to
>   start the operator's stack on live ports and have no scratch mode.
>
> It is **dot-sourced**, so a missing guard file stops the script rather than
> silently disabling the control. A human shell has no `CLAUDECODE` and is
> unaffected. Both halves were watched: the refusal fired from an agent shell,
> and a scratch path passed.
>
> ### There is ONE way to start the POS
>
> ```
> .\apps\pos\run-dev.ps1
> ```
>
> **Never `pnpm dev` in a separate terminal first.** `tauri.conf.json`'s
> `beforeDevCommand` is `pnpm dev`, so `tauri dev` always starts Vite itself,
> and `vite.config.ts` sets `strictPort: true`, so a second one fails on 5173.
> The old note claiming `tauri dev` "will not start its own" Vite was false in
> both directions and cost an evening — and a stale Vite serving the window
> serves a bundle built in a different environment, which is exactly why the UPI
> QR was missing from the bill screen while `.env.dev` carried the payee.
> `run-dev.ps1` now **refuses when 5173 is held and names the pid**;
> `-SkipViteCheck` forces past it. The bootstrap prints this one sequence, and it
> is in `docs/demo-script.md`'s day-of checklist.
>
> ### NEXT STEPS — in order, recorded verbatim as given to the operator
>
> 1. **Confirm the QR on screen (30 seconds).** Ring up anything on the till,
>    open the bill, and look for **"Scan to pay via UPI"** with a QR, the amount,
>    and `ShinjukuYakitori` beneath it. If instead you see **"UPI QR not
>    configured"**, that means the window is on a different bundle than the
>    server probed at the end of this session.
> 2. **KDS.** It is on 5174 already. Check the indicator reads **connected**
>    against `ws://192.168.0.106:9310/kds`. Load-but-never-connect means a stale
>    `VITE_KDS_LAN_URL`.
> 3. **Enrol the WAITER device and pair the phone.** Must be enrolled **after
>    2026-09-11 20:28** — the T29 fix has no backfill (S-CAP-20), so re-pairing
>    an older device will not work; it needs re-enrolling.
> 4. **Then the phone run, which produces the perf logs.** Perf is LOG-BASED,
>    not scripted, by the operator's ruling: no process of the agent's touches
>    the live ports. Grep `HOLLER-PERF` from each of these and paste them:
>    - POS terminal — `captain_order_received`, `captain_send_received`,
>      `kot_upserted_emitted`
>    - KDS console — `kds_ticket_rendered`
>    - Till console (DevTools) — `till_bill_tapped`, `till_bill_screen_opened`
>
>    The two intervals to compute are **captain send to KDS render** and **till
>    tap to bill open**. **Anything over 1s is a demo blocker.**
>
> ### Two decisions still open before Monday
>
> - **The menu is still the Gong card** — 277 items under the Shinjuku Yakitori
>   name. Only the one self-naming cocktail was renamed. If a Shinjuku sheet
>   arrives: `python scripts/menu-to-seed.py --workbook <path>`, re-emit
>   `seed/demo-outlet.json`, re-run the 9 devseed tests.
> - **The bill header address and GSTIN are placeholders.** Camp / Pune 411001
>   and `27AAAAA0000A1Z5`, registered to nobody. The real registered address
>   replaces three constants in `devseed.rs` and nothing else.
>
> ### Still not started
>
> - **Item 7** — the six demo steps in `docs/demo-script.md`, drafted Monday
>   evening from the operator's phone runs.
> - **Item 8** — three timed rehearsals from a clean reset, Tuesday.
> - **KDS ticket and spinner polish** — the CSS change still owes a screenshot;
>   no socket-stub harness exists for it.
> - **Item 5 (offline tick)** — conditional on observing the till sluggish with
>   the cloud down.
>
> ---

> ## DEMO BUILD — WHERE THIS SESSION GOT TO (2026-09-12, pushed through `c9195f5` + the orders work)
>
> Written so an accidental restart picks up here instead of reconstructing a
> verdict from git history. **The repo is the authority; this block is a
> pointer, not a second source of truth.** Full narrative, with what was
> falsified and how, is `docs/demo-status.md`.
>
> ### Landed and pushed, in order
>
> 1. **`4f98b06` — every menu item carries a variant.** Twelve of forty-three
>    seeded items had none, which made them un-orderable from `apps/captain`
>    and impossible to give a recipe (a recipe binds to a variant), so selling
>    one deducted no stock. Closes S-CAP-06 and S-SYNC-08 at the source.
> 2. **`f7d9c8f` — the demo menu is the CLIENT'S OWN CARD.** The invented
>    Indian dev menu is gone. `menu_imgs_gong/gong_menu.xlsx` is the authoring
>    source, `scripts/menu-to-seed.py` generates
>    `edge/database/src/bin/devseed/client_menu.rs`, and devseed emits
>    `seed/demo-outlet.json` from it. **277 items, 46 categories, 343 variants,
>    21 modifier options, 38 inventory items, 16 root recipes + 2 sub-recipe
>    batches, 7 supplier items.** Outlet is `Shinjuku Yakitori`, seeded from ONE constant (`RESTAURANT_NAME` in devseed.rs) that every panel reads rather than copies.
> 3. **`c9195f5` — the login budget is configurable and the demo build widens
>    it to 50** (`HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS`, set in
>    `scripts/dev-up.ps1`). Closes S-BE-09. The identical-401 behaviour ADR-012
>    requires is unchanged; only the count moved.
> 4. **The admin Orders screen** (`apps/admin/src/components/OrdersScreen.tsx`)
>    plus two cloud defects it exposed — see the next section.
>
> ### THE RULING THAT CHANGED: `GET /orders` WAS NEVER MISSING
>
> The scenario board's S-ADM-08 says no orders screen exists and no cloud read
> route exists. **The second half was wrong, and the repo says so:**
> `backend/internal/ordering/http.go:32` has routed `GET /orders` since
> Milestone 1 (`listOrders` → `svc.ListOrders` → `ListByOutlet`). It was absent
> from `packages/contracts/openapi/openapi.yaml`, which is what an earlier
> survey read. Conditional B therefore cost hours, not a day, and needed no new
> wire type: the route is now DOCUMENTED in the spec as it behaves.
>
> **Two real defects came out of pointing a browser at it, both invisible from
> Go and from every existing test:**
>
> - **The cloud dropped `display_number` entirely.** It was in neither the
>   INSERT nor the SELECT (`backend/internal/ordering/repository.go`), so every
>   replayed order carried NULL and the back office could not name a single
>   order. Same shape as contracts 0.5.9's `source_stock_count_id`.
> - **Timestamps were served with a `+05:30` offset**, because pgx returns
>   timestamptz in the session timezone. `CanonicalOrderSchema` types them as
>   `z.string().datetime()`, which REJECTS an offset, so every TypeScript client
>   failed to parse an order the cloud served. Nothing had read an order back
>   from the cloud in TypeScript until this screen existed.
>
> Both fixed and pinned by
> `TestPostgresRepository_DisplayNumberAndUtcTimestampsSurviveTheRoundTrip`,
> which asserts the MARSHALLED BYTES (a `time.Time` comparison passes under
> both spellings). Each half was falsified separately by planting the old
> behaviour and watching it fail.
>
> ### What the operator still has to do
>
> 1. **Run `scripts/demo-reset.ps1 -Force` once.** It covers three things at
>    once: the variant renumbering (see the warning below), the edge half of
>    seed parity, and the new client menu. `apps/pos/.env.dev` is deny-ruled to
>    agents, so this is the operator's.
> 2. **Re-enrol the demo phone.** The T29 device-row fix has NO BACKFILL
>    (S-CAP-20): a WAITER paired before 2026-09-11 20:28 stays broken and
>    re-pairing does not help.
> 3. Then the phone runs, then items 7 and 8 (demo script, three timed
>    rehearsals).
>
> ### Live-stack warnings that will otherwise waste an hour
>
> - **A SEED CHANGE THAT RENUMBERS IDS IS ONLY SAFE THROUGH `demo-reset.ps1
>   -Force`.** Running `backend/cmd/devseed` against an ALREADY-seeded database
>   fails loudly on `idx_menu_item_variant_one_default`: the seeder upserts
>   without pruning, so the previous seed's default variant survives beside the
>   new one. The live dev `holler` database is in exactly that state.
> - **THE DEV `holler` DATABASE CANNOT LOG ANYONE IN RIGHT NOW.**
>   `owner@holler.test` and `cashier@holler.test` carry
>   `$argon2id$fixture-hash-not-a-r…` — a TEST FIXTURE hash. Running the Go
>   suite with `HOLLER_TEST_DATABASE_URL` pointed at the shared dev database
>   (which is what `docs/RESUME.md` §6 tells you to do) overwrites those rows.
>   A 401 from the admin console or the till right now is THAT, not a
>   credential you mistyped and not the rate limiter. The reset fixes it.
> - **Backend on 8080 is running in its own window as pid 9360** (started by
>   `scripts/dev-up.ps1 -SkipInfra -SkipSeed -NoKds -NoPos` on 2026-09-12).
>   **SUPERSEDED 2026-09-13: the backend on 8080 is now pid 23052** — see
>   the stack table in the block at the top of this file.
>   Docker Desktop was started this session; `postgres`, `redis` and `nats` are
>   up. Verify any restart by NEW PID, never by the port answering.
> - Contracts are at **v0.8.2** (ADR-027) — a DOCUMENTATION-ONLY bump: `GET
>   /orders` gained its OpenAPI entry, and no `.sql`, `.ts` or `.go` shape
>   moved. The OpenAPI `info.version` field still reads `0.6.2` and has been
>   stale for several bumps — noted, not touched.
>
> ### Still open on the demo brief
>
> - **Item 5 (offline tick)** — not started; conditional on observing the till
>   sluggish with the cloud down.
> - **Item 7 (`docs/demo-script.md`)** and **item 8 (three timed rehearsals)** —
>   not started, and both need the operator's reset first.
> - **S-ADM-09 (stock variance screen)** — ruled OUT by the operator: demo step
>   5 is GRN + stock variance, and with A ruled the orders screen was the
>   conditional. No cloud read route exists for inventory (`/inventory/*` are
>   all POST-only), so a variance screen would need new OpenAPI paths.
> - **Known and carried, DO NOT FIX:** S-SYNC-04 (gap A7), S-SYNC-13,
>   S-SYNC-06, S-CUI-06, gaps A4 and A6.
>
> ---


> ## READ THIS BLOCK FIRST. THE REST OF THIS FILE IS M5 AND M6 HISTORY.
>
> # Next work: DEMO BUILD — see `docs/demo-kickoff.md`
>
> **That file is the brief and it is the first thing to read.** Purpose is a
> client demo: presentability and reliability of what already exists. **No new
> features, no M6.1, no pilot-only work** — the pilot list in
> `docs/pilot-readiness.md` stays untouched, and an item being on it is not a
> reason to do it now. **Report demo blockers only.**
>
> One ruling in that brief reverses a standing M5/M6 prohibition, so read it
> deliberately rather than from memory: **seeding the cloud's catalogue is now
> ALLOWED**, because the defects that prohibition protected (P2/P3/P5) are fixed.
> Seed parity across cloud and edge is the first item of demo work.
>
> ## M6 IS CLOSED — tagged `m6-complete`, 2026-09-11
>
> **Seven of eight criteria met and observed on the shipping binaries: C1, C3,
> C4, C5, C6, C7, C8.** C8 is recorded `SHAPE ONLY — no integration evidence`
> and its integration half travels to M6.1 as an explicitly unmet row.
>
> **C2 is PARKED, and M6 closed WITH it parked, deliberately.** Trigger: *any
> platform sandbox access granted*. It cannot be evidenced from our own logs —
> "never evidence a snooze from our own log" is the criterion's own wording —
> so M6 closes without it rather than with a fake pass.
>
> **Do not re-run any criterion and do not reconstruct a verdict from git
> history.** Every one is written up with its observation, its falsifier and its
> date in `docs/m6-acceptance.md`. The end-of-phase handover — including what
> was learned, what is carried and what must not be tidied away — is
> `docs/m6-phase-c-boundary.md`.
>
> ### Three facts about the live stack that will otherwise waste an hour
>
> - **Contracts are at v0.8.2 (ADR-027 documented `GET /orders`; ADR-026 before
>   it), and sqlite 0035 has NOT been applied to
>   the live edge database.** Postgres has it. The edge takes it at the next
>   clean bootstrap, which the demo seed work does anyway. Nothing writes the two
>   new `order.source` members, so the lag changes no behaviour.
> - **Thirteen rows are permanently blocked in the live edge outbox, and they are
>   C3's FIXTURE, not a fault.** Each is `missing_reference (HTTP 422)` on the
>   variant foreign key. The demo's clean reseed clears them. **Do not clear them
>   by seeding only the cloud's catalogue against the current edge database** —
>   that makes the symptom vanish while leaving the underlying gap (§3.1 of the
>   boundary report) unfixed.
> - **No exit path on this build seals the edge database.** Neither a window
>   close nor `Ctrl+C` fires `RunEvent::Exit`, so a plaintext `edge.db` is left
>   beside the `.enc` every time, and the `.enc` is only as current as the last
>   successful seal. That is A6, it blocks a pilot, and it is why no trustworthy
>   backup can be taken before a risky migration.
>
> ### Where everything is
>
> | What | File |
> |---|---|
> | **The demo brief** | `docs/demo-kickoff.md` |
> | M6 criteria and their evidence | `docs/m6-acceptance.md` |
> | The Phase C handover | `docs/m6-phase-c-boundary.md` |
> | Everything triggered before a pilot | `docs/pilot-readiness.md` |
> | Every deferred item, single register | `docs/backlog.md` |
> | Method lessons | `docs/retro.md`, folded into `CLAUDE.md` |
>
> ### Stack state as this session ended
>
> - Postgres, Redis, NATS: up and healthy. Postgres carries migration 0035.
> - Backend: pid **58148**. Verify by identity, never by the port answering.
> - POS: pid **26140**, built from `df5e030`, window open, pump interval 10s
>   (set for the C3/C4 runs; the shipped default is 60s).
> - Admin dev server and KDS: not started.
>
> ### Rebuilding the stack from cold, if it is down
>
> 1. `docker compose up -d postgres redis nats` — **not** `make dev`, and not a
>    bare `docker compose up`: the compose file's `backend` service fails to build
>    (`go build -o /out/api ./cmd/api` exits 1) and is not used here. Docker
>    Desktop does not autostart on this box; start it and wait for `docker info`
>    to answer.
> 2. Backend in its own window:
>    `.\scripts\dev-up.ps1 -SkipInfra -SkipSeed -NoKds -NoPos`, then record the
>    **NEW** pid.
> 3. **Do NOT re-run the bootstrap.** It reseeds the edge database and would
>    destroy `GRN/20260910/0001` and `/0003` — C5's and C6's evidence — and the
>    C4 order above.
> 4. POS, from a terminal the operator owns; a Tauri window launched from a tool
>    with redirected stdio never appears: `.\apps\pos\run-dev.ps1`, sign in
>    `cashier@holler.test` / `holler123`.
> 5. Admin console: `cd apps\admin` then `pnpm dev`, `http://localhost:5175`,
>    sign in `owner@holler.test` / `holler123`.
>
> `apps\pos\.env.dev` is deny-ruled to the agent because it carries the edge
> encryption key, so **the operator runs anything that needs it**, including the
> bootstrap and the device token.
>
> ### NEXT, in this order
>
> 1. **Contracts 0.8.1 — the `order.source` widening**, on the M6 BOUNDARY LIST in
>    `docs/m6-acceptance.md`: one CHECK widening carrying **two** values, the
>    aggregator platform and `TABLE_TAB` (ADR-025). Additive, but not landed until
>    its consumer list is: both stores, the Go struct, the Zod schema, the OpenAPI
>    shape, and the edge writer that hardcodes `DIRECT` today
>    (`commands::aggregator::ACCEPTED_ORDER_SOURCE`).
> 2. **The Phase C boundary report.** The operator has a further architecture
>    addition to hand over at that boundary.
>
>
> `docs/M6 kickoff.md` carries the operational runbook, but **it is stale on
> position** — it was written on 2026-09-07 and still names Phase B as the next
> work. Phase B closed on 2026-09-08. `docs/m6-acceptance.md` holds criteria and
> evidence and is the authority; `docs/backlog.md` holds every deferred item.
>
> **Phase A closed with FIVE of seven gaps landed — A1, A1b, A2, A3, A5 — and
> THREE carried: A4, A6, A7.** Never report it as "Phase A complete". A7 is the
> one that matters: **78 rows on the live edge database have no route and can
> never be sent** (55 `kot`, 22 `stock_count`, 1 `invoice`, measured
> 2026-09-07). The `order` stream replays end to end; nothing else replays at
> all, and **A7 must close before any aggregate beyond `order` is expected to
> replay in Phase C.**
>
> **M6 C7 was observed on the shipping binaries on 2026-09-07 by the operator**
> — seven rows on the till, each `order <id> · 5 attempts · missing_reference
> (HTTP 422)`, matching `sync_outbox_block` field for field, surviving an
> unclean exit and WAL-replay crash recovery. Backend identity verified by a NEW
> pid (60872, created 15:48:56), not by the port answering. **Do not re-run it,
> and do not reconstruct its verdict from git history** — the evidence is in
> `docs/m6-acceptance.md`. Reconstructing verdicts after a restart is exactly
> what cost M5 four criteria that had in fact been observed.
>
> **M6 C3 is NOT closed** — its observation came in passing during the C7 run
> (order rows published 74 → 84 while five aggregates were blocked), but its
> falsifier needs the pre-fix binary with neighbour counts recorded both times,
> and that exists only in tests.
>
> **A5 landed at `4d12363`**: the periodic sync pump. Before it, `drain_outbox`
> ran at startup and shutdown only, which is why the 2026-09-05 C7 attempt
> reached 2 attempts and stopped one short of the amber threshold. Interval is
> 60s by default, overridable with `HOLLER_SYNC_PUMP_INTERVAL_SECS`.
>
> **The dev stack has an ordering defect that has now cost two acceptance
> runs.** `dev-up.ps1` runs the bootstrap before it starts the backend, so
> `[3b/4]` can never enrol the till's sync credential on a cold start and the
> POS comes up with **sync silently disabled**. The working sequence and its
> four traps are in `docs/M6 kickoff.md`; the defect is in `docs/backlog.md`.
>
> Contracts FROZEN at **v0.8.2** (ADR-027 documented `GET /orders`; ADR-022 M6 Phase C aggregator shapes;
> ADR-026 the `order.source` widening); migrations through **sqlite 0035 /
> postgres 0035**. Note 0035 is applied in POSTGRES and NOT YET on the live
> edge database: the widening has no writer, so the edge takes it at the next
> clean bootstrap rather than rebuilding a live file with no trustworthy
> backup (see `docs/backlog.md`).

---

# M5 resume state — 2026-09-02 (HISTORY — superseded by the block above)

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

Contracts are **FROZEN at v0.8.2** (ADR-027 documented `GET /orders`, documentation only; ADR-026 the `order.source` widening; ADR-022 M6 Phase C aggregator shapes; ADR-024 M6 Phase B admin routes;
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
  `HOLLER_TEST_DATABASE_URL`, and **it must name a database called
  `holler_scratch_*`, which you create and drop yourself** — the suite now
  refuses anything else (2026-09-16):

  ```powershell
  docker exec holler-postgres-1 psql -U holler -d postgres -c "CREATE DATABASE holler_scratch_local;"
  $env:HOLLER_TEST_DATABASE_URL = "postgres://holler:holler_dev@127.0.0.1:5432/holler_scratch_local?sslmode=disable"
  ```

  **This line used to say `/holler`, and following it cost a working dev
  stack.** The suite migrates and seeds whatever it is pointed at, so it
  overwrote `owner@holler.test` and `cashier@holler.test` with fixture hashes
  and the till then refused a CORRECT password with a 401 indistinguishable
  from a wrong one. Also **`HOLLER_SKIP_PG_TESTS=1` hides real failures**: it
  masked four `internal/payments` tests that T7a's `billing.manage` check
  broke.
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

**Contracts are FROZEN at v0.8.3** (ADR-028 — at-least-once order replay, and
the line-amendment set declared once), cross-checked against
`packages/contracts/package.json` by `scripts/check-milestone-marker.mjs` —
which caught this very line claiming 0.6.2 after one bump, and 0.8.2 after the
next. **That second catch sat in a red CI for eleven commits** before anyone
read it, which is the point of the CI rule now in CLAUDE.md: a guard that fires
into a wall of pre-existing red is a guard nobody hears.

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
