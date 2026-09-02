# M6 — plan approved and cut applied, four amendments outstanding
 
Last updated 2026-09-02, evening. Read `docs/m5-acceptance.md` for the closed
milestone and its carry-forward list (the `claude/m5-review-and-decisions.md`
pointer this file carried does not exist — repo wins). **The repo is the authority** —
if it disagrees with anything here, the repo wins.
 
---
 
## Where this stands
 
M5 closed with all seven criteria observed and its evidence committed to
`docs/m5-acceptance.md`. Contracts 0.6.3, sqlite 0030 / postgres 0031, ADRs
through 021, CI green.
 
The M6 plan is approved and committed in `docs/m6-planning.md`. The two-stage
cut was folded in and pushed at **949f658** (superseding 08a29d1). ADR-022
(aggregator order authority) remains in PROPOSED state — it must be ACCEPTED
before any aggregator table is drawn.
 
**Four amendments are outstanding** against 949f658 — see "Amendments pending"
below. Nothing else blocks the fresh execution session.
 
---
 
## M6 scope as approved
 
Three deliverables, in this order:
 
1. **Phase A — the seven sync gaps carried out of M5** (~1 week). P2 500-on-
   client-data, P3 head-of-line blocking, P4 retry budget, P6 `Offline`
   reporting, P5 periodic pump, P7 drain observability, P1 edge route resolver.
   Sync before aggregators, because aggregator orders ride the same outbox.
2. **Phase B — `apps/admin`** (~1.5–2 weeks). Menu and pricing, suppliers and
   pack sizes, purchase orders, staff and permissions, goods-receipt list.
   Admin before aggregator: the supplier criterion needs it, and menu push needs
   a menu surface that is not devseed.
3. **Phase C — the aggregator layer, stage 1 only** (2.5–3.5 weeks). Framework,
   both adapters, drift check, Beckn message surface. **Not a live channel on
   any platform.** "Aggregators done" must never be read as "orders arriving
   from Swiggy".
**Phase D — Network Participant paperwork** runs from week one in parallel:
legal entity, whitelisted domain, SSL certificate, public HTTPS callback host.
Calendar-bound, consumes no engineering time, owned by Gaurav rather than the
pipeline. It now sets **M6.1's start**, not M6's close — nothing in M6 waits on
it, everything in M6.1 does.
 
M6 totals ~5–6.5 weeks, down from 7–9.
 
### Corrections the orchestrator made to the kickoff prompt, all accepted
 
- P6's transport half already landed at 262e03a; P6 is the reporting half only.
- `stale_connection.rs` is closed and is **not** a hardware finding — `os error
  10054`, a socket closed with unread bytes discarding an already-written reply,
  a bug in the test's own fake servers. 0/200 idle, 0/100 under load after fix.
  The "target hardware" reading was inference; their measurement beat it.
- P1 is an edge resolver and envelope job, not a backend build. The cloud ingest
  routes exist; `edge/sync/src/route.rs` maps only `order` and `table_session`.
- P7's premise was wrong — the drain does `eprintln` (`state.rs:298`) and its
  lines were observed. The question is which build/attach state loses them.
- P2 is wider than orders: `23503` is mapped nowhere, `23505` in five contexts.
---
 
## The two-stage cut, as applied at 949f658
 
One principle applied twice: **build what a published schema can verify; defer
what only a live registry can verify.**
 
- **Stage 1 (M6):** framework, both adapters, drift check, Beckn message
  surface — all driven by published schemas, so a schema-generated fake
  genuinely checks them.
- **Stage 2 (M6.1, 1.5–2.5 wk):** Ed25519 signing, registry subscribe and the
  `on_subscribe` X25519 challenge, and the public HTTPS callback ingress.
  Recorded reason: *a self-authored counterparty cannot falsify a signature
  scheme; it can only agree with it.* Triggered by Phase D landing; verified
  against the real registry in one stretch.
- The **ingress security gate** moved under M6.1 in full rather than being
  deleted, so it is not rediscovered. Reviewing it inside M6 would mean
  reviewing it against traffic we generate ourselves — the thing the cut exists
  to avoid. In M6, nothing is exposed publicly at all.
- Certification (2–8 wk, not ours to control) stays outside both milestones and
  still needs a named landing.
**Also offered and not taken:** reorder to A + B → M7 reporting → aggregator
layer, if visible customer value sooner matters more.
 
---
 
## Design rulings already made
 
**§50.1 direction for aggregator orders — two aggregates, not one.**
`aggregator_order` is cloud-authoritative and syncs down (an inbound document
from a system the till cannot receive directly). `order` stays edge-authoritative
and syncs up, created from it and linked by `external_order_id`. Making one
aggregate switch authority by channel is split authority, and it would mean a
delivery-heavy outlet could not bill an aggregator order offline — the opposite
of the product. Published guarantee: **a new aggregator order cannot arrive
while the uplink is down; one that has already arrived is fully operable
offline.**
 
**Four conditions on the "placeholder" adapters.**
 
1. Placeholders fail loudly — `PlatformNotImplemented` at the first line, never a
   success, never a no-op, never selectable by default.
2. **Two working adapters, not one.** A one-adapter abstraction is
   indistinguishable from no abstraction. ONDC (async callback) plus a local
   sync-REST fake proves the contract carries both shapes.
3. Evidence separation: the fake proves **shape**, ONDC staging proves
   **integration**. Two rows in the acceptance file, never one. With staging cut
   to M6.1, the integration row travels with it as an explicitly unmet row —
   it does not close in M6.
4. Structural boundary: a drift check fails the build if `swiggy`, `zomato`,
   `ondc`, `beckn`, `on_*` or the signing header names appear outside their
   adapter module.
**The Beckn fake must be generated from ONDC's published artefacts** — official
schemas, example payloads, reference flows, layer-2 config — each cited with
source and version in the fake's header. A hand-written fake proves only that we
agree with ourselves, which is *a test that constructs its own subject* arriving
in the one place with no external check for six weeks. If the artefacts cannot be
obtained, the adapter — and C8's evidence — is labelled **"unverified against the
specification"**.
 
---
 
## Amendments — FOLDED IN, see `docs/m6-planning.md`
 
Raised at review of the applied cut; all four are now folded in, three as
written and the fourth as a correction. Amendment 4's premise was stale: the
zero-tests-executed audit was EXECUTED at `1b51df0` — three runners probed,
`scripts/assert-tests-ran.mjs` wired into 13 CI steps, every suite count
re-measured — and its only residue, Playwright, was already filed in
`docs/backlog.md` with an M6 trigger. Certification also has its landing now:
a named gate in the backlog, outside both M6 and M6.1.
 
1. **"criterion 7" is ambiguous in the boundary prompt.** Phase A says "watch
   criterion 7's falsifier fail" while the same prompt says "do not re-run M5
   criterion 6" and puts `docs/m5-acceptance.md` in the read list. A fresh
   session will bind it to M5's weighted-average-cost criterion. Must read
   **M6 C7**.
2. **C8 now proves shape twice and integration zero times.** Both adapters run
   against fakes we authored. Retarget C8 as explicitly shape-only, and carry
   the integration row into M6.1's criteria as unmet. Otherwise M6 closes with
   C8 green and it reads as "ONDC works".
3. **"Expose no public endpoint in M6" needs one clarifying line.** The callback
   receive path IS built in M6 and is reachable locally only; what defers is the
   public HTTPS ingress and its security gate. Read the other way, a builder
   removes the async shape and having two adapters stops proving anything.
4. **The zero-tests-executed audit has no home.** M5's rule was recorded, but
   the audit of other claims resting on invocations that could have executed
   nothing is outstanding and appears nowhere in the plan or the boundary
   prompt. Needs a named landing — Phase A alongside A1, or `docs/backlog.md`
   with a trigger.
Minor, non-blocking: certification needs a named landing so M6.1 has a close
condition.
 
---
 
## Acceptance criteria
 
Seven approved, each naming its observation and its falsifying condition, plus:
 
- **C2** (stock-out snooze) is **ONDC-only and parked** behind the trigger
  "when any platform sandbox access is granted". Never evidenced from our own log.
- **C7** (a 500-class client-data failure reported as 4xx with a recorded reason)
  was the orchestrator's addition — P2 is the milestone's first defect and nothing
  else observed it.
- **C8** — an aggregator order flows end to end through **both** adapters with no
  branch on platform identity in the core. Falsifier: introduce a platform branch
  and watch the drift check go red. Retargeted at the artefact-generated Beckn
  fake now that staging moved to M6.1; see amendment 2.
---
 
## Session boundary prompt
 
The orchestrator's boundary prompt (fresh session, M6 execution) is in the
2026-09-02 chat and should be taken from the updated plan after the four
amendments land. Its substance: read `docs/m6-planning.md` first, then ADR-022
(PROPOSED — escalate before drawing any table), `CLAUDE.md`, `docs/backlog.md`
(the only register), `docs/m5-acceptance.md`, and the 2026-09-02 entries in
`docs/retro.md`. Repo wins over plan, said out loud before proceeding. First act:
flip the `CLAUDE.md` milestone block and `.claude/current-milestone` to 6 —
`scripts/check-milestone-marker.mjs` enforces agreement. Then Phase A gap A1
(P2): map SQLSTATE 23503 to 4xx with a recordable reason, then audit every
`httpx.Error`-returning path per bounded context — sinks, not handlers.
Contracts stay frozen at 0.6.3; 0.7.0 lands after Phase A. Menu seed drift stays
untouched until A1 and A2 are red-then-green. Do not re-run M5 criterion 6.
 
---
 
## Standing rules added this milestone
 
- **A milestone does not close until its acceptance evidence is committed to the
  repo.** The chat is not the record. In `CLAUDE.md`, all builder agent files, and
  `verifier.md` as an automatic FAIL.
- **A test invocation reporting zero tests executed is a failure, not a pass.**
  `cargo test -p holler-edge-sync` and `make check-seams` both ran nothing and
  exited 0. The audit of other claims resting on such invocations is outstanding
  (amendment 4).
- Backend runs in its own window via `scripts/dev-up.ps1`, verified by PID at
  step 0 of any acceptance run — never as a session-owned background process.
- Criteria depending on external grants must be identified at planning time and
  given triggers up front, not discovered mid-milestone. Two so far: the ESC/POS
  hardware gate and platform sandbox access.
---
 
## Unresolved, recorded, blocking nothing
 
- The ~120 pending-row count and the `GRN/20260902` ordinal reconciliation —
  Docker/Postgres down, edge DB encrypted. SQL for both stores is recorded; both
  stay UNRESOLVED until Docker is up.
- Platform access: no partner relationship with Swiggy, Zomato or ONDC yet. All
  three tracks to start in parallel; ONDC is the only one without a gatekeeper.
---
 
## Commercial state, kept separate
 
A client proposal deck (`Holler-Client-Proposal.pptx`, 18 slides) and a
competitive comparison document exist for a funding conversation: phase one at
₹34.50 lakh over three months, 4.5 FTE at Tier-1 metro market-mid rates, full
programme ~₹1.40 Cr over eleven months. The deck's M6 wording was updated to
"Aggregator layer & back office" so it no longer implies a live channel in
phase one. Do not raise this unless Gaurav does.