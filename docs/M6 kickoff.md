# M6 — Phase A closed, C7 closed, Phase B is next

Last updated **2026-09-07**, end of session. HEAD is on `main`, everything
pushed. **The repo is the authority** — if it disagrees with anything here, the
repo wins, and say so out loud before acting.

Read `docs/m6-acceptance.md` for the criteria and their evidence,
`docs/backlog.md` for every deferred item (the single register), and
`docs/m5-acceptance.md` for the closed milestone. **The `claude/` directory does
not exist** — earlier drafts of this file pointed at
`claude/m5-review-and-decisions.md`, which has never existed; that pointer was
removed once and reintroduced by a rewrite. Do not restore it.

---

## Where this stands, in one paragraph

**Phase A is CLOSED with five of seven gaps landed and three carried.**
**M6 C7 is CLOSED**, observed end to end on the shipping binaries on 2026-09-07.
Contracts are FROZEN at **v0.8.0** (ADR-022); migrations through **sqlite 0032 /
postgres 0033**. **Next work is Phase B, `apps/admin`** — a directory that has never
existed. Nothing is mid-flight; no branch is open; no test is red.

---

## Tomorrow's first action

**Start Phase B.** There is no outstanding observation to make, no operator run
pending, and no evidence uncommitted. If you are a fresh session after a reboot:
you are not resuming a run, you are starting a new phase.

The one judgment call put to the operator and not yet answered: whether Phase B
builds **all five** admin surfaces as scoped (menu and pricing, suppliers and
pack sizes, purchase orders, staff and permissions, goods-receipt list) or
narrows to the two that M6 C5 and C6 actually touch (suppliers/pack sizes, and
the goods-receipt list). The recommendation on record is **all five**, because
Phase C's aggregator menu push needs a menu surface that is not `devseed`.

---

## Phase A — what closed and what did not

**Landed: A1, A1b, A2, A3, A5.** **Deferred: A4, A6, A7**, each in
`docs/backlog.md` with the trigger *before the first pilot*.

| ID | Gap | State |
|---|---|---|
| A1 | Cloud returned 500 for a client-data failure | **LANDED** `99875cc`, `ab6c201` |
| A1b | Two more ingest paths, 500 and 404 | **LANDED** `856616b`, `3f7abaa` |
| A2 | Head-of-line blocking stranded the whole outbox | **LANDED** `07d7968`, `59c2ea3` |
| A3 | Retry budget never spent; nothing surfaced a blocked row | **LANDED** `c95dc24`, `078d4e5`, `c8147ef` |
| A5 | No periodic sync pump | **LANDED** `4d12363` |
| A4 | `Offline` conflates four states | **DEFERRED** |
| A6 | Shutdown drain silent; window close does not exit the process | **DEFERRED** |
| A7 | Rows with no edge route | **DEFERRED** |

**NEVER REPORT THIS AS "PHASE A COMPLETE".** A7 is not cosmetic: **78 rows on
the live edge database have no route and can never be sent** — 55 `kot`, 22
`stock_count`, 1 `invoice`, measured 2026-09-07. The order stream replays
correctly end to end; nothing else replays at all.

**The consequence that binds Phase C:** contracts may proceed to 0.7.0 on the
strength of the order stream, but **A7 must close before any aggregate beyond
`order` is expected to replay.** Discovering that during the aggregator build is
the expensive way to learn it.

---

## M6 C7 — closed, and how

**Observed 2026-09-07 by the operator on the shipping binaries.** Both halves of
the falsifier were watched: the pre-fix 500 on 2026-09-03, the post-fix
422-stored-and-surfaced on 2026-09-07. Full evidence, with the row table and the
verified preconditions, is in `docs/m6-acceptance.md` — **read that, do not
reconstruct this from git history.** That reconstruction failure is exactly what
cost M5 four criteria after a restart.

Three things worth carrying forward from the run itself:

1. **The 2026-09-05 attempt failed for a mechanical reason, not a code fault.**
   It reached 2 attempts and stopped, one short of the amber threshold, because
   before A5 the drain only ran at startup and shutdown — so "watch five pumps"
   with the window open could not move the counter at all. The banner was
   correctly absent and there was no defect in it.
2. **C7's falsifier is three events, not one.** "4xx, reason stored, row
   surfaced" spans thresholds: `OUTBOX_ATTENTION_ATTEMPTS = 3` for the amber
   row, `MAX_OUTBOX_REPLAY_ATTEMPTS = 5` for blocked. A single order and a few
   pumps cannot satisfy it. The criterion in `docs/m6-acceptance.md` now says
   so.
3. **The "records" half was tested across an unclean exit**, because closing the
   POS window does not terminate the process. Crash recovery replayed the WAL,
   resealed superseding a sealed file 79 minutes stale, and the banner returned
   identical.

**M6 C3 is NOT closed.** The run produced its observation in passing — order
rows published 74 → 84 while five aggregates were blocked — but C3's falsifier
requires the same fixture on the pre-fix binary with neighbour counts recorded
both times, and that has only been done in tests.

---

## The operational runbook, recorded because rediscovering it cost two runs

**`dev-up.ps1` runs the bootstrap BEFORE it starts the backend**, so the
bootstrap's step `[3b/4]` — which enrols the till's sync credential against the
API — can never succeed on a cold start. It prints
`[3b/4] sync credential SKIPPED: no API at http://localhost:8080`, writes no
device token, and **the POS then comes up with sync silently disabled.** Two
acceptance runs were lost to this before it was diagnosed. Filed in
`docs/backlog.md`; the fix is a restructure across both scripts.

Until that is fixed, the working sequence is:

```
# 1. backend only, bootstrap skipped
#    -AdminOrigin defaults to http://localhost:5175 and sets
#    HOLLER_CORS_ALLOWED_ORIGINS. WITHOUT IT apps/admin fails every request
#    with "Failed to fetch" and neither log says why.
.\scripts\dev-up.ps1 -SkipInfra -SkipSeed -NoKds -NoPos
Get-NetTCPConnection -LocalPort 8080 -State Listen | Select-Object OwningProcess
#    verify a NEW pid -- the port answering proves nothing

# 2. bootstrap by hand, with the backend already listening
$env:HOLLER_DB_KEY_HEX = (Select-String -Path apps\pos\.env.dev -Pattern '^HOLLER_DB_KEY_HEX=(.+)$').Matches[0].Groups[1].Value
$env:HOLLER_DB_KEY_HEX.Length          # MUST print 64
.\scripts\dev-bootstrap.ps1 -SkipInfra -WithBilling
#    [3b/4] must say "enrolling" or "rotating", never SKIPPED

# 3a. the back office (optional; needs the CORS origin above)
cd appsdmin; pnpm dev        # http://localhost:5175
#     sign in as owner@holler.test / holler123 -- the CASHIER cannot manage
#     the menu, and that is deliberate (§50.1: the till never authors one)

# 3. the till -- ONE terminal, not two
$env:HOLLER_SYNC_PUMP_INTERVAL_SECS = "10"
.\apps\pos\run-dev.ps1
```

Four traps in that sequence, each of which has actually bitten:

- **Do NOT start `pnpm dev` separately.** `tauri.conf.json`'s
  `beforeDevCommand` is `pnpm dev`, so `tauri dev` starts Vite itself; a second
  one aborts the launch with `Port 5173 is already in use`.
- **A wrong or missing `HOLLER_DB_KEY_HEX` does not error usefully.** The
  bootstrap refuses to run without one, but running with a *different* one
  builds a separate empty database and the previous state is simply gone.
- **`LNK1104: cannot open file ...exe` is McAfee**, not a code fault. Re-run;
  cargo keeps every binary that already linked, so two or three attempts reach
  green.
- **Closing the POS window does not stop the POS.** `holler-pos.exe` keeps
  running with the pump ticking and the database open and unsealed. Use
  `Ctrl+C` in the launching terminal, then confirm with
  `Get-Process holler-pos`.

---

## Findings from the C7 run, all filed, none blocking

- **The banner prints `aggregate_id`, so one order appears once per outbox
  row.** A four-item order showed as three identical-looking entries. The count
  climbs toward ~20 for what is 5 orders, and an operator reads that as 20 lost
  orders. Open design question: whether successor rows of an already-abandoned
  aggregate should be attempted at all.
- **The banner covers the POS top bar** — `position: fixed; top: 0` with
  content-dependent height, hiding the search box and the order-type row. Third
  fixed-overlay collision in this codebase.
- **An unclean exit leaves the edge database plaintext on disk** beside its WAL,
  carrying Argon2id password and PIN hashes. **Recovery-on-open DOES exist**
  (`crypto::recover_crash_leftovers` replays, reseals superseding the old
  `.enc`, wipes) — an earlier version of the backlog entry claimed otherwise and
  was corrected the same day. The real exposure is that recovery only runs when
  that database is next opened, which was two days in the observed case.

---

## M6 scope, unchanged

Phase A (closed, three carried) → **Phase B `apps/admin` (~1.5–2 wk, NEXT)** →
Phase C aggregator stage 1: framework, both adapters, drift check, Beckn message
surface (2.5–3.5 wk).

**M6.1** — Ed25519 signing, registry subscribe / `on_subscribe`, public HTTPS
callback ingress and its security gate. Triggered by Phase D (NP paperwork). In
M6 the callback receive path IS built and is reachable **locally only**.

**C8 is SHAPE ONLY in M6**; the integration row travels to M6.1 as M6.1 C1,
marked CARRIED FROM M6 AS UNMET. **C2** (stock-out snooze) is parked behind
"any platform sandbox access granted". **Certification is outside both
milestones.**

---

## Design rulings that still govern

**§50.1 — two aggregates, not one (ADR-022, PROPOSED).** `aggregator_order` is
cloud-authoritative and syncs down; `order` stays edge-authoritative and syncs
up, linked by `external_order_id`. **ADR-022 must be ACCEPTED before a single
aggregator table is drawn.**

**Four conditions on the adapters.** Fail loudly (`PlatformNotImplemented`); two
working adapters, not one; evidence separation (fake proves shape, staging
proves integration — two rows, never one); a drift check on platform names
outside the adapter module, **watched going red before it is trusted**.

**The Beckn fake is generated from ONDC's published artefacts**, each cited with
source and version. If they cannot be obtained, the adapter and C8's evidence
carry the label "unverified against the specification".

---

## Unresolved, blocking nothing

- `edge/sync` has no host outside a test process (standing backlog item).
- Platform access: no partner relationship with Swiggy, Zomato or ONDC yet.
- ~24 backend files show as modified with zero content diff — CRLF/LF
  normalisation, pre-existing, not a change anybody made.

---

## Commercial state, kept separate

A client proposal deck (`Holler-Client-Proposal.pptx`, 18 slides) and a
competitive comparison document exist for a funding conversation: phase one at
₹34.50 lakh over three months, full programme ~₹1.40 Cr over eleven months. The
deck's M6 wording reads "Aggregator layer & back office" so it does not imply a
live channel in phase one. Do not raise this unless Gaurav does.
