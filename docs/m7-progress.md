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
