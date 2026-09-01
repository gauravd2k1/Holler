# ADR-020 — Where the sync worker runs, and when it drains

**Status:** Accepted (2026-08-31)
**Date:** 2026-08-31
**Extends:** ADR-013 (outlet deployment target), ADR-011 (edge encryption at rest), ADR-012/§50.1 (authority split), ADR-015 (edge credential sync and LAN transport), ADR-017 (device enrollment).
**Supersedes:** nothing.

## Context

`edge/sync` is complete, well tested, and **called by nothing that ships.**

This was found on 2026-08-31 while staging Milestone 5's acceptance pass, and it
is worth stating precisely because it survived two milestones:

- No `Cargo.toml` in the repository depends on `holler-edge-sync`.
  `apps/pos/src-tauri` takes `holler-edge-database`, `holler-edge-printer` and
  `holler-edge-device`, and not sync.
- `SyncWorker::new` is constructed nowhere outside tests.
- `config::pull_and_apply_config` has exactly one caller:
  `edge/sync/tests/worker_integration.rs:433`.
- `local_outbox` is **written by many paths and drained by none.** Outside
  `edge/sync` and tests, `repo::mark_outbox_published` has zero callers — no row
  has ever been marked published by a shipping process. The one POS caller in
  this area, `orders.rs:444`, is `get_unpublished_outbox_payload`, which reads a
  pending payload in order to amend a quantity. It is a correction path, not a
  publisher.
- The only binaries in the edge/POS tree are `crashpoint`, `devseed` and
  `kds_lan_server`, none of which sync.

**Two acceptance claims rest on a host that does not exist.** M1's item "WiFi
off → order → WiFi on → verify cloud-side" was never performable: with nothing
draining the outbox, the second half of that sentence could not happen. M5's
criterion 6 ("a GRN created at the edge replays to the cloud and reads back
identically") is evidenced today by `edge/sync/tests/cloud_replay.rs`, and M5's
rule is that **no criterion may be evidenced by a test harness**. The test is a
good one — real `cmd/api`, real PostgreSQL, real socket, real ADR-017 enrollment
— and it is still a harness driving a worker no product code constructs.

### What this actually costs: the offline-first promise has never been demonstrated

This is not a wiring tidy-up, and framing it as one would be the third mistake
in the same family.

Holler's differentiator is one sentence: **the restaurant keeps trading when the
internet is down, and the day catches up afterwards.** The first half is real and
has been exercised repeatedly -- the edge takes orders, prints, bills, deducts
stock and receives goods with no uplink, and M1-M5 have observed it doing so.

**The second half has no host and never has.** `local_outbox` has been written by
many paths since Milestone 1 and drained by none. Across five milestones, "and
then it catches up" has been a claim about code that exists, not about behaviour
anyone has seen. Every acceptance run that mattered stopped at the edge boundary,
because nothing could carry it further.

So implementing this ADR is not plumbing. **It is what makes the central product
promise testable for the first time.**

**Why was this not done in Milestone 1?** -- the next reader's question, and it
deserves a real answer rather than an implied lapse. M1's offline-replay item was
written as an acceptance criterion and the crate to satisfy it was built,
reviewed and tested. What was never built was the caller. Nothing failed: the unit
tests passed because they construct the worker themselves, the integration tests
passed because they do too, and CI stayed green because a crate with no consumer
still compiles. The criterion was then recorded as met on the strength of that
green suite. Every later milestone inherited the shape and added its own replay
stream to a pump nobody started -- M5's T3 landed cursors and a per-entry retry
budget into a worker with no host.

The lesson generalises past sync, which is why this is a section and not a status
line: **a test that constructs its own subject cannot detect that nothing else
constructs it.** The project already knows the narrow form -- "a column nothing
reads is a column that does not exist" (contracts 0.5.2, again at 0.5.9), and "a
deduction test proves deduction only for the path its caller takes" (M4 criterion
1). This is the same defect at crate scale, and it survived longest because it was
largest.

This is the project's recurring failure at crate scale. "A column nothing reads
is a column that does not exist" (contracts 0.5.2, again at 0.5.9) becomes: **a
crate nothing calls is a crate that does not exist.** Every guard was green
throughout, because the wire shape was never the broken part.

## Decision

### 1. The sync worker is hosted **in the POS Tauri process**

ADR-013 decides it. Outlet machines are bare Windows 10, 4GB, spinning disk, no
IT staff, and the outlet runs **one native executable** over one SQLite file. A
second thing to install, supervise, restart and debug at a customer site is paid
forever, by people who cannot do it; the cleanliness of process separation is
paid once, by us. That trade is not close.

**Checked before relying on it:** the one-executable premise is intact, and
there is already a precedent for exactly this pattern. `kds_lan_server` appears
as a binary in the edge tree, which would weaken the premise if it shipped
alongside the POS — it does not. The KDS LAN server **runs inside the POS
process**: `state.rs:114` starts it during `AppState` construction and holds a
`LanServerHandle` in state, and `lib.rs:105` stops it on `RunEvent::Exit`. The
standalone binary is a development and two-machine-runbook convenience. So a
background service with a handle in `AppState` and a shutdown hook is the
established shape here, not a new one.

**Rejected: a separate edge service.** Cleaner separation and independently
restartable, at the cost of a second install, a second supervisor (a Windows
service or a scheduled task), a second failure mode invisible from the POS, and
a second thing to explain to a restaurant owner. It also does not buy what it
appears to: both processes would contend for the same encrypted SQLite file,
which ADR-011 requires to be sealed on exit, so the separation introduces a
sharing problem it does not solve.

### 2. Sync drains at BOTH ends of every trading day

Hosting in the POS process means sync stops when the till closes. That is
acceptable for inventory config, which can be a day stale without anyone
noticing. **It is not acceptable for a day's invoices and payments**, which
would otherwise sit unreplayed overnight, or across a holiday, or through a
weekend the restaurant is shut.

So the host is not "start a worker at launch and let it run":

- **Drain on graceful shutdown, before the process exits.** The outbox is pumped
  to completion (or to a bounded deadline) as part of `RunEvent::Exit`.
- **Drain on next launch, before anything else.** Ahead of the first sale of the
  day, not lazily whenever a timer first fires.

Together these convert a weak property — *"syncs while the till is open"* — into
one that can be stated to a restaurant: **"your day reaches the cloud at both
ends of every trading day."** That sentence is the decision; the periodic pump
while open is an optimisation on top of it.

**Ordering constraint at shutdown, and it is not optional.** `lib.rs`'s exit
handler already seals the edge database via `Db::shutdown_in_place()` (ADR-011:
the decrypted SQLite caching Argon2id hashes must not be left on disk). The
drain needs a live database connection, so it runs **before** the seal, in the
same handler, after `shutdown_lan_server()`. A drain placed after the seal
cannot publish anything at all.

**Correction, verified 2026-09-01.** This ADR first said such a drain would
*silently do nothing*. It does something worse and louder. `Db::connection` is
`self.conn.as_ref().expect("edge database handle used after shutdown")`, so a
drain below the seal **panics inside the exit handler**
(`edge/database/src/lib.rs:208`). Falsified both ways in
`apps/pos/src-tauri/tests/adr020_outbox_drain.rs`: the same state, same worker
and same fake cloud publishes 3 rows before the seal and reaches the cloud zero
times after it, with three rows deliberately left pending so "published
nothing" cannot be read as "had nothing to publish". The conclusion is
unchanged and the reason is sharper: **nothing replays**, and the ordering is
load-bearing.

**The shutdown drain must be bounded and must never block exit indefinitely.**
An outlet closing with no uplink is the normal case, not an error: the drain
attempts, gives up on a deadline, leaves the rows in the outbox, and exits. The
startup drain picks them up. A shutdown path that hangs waiting for a network
that is not coming is a worse defect than the one this ADR fixes.

### Correction, 2026-09-02: the host must drive THREE pumps, not one

The first implementation called `pump_outbox` alone, and that is not the outbox.
`worker::pump_outbox` routes orders and table sessions; a goods receipt is
`("goods_receipt_note", "GoodsReceiptRecorded")`, which that router does not map
at all — it reports the row as `unrouted_skipped` and leaves it pending. Ledger
entries and stock gaps are a third stream again (`pump_ranged_streams`), and
procurement has its own (`pump_procurement`) carrying the per-entry retry budget.

**So the drain would have reported success while every GRN, purchase return,
transfer and ledger entry stayed in the outbox.** Worse than the ordering trap
this ADR already warns about: that one publishes nothing and says nothing, this
one publishes nothing and says *"drain published 0 rows"*, which reads as an
empty outbox rather than an unrouted one.

Found while writing the procedure to close M5 criterion 6 — by asking which code
path a GRN actually takes, rather than assuming "the outbox" was one thing. The
drain now runs all three pumps and names the stream in every stop message,
because "the drain stopped" without saying which stream is the swallowed-stderr
failure from the CI sweep, one layer in.

### Scope of the first implementation

Landed: the worker is constructed in the POS Tauri process, held in `AppState`
beside the `LanServerHandle`, drained on launch inside `AppState::open`, and
drained again from `RunEvent::Exit` before the seal. That is the guarantee this
ADR names -- both ends of every trading day.

**Not landed: the periodic pump while the till is open.** This ADR calls it an
optimisation on top of the guarantee and it stays one. The cost of leaving it
out should be stated rather than discovered: a till open all day, whose uplink
returns mid-service, will not replay until it closes. Nothing is lost -- the
rows sit in `local_outbox`, which is what it is for.

**Also not landed: the config pull still has no caller.** `pull_and_apply_config`
remains driven only by tests. This ADR hosts the OUTBOUND half, which is the half
M1's acceptance and M5's criterion 6 both need; the inbound half is what the
inventory-config-push item in `docs/backlog.md` waits on. Hosting one direction
is not hosting both, and that backlog entry must not be closed on the strength of
this change.

## Consequences

- **M1's offline-replay acceptance item must be re-run once the host exists,**
  and until then it is unevidenced. Recording it as met was never justified;
  nothing could have performed the check.
- **M5 criterion 6 stays blocked** until a GRN replays through the hosted worker
  rather than through `cloud_replay.rs`.
- **Criteria 1, 2, 3, 4 and 7 are unaffected** — every one is offline-only and
  concerns what the edge computes and stores. They may be banked before this
  lands, provided the caveat in `docs/RESUME.md` is recorded with them: they run
  against **locally-seeded edge data**, so they evidence offline behaviour and
  say nothing about config transport. Two claims, two pieces of evidence.
- **The inventory config push remains undemonstrated** (`docs/backlog.md`). It
  cannot be verified before this ADR is implemented, because proving a row
  arrived by transport requires an empty start and a real pull — and presence
  after both seeders have run is not evidence of transport.
- `edge/sync`'s existing tests keep their value. They were never wrong; they
  were unhosted.
