# `scripts/demo-reset.ps1` — the one command that resets cloud + edge

Implements `seed/README.md`'s "The one command" and demo-kickoff work item 2
(T10). Read `seed/README.md`'s "The one command" section first -- it is the
binding spec this script follows, in order.

## What it does, in order

1. **Verifies the backend by PID, never by the port answering.** Kills
   whatever owns port 8080, confirms the port is free, starts a fresh
   backend, confirms a *new* process owns the port before continuing. This is
   not cosmetic: a restart that fails to rebind leaves the OLD process
   answering identically, with its in-memory state (the login rate limiter,
   for one) unchanged, which reads as a different failure entirely if nobody
   checks the PID.
2. **Drops and re-applies the Postgres schema**, then runs
   `backend/cmd/devseed` (which itself applies every contract migration
   before seeding -- the same call `scripts/dev-bootstrap.ps1` makes).
3. **Deletes the edge data directory** -- both `edge.db.enc` and the
   plaintext `edge.db` that every run leaves beside it (gap A6, not fixed
   here, worked around by deleting both) -- then runs the edge devseed.
   SQLite migration 0035 applies at this clean bootstrap.
4. **Asserts** the known-clean-state invariants via `scripts/demo-assert`
   (see below), reporting each one by name with its actual row count, and
   exits non-zero on the first failure.

## Usage

```powershell
# See the plan; nothing is touched.
.\scripts\demo-reset.ps1 -WhatIf

# The real reset. Needs the SAME key apps\pos\.env.dev already carries.
$env:HOLLER_DB_KEY_HEX = "<the 64-hex-char key from apps\pos\.env.dev>"
.\scripts\demo-reset.ps1 -Force
```

Prerequisite: `docker compose up -d postgres redis nats` already running.
This script does not start infrastructure -- `scripts/dev-bootstrap.ps1` and
`scripts/dev-up.ps1` own that, and this script reuses their patterns (the
devseed `KEY=VALUE` output parsing, the hex-key validation, the "own window"
backend launch) rather than a second, weaker version of the same logic.

**Non-interactive by design.** `apps\pos\.env.dev` carries the edge
encryption key and is deny-ruled to agents, so an agent cannot run this
script end to end -- only an operator can, watching the output. Every step
says what it is about to do before it does it; every failure names the next
action, never a bare exception.

## `scripts/demo-assert`

A small standalone Rust binary (`scripts/demo-assert/`, path-dependent on
`edge/database`, not a workspace member of it -- this repository's Rust
crates deliberately do not share a cargo workspace) that opens the sealed
edge database directly and checks:

| # | Query | What "zero" means |
|---|---|---|
| 1 | `sync_replay_block` | no ranged-stream (ledger/deduction-gap) row has been permanently given up on |
| 2 | `stock_deduction_gap` | no sale went unaccounted for lack of a recipe -- **not** `grn_gap`, which the demo seed legitimately populates with one `NO_PURCHASE_ORDER` row (ADR-019: a GRN never blocks on a PO) |
| 3 | `sync_outbox_block` where `blocked_at IS NOT NULL` | no general-outbox row has been given up on -- this is what "zero blocked rows in local_outbox" actually queries, since `local_outbox` itself carries no blocked flag (contracts 0.6.4) |
| 4 | `sync_outbox_block` where `blocked_at IS NULL AND attempts >= 3` | no row is stuck retrying after repeated failures |

**Assertion 4 is a database proxy for "the POS sync banner is empty", not an
observation of the banner itself.** It queries the same two tables
`SyncBlockedBanner.tsx` reads via `list_blocked_outbox_rows` /
`list_persistently_failing_outbox_rows` (assertions 3 and 4 together), but
neither this tool nor `demo-reset.ps1` has rendered that component in a
browser. If a rehearsal needs the banner itself confirmed empty, look at the
till.

`demo-assert` reseals the database (`Db::close`) before exiting either way,
so a run of it never adds a second plaintext leftover on top of gap A6's own.

Falsified once, on purpose, before being trusted: a test in
`scripts/demo-assert/src/main.rs` (`reports_a_planted_blocked_row_as_failure`)
plants a fake blocked outbox row directly in a scratch sealed database and
asserts the tool reports it as a failure rather than passing silently.

## What was and was not exercised (2026-09-11)

Run against a real, purpose-built scratch sealed edge database (built via
`edge/database`'s own devseed, in a scratchpad directory, with a throwaway
key -- never the real dev key or the shared dev Postgres):

- `demo-assert` built and run end to end against that database: all four
  checks pass on a clean seed (`0 rows -- OK`), and correctly reseals with no
  plaintext left behind.
- Its falsifier test: a planted blocked row is correctly reported as `FAIL`,
  not silently passed.
- `demo-reset.ps1`'s argument handling: missing key, invalid key length, and
  the `-Force`-required gate all fail loudly with a named next action.
- `demo-reset.ps1 -WhatIf` run against the actual live dev stack on this
  machine (Postgres container up, a real backend already on port 8080):
  confirmed the preflight Postgres check, the destructive-action banner, the
  backend-PID logic finding and naming the real listening PID without
  touching it, and every per-step "would do X" line -- all four steps'
  `-WhatIf` branches printed and none of them touched anything.

**NOT exercised, named explicitly:** the real `-Force` run end to end. It was
deliberately not executed in this session -- it would drop the shared dev
Postgres schema and delete the shared dev edge database, and several parallel
tracks have live state in both right now. The unexercised paths specifically
are: the actual `DROP SCHEMA ... CASCADE` against a live database, the actual
`Remove-Item` of `edge.db.enc`/`edge.db`, the real `go run ./cmd/devseed` and
`cargo run --bin devseed` invocations as driven by this script (their
underlying commands were exercised separately and directly, not through this
script), and the full backend kill-and-restart sequence with a live process
actually being killed by this script. **An operator's first real `-Force` run
is the first time the whole sequence has executed together.**
