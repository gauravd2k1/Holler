# Milestone history

The closed-milestone record, moved out of `CLAUDE.md` so it is not resident in
every session. Nothing here is superseded — it is history plus the retro
findings each milestone left behind. The items that still **bind current work**
stayed in `CLAUDE.md` (see its **Carried out of M6, still live, still binding**
and **Completed milestones** sections); everything else is here.

**Do not re-run any M6 criterion and do not reconstruct a verdict from git
history.**

## M6 — Sync gaps, back office, aggregator framework

**CLOSED and tagged `m6-complete`** (2026-09-11) at contracts v0.8.1. **Seven of
eight criteria observed on the shipping binaries: C1, C3, C4, C5, C6, C7, C8** —
evidence in `docs/m6-acceptance.md`, handover in `docs/m6-phase-c-boundary.md`.
Record it honestly, in these three parts:

- **M6 C8 is `SHAPE ONLY — no integration evidence`.** Both adapters run against
  fakes we authored, so it proves the contract shape twice and the integration
  zero times. Its integration half travels to M6.1 as an explicitly UNMET row
  (M6.1 C1), carried and never merged into C8.
- **M6 C2 is PARKED and M6 closed WITH it parked, deliberately.** Trigger: *any
  platform sandbox access granted*. It cannot be evidenced from our own logs —
  "never evidence a snooze from our own log" is the criterion's own wording — so
  M6 closed without it rather than with a fake pass.
- **Phase A closed FIVE of seven: A1, A1b, A2, A3, A5 landed; A4, A6, A7
  carried**, each in `docs/backlog.md` and `docs/pilot-readiness.md` with the
  trigger *before the first pilot*. **Never report it as "Phase A complete".**
  A7 is not cosmetic: `edge/sync/src/route.rs` maps only `order` and
  `table_session`, so 78 rows on the live edge database have no route and can
  never be sent (55 `kot`, 22 `stock_count`, 1 `invoice`, measured 2026-09-07).

## M1 Core POS and M2 Kitchen

Both complete. M2's acceptance item 5 — one real KDS↔edge socket session — **is
met**, re-evidenced 4/4 against a real socket after ADR-017. Record it honestly:
it stood recorded as met while its test bridge silently failed to **compile**
for a period, so the `lan-integration` CI job was failing at `cargo build` and
proving no socket session at all (`docs/RESUME.md` §5). The `rust-seams` job and
`make check-seams` exist so a tenth such break fails fast.

## M5 Procurement

**CLOSED at contracts v0.6.3** — seven of seven criteria observed on the
shipping binaries, evidence in `docs/m5-acceptance.md`. Two findings outlive the
milestone. **An acceptance criterion satisfied by either of two definitions
cannot tell you which one you built** (criterion 7 passes under both a lifetime
and an on-hand cost average, and reports neither). And **a test condition the
environment cannot produce is not a weak test, it is no test** — every "network
disconnected" step since M1 was performed by switching WiFi off against a cloud
at `http://localhost:8080`.

## M4 Inventory & Recipes

**Complete and tagged `m4-complete`** — all seven acceptance criteria observed
against the shipping binaries, none evidenced by a test harness. Criterion 1 was
CONTESTED for four days and closed by `7e88d1c`: the till hardcoded
`variantId: null`, so no sale the POS ever took wrote a ledger row, while the
harness that evidenced the criterion selected a variant directly. **A deduction
test proves deduction only for the path its caller takes.** Criterion 6 was
falsified, not merely observed, and the falsification found a dropped field the
201-echo comparison structurally could not see.
