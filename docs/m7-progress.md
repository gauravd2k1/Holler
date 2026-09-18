# M7 progress — per-track reports

One §95 report per finished track, appended in the order the tracks ran. Each
carries an **Executed / Read-verified** split, because the distinction is the
whole point: a thing this session ran and watched, versus a thing it read and
believed. Read-verified is never a pass.

The run these reports come from is the autonomous run of 2026-09-18, scoped by
the operator to **B0 → B1 → B2 and a hard stop after B2**. A6, A7, F2-T0 and
F6-T0 were explicitly NOT to be started in it.

---

## B0 — `e2e-scenario` red: the harness's tax rules hung off a compliance version nothing resolved

**Branch:** `m7-b0-e2e-compliance-version` → merged to `main` at `3b92f27`
**CI on a fresh checkout:** run `35329292383`, **`completed success`, 16 of 16
jobs green.** The first fully green run in the visible history of this
repository — every run before it carried `e2e-scenario` red.

### Implemented

`tests/e2e-scenario/harness/src/main.rs` no longer mints its own
`compliance_version` and pins its tax rules to it. It reads back the versions
devseed actually wrote for the outlet and pins its rules to whatever
`holler_edge_database::tax::resolve_compliance_version` resolves — the same
function the billing path calls. The old mint survives as the empty-database
fallback, so a bare template carrying no catalogue tax config still bills.

`tests/e2e-scenario/harness/Cargo.toml` gains `chrono`, which supplies the
instant the resolver resolves AT. Same version and feature set `edge/database`
uses, so there is one chrono in the graph. Test-only crate; no product code and
no contract was touched.

### The cause, which is NOT what `docs/m7-kickoff.md` recorded

The kickoff (§0) records the fix as *"the harness reuses devseed's compliance
version instead of minting its own"*. **The repository contradicts that, and
the repository wins.** devseed's own comment (`edge/database/src/bin/devseed.rs`
, the `TAX_PROFILE_FOOD5_ID` doc block) states that the only
`compliance_version` the crate seeds lives behind `HOLLER_SEED_BILLING=1`,
*"precisely so the harness's bare (non-billing) devseed run never gains a second
compliance_version row and never sees its own resolution silently redirected to
this one"*. The harness does not set that variable, and minting its own was the
**designed** behaviour resting on that promise.

**The promise went stale.** `write_tax` now seeds the shared catalogue's
`compliance_version` (`0191a000-…-0040`) **unconditionally** — `seed`'s own
comment says so — because every spec menu item's `tax_profile_id` points at a
profile hanging off it. So the outlet carried **two** versions, both with
`effective_from` 2020-01-01T00:00:00Z. `resolve_compliance_version` returns the
latest `effective_from <= at` and **has no tie-break beyond insertion order**,
so it returned devseed's, under which the harness's own tax profile has no
rules at all.

Resolving rather than hardcoding devseed's id is deliberate: a third version or
a renumbered catalogue cannot desync this again, because the harness now asks
the same question the code under test asks.

### Verified — EXECUTED

| What | Result |
|---|---|
| Reduced CI run locally (50 fixed-seed scenarios + 4 named regressions), through `node scripts/assert-tests-ran.mjs vitest` | **1 test executed**, 93.20s, passed. Not "passed" without a count — the count is the point |
| `9_tax_reconciliation` in the run report | **54 checked, 54 passed, 0 failed.** Against `expected 33 to be +0` before |
| `issue_invoice rejected` occurrences in the run report | **0.** Against one per failing scenario before |
| Shape counts, proving invoices issue at all rather than the invariant being skipped | 24 discounts applied, 21 multi-part splits, 54 print jobs printed |
| `cargo build` (harness), `cargo fmt --check` (harness) | clean |
| CI on a fresh checkout, run `35329292383` | **16/16 green**, including `e2e-scenario` |

**On the falsifier.** The pre-fix binary does not pass this criterion: the same
job was red on `fdf295b` and on every run before it with exactly this
assertion, and the local run reproduces green only after the change. The
alternative explanation "it was flaky" is excluded by the failure being present
on every run in the visible history rather than intermittently.

### Verified — READ-VERIFIED ONLY (not a pass)

- The claim that `write_tax` is reached on the harness's devseed invocation is
  read from `seed`'s call order in `devseed.rs`, not instrumented. The
  behavioural consequence *was* executed (the resolver returned devseed's id,
  which is why the job was red), so this is an explanation, not a load-bearing
  assertion.
- No check was run for other consumers of `COMPLIANCE_VERSION_ID` outside the
  harness. The constant remains in the file as the fallback, so nothing was
  orphaned.

### Performance

Local reduced run 93.20s. The `e2e-scenario` CI job remains the slowest in the
file. No change either way — this fix removes failures, not work.

### Remaining

- **The stale comment in `devseed.rs` was not corrected.** It still promises
  that the only `compliance_version` devseed seeds is gated behind
  `HOLLER_SEED_BILLING=1`, which is no longer true and is what misled the
  harness. Correcting it touches `edge/database`, which is outside this track's
  stated files, so under the run's stop rules it was left alone and is recorded
  here instead. **It is the next person's trap.**
- Nothing guards against a third `compliance_version` appearing with an
  identical `effective_from`. The harness is now immune by construction; the
  product path still resolves by insertion order when two versions tie.

### Next

B1.

---
## B1 — Admin console serving and sign-in error clarity: **BLOCKED, no code written**

**Branch:** none. **Commits:** none. Nothing was implemented, deliberately.

### Why it is blocked

**Every one of B1's four acceptance criteria terminates in a person looking at
Chrome on the demo laptop**, and the track's own design makes the first of them
a precondition for the rest. `docs/m7-kickoff.md` §F1 states it plainly:
B1-T0 is *"the falsifier: without it, every fix below is speculative"*.

Under the run's stop rule — *an acceptance criterion cannot be executed;
read-verified only is not a pass* — writing T1/T2/T3 now would be producing
exactly the speculative work the track forbids, then reporting it as landed
because CI happened to be green. CI cannot see any of this.

It is also blocked mechanically, not only by policy:

- The admin console runs on **5175** and the API on **8080**. Both are on the
  forbidden-port list for this session, and the failure under investigation is
  *origin-specific*, so moving either to a scratch port changes the thing being
  measured.
- A scratch-port backend needs Postgres. **Docker is not running on this box**
  and starting it is outside the track.
- The browser automation available to this session cannot stand in: the
  observation required is the operator's own Chrome, with their extensions and
  their HTTPS-First setting, which is the entire hypothesis space.

### What this track produced anyway: three corrections to the kickoff

Read-verified, from the stated files. Each changes what the work is.

1. **CORS is already a list and already exact-matches — the "one origin"
   is a DEV SCRIPT DEFAULT, not a server limitation.**
   `backend/internal/platform/httpx/cors.go:34` takes
   `allowedOrigins []string` and matches each exactly. The single origin comes
   from `scripts/dev-up.ps1:62`, `-AdminOrigin = "http://localhost:5175"`,
   passed as `HOLLER_CORS_ALLOWED_ORIGINS`. So B1-T3 is a one-line dev-script
   change, not a backend change.
2. **The kickoff cites `config.go:62` for this. There is no such line.** The
   file is `scripts/dev-up.ps1` and the line number happens to match. A reader
   sent to `config.go` finds nothing and concludes the claim is stale.
3. **"All three branches already exist in code" is WRONG, and the missing one
   is the whole defect.** `api.ts:34-38` has `configError()` for the missing
   variable. `session.ts:80-87` has the API-refusal message, and it is
   **deliberately undifferentiated** — its own comment says ADR-012 returns the
   identical answer for a wrong password and a throttled attempt, because a
   distinguishable throttle is an account-enumeration oracle. **Do not
   "improve" that one.** The third branch — *the request never left the
   browser* — does **not** exist: a `fetch` rejection surfaces as the raw
   `TypeError: Failed to fetch`, caught nowhere and re-worded nowhere. That is
   why the screen reports a blocked request in the words of a wrong password.

**Consequence for whoever picks this up:** T2 is narrower than written (add one
transport branch; leave the credential message exactly as it is), and T3 is a
dev-script default rather than a backend change. T1 is unchanged.

### What the operator must do to unblock it

One observation, five minutes, on the demo laptop:

1. Open the admin console in **Chrome** — their normal profile, extensions on,
   HTTPS-First at its default.
2. Paste the Console snippet at `docs/demo-runbook.md:450` into DevTools on the
   admin page. It is the same call the form makes.
3. Record **which** of the four candidates it names (HTTPS-First upgrade, an
   extension, a stale service worker, the wrong origin in the address bar) and
   write that into `docs/visual-verify.md` as a new row.

Until that row exists, B1's fixes cannot be distinguished from guesses.

### Verified — EXECUTED

Nothing. Stated plainly rather than dressed up: **no part of B1 was executed
this run.**

### Verified — READ-VERIFIED ONLY (not a pass)

The three corrections above, from `cors.go`, `dev-up.ps1`, `api.ts`,
`session.ts` and `docs/demo-runbook.md`.

### Next

B2, per the run's ordering. B1 is left with no branch and no partial work in
the tree.

---
## B2 — the till is not the only writer of its own state: **T0 and T3 landed, T1/T2 not**

**Branch:** `m7-b2-query-key-guard` → merged to `main` at `d1f8c7c`
**CI on a fresh checkout:** run `35336799438`, **`completed success`, 16 of 16
jobs green.**

### Implemented

- **B2-T0** — `docs/m7-b2-sinks.md`: every write path into the edge database
  that does not originate in the POS webview, and a ruling for every key in
  `queryKeys` against them.
- **B2-T3** — `scripts/check-query-key-freshness.mjs`, wired into
  `.githooks/pre-push` and the `contracts` CI job. Every key must carry a
  ruling: `poll`, `event`, `uncovered` naming the owning track, or `exempt`
  with a reason. A key added with no ruling fails the build.

**No runtime code was changed by this track.** B2-T1 (landing the freshness
fixes) and B2-T2 (choosing poll versus event) are NOT done — see Remaining.

### The two corrections this track forced

**1. The push channel B2-T2 was to "propose" already exists.** The kickoff's
§F4 says *"there is no push from the LAN hub into the till's UI"*. It has
existed since D14 (`97bc3dc`): `apps/pos/src-tauri/src/lib.rs:62-90` subscribes
to the in-process hub for every station and forwards `KotUpserted`/`KotRemoved`
as `holler://kitchen-changed`; `OrderListScreen.tsx:312-327` listens and
invalidates **both** the KOT key and the orders key. Its shape is already
right — the payload is an id and the row is re-read through the same command,
never patched from the event.

**The defect is a MOUNTING defect.** That listener lives in `KotsPanel`
(`OrderListScreen.tsx:280`), mounted only while an order's Kitchen panel is
expanded. Collapsed — the normal state, and the state during the demo failure —
nothing in the process is listening. **That one fact explains both reported
symptoms**, which is why they were filed as two defects: the "only updates on a
remount" kitchen status is the collapsed case plus `staleTime: 0` refetching on
the next mount, and the waiter's order never appeared because a captain order
not yet sent to the kitchen creates no KOT at all, so the hub has nothing to
broadcast regardless of who is listening.

**2. THERE IS A FIFTH SINK, AND THE GUARD FOUND IT — THE ENUMERATION DID
NOT.** `pull_and_apply_aggregator_orders` (`edge/sync/src/aggregator.rs:75`)
runs in the A5 worker loop (`edge/sync/src/worker.rs:313`) and writes
`aggregator_order` and the `order` rows those documents become. Nothing in the
webview hears it. **This is demo step 6's path** — the fake ONDC order that
arrives and is accepted on the till.

The hand-written enumeration, produced an hour earlier by reading the code,
said **four**. The guard said five on its first run, by refusing a key
(`unacceptedAggregatorOrders`) that nobody had ruled on. It also refused
`grnGaps` and `purchaseOrderReceiptProgress`, and caught one ruling written
from memory that was simply wrong: **`blockedReplays` is not polled**, though
the document asserted it was. The document is corrected in place and says what
it said before.

This is the enumeration lesson turned on its author. "Enumerate the sinks, not
the surfaces" is right, and a sink list assembled by reading is still a list
with something missing. The only reliable finder is a check over a closed set
the code already enforces.

### Verified — EXECUTED

| What | Result |
|---|---|
| Guard on the unmodified tree, first run | **FAILED — 3 unruled keys and 1 wrong ruling**, before any violation was planted |
| Plant: a new key with no ruling | **caught**, exit 1 |
| Plant: a `poll` ruling with its `refetchInterval` removed | **caught**, exit 1 |
| Plant: an `event` ruling with its invalidation removed | **caught**, exit 1 |
| Guard after the rulings were corrected | OK — 27 keys: 5 polled, 1 event, 12 uncovered, 9 exempt |
| CI on a fresh checkout, run `35336799438` | **16/16 green** |

**WHAT IT WAS WATCHED LETTING THROUGH, before it was trusted** — a check is
falsified by what it lets through, not by what it catches:

- **A `refetchInterval` of one hour passes as a poll.** It checks for the
  property, not for a sane value. Confirmed by experiment.
- **An `event` ruling passes even when its listener is mounted somewhere it
  will not be** — which is the live defect right now. No static check sees a
  mount point.
- **A sixth writer of rows behind an already-ruled key fires nothing.** The
  guard is key-centric, not sink-centric.

All three are written into the script's own header and into
`docs/m7-b2-sinks.md`, rather than left for a reader to discover.

### Verified — READ-VERIFIED ONLY (not a pass)

- **Every claim about what a sink writes**, and every file:line in the
  enumeration. Read from the code, not instrumented.
- **The mounting diagnosis.** `KotsPanel` is a child rendered only when
  expanded, read from `OrderListScreen.tsx:227,280,290`. Consistent with both
  reported symptoms and with VV-009's recorded FAIL, but **not observed in the
  Tauri release window by anyone this run.**
- **That sinks 3 and 5 leave a screen stale.** Derived from the absence of an
  emit and the absence of a poll, not from watching a stale screen.

### Performance

No runtime change. The guard adds ~100ms to `pre-push` and to the `contracts`
job.

### Remaining

- **B2-T1 — land the fixes. NOT DONE, and an agent cannot close it**:
  acceptance criteria 1 and 2 both require observation in the Tauri release
  window, which refuses to launch under `CLAUDECODE`. The enumeration names the
  order to do it in: move the kitchen listener up out of `KotsPanel`, emit on
  the captain's own writes (sinks 2a/2b, the only ones with no channel at all),
  then rule separately on sinks 3 and 5, which are genuine polling cases.
- **B2-T2 — propose poll versus event. Superseded in part**: the event
  mechanism already exists and works, so the question is narrower than the
  kickoff framed it.
- **`m7-b2-stale-screens` (`29de133`) must not be merged as-is.** The
  `refetchInterval: 5000` on `useOrdersQuery` is the right shape *only* if the
  event fixes are not done. It covers one key at the cost of a permanent 5s
  tick.
- **12 keys stand ruled `uncovered`.** That is a declared debt with a named
  owner, not an oversight, and the guard will keep it visible.

### Next

**Nothing — the run's hard stop is here.** A6, A7, F2-T0 and F6-T0 were not
started, as instructed.

---
