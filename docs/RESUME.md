# M3 resume state — 2026-08-20

`main` is committed and clean at **`c857623`**, pushed to `origin/main`. Nothing
is in flight.

Read this file first. Then `docs/adr/ADR-016-m3-billing-contracts.md` (0.4.4,
0.4.5 addenda), `docs/adr/ADR-017-device-enrollment-credential.md`,
`packages/contracts/sqlite/0012_printer_role.sql` (0.4.7 — its own rationale is
the best short statement of the config/authority split), and the 2026-08-14 /
2026-08-15 entries in `docs/retro.md`.

**Next: M4 planning — competitive gap reconciliation. Start a fresh session and
read this file first.**

---

## 1. M3 status — code-complete, proven by test

All seven billing deliverables are implemented and have tests. Every number
below was produced by running the named command on this machine on 2026-08-20.

| Deliverable | Where | Evidence |
|---|---|---|
| Tax engine (GST, per-component half-up, round-off) | `edge/database/src/tax/` | harness invariant 9, 54/54 |
| Invoice numbering (edge-local counter, offline-safe) | `edge/database/src/invoice/numbering.rs` | unit + `edge/database` suite |
| Split payments / tenders + reversals | `edge/database`, `commands/billing.rs` | harness invariant 10, 54/54 |
| Cash shift (open/close, variance, recovery) | `commands/billing.rs` | `apps/pos/src-tauri/tests/billing_flow.rs` |
| GST invoice printable | `edge/printer/src/template.rs` + enqueue | harness invariant 13, 54/54 |
| Split bills reachable from the shipped surface | `issue_split_invoices` | harness invariant 12, 21/21 |
| Per-line discounts real and governed | `domain/discount.rs`, `issue_invoice` | harness invariant 11, 54/54 |

| Suite | Result | Command |
|---|---|---|
| `edge/database` | **175** | `cargo test` |
| `edge/printer` | **45** | `cargo test` |
| `edge/sync` | **28** | `cargo test` |
| `edge/device` | **11** | `cargo test` |
| `apps/pos/src-tauri` | **70** | `cargo test` |
| `apps/pos` | **165** | `pnpm test` |
| `apps/kds` | **30** | `pnpm test` |
| `packages/contracts` TS / Go | **40** / ok | `npx vitest run`, `go test ./...` |
| **e2e harness** | **54 scenarios, 13/13 invariants, 0 fatals** | `cd tests/e2e-scenario/orchestrator && pnpm test` (~85s) |

**`backend` was NOT re-run this session** — the one gap in the table above.
Last known: 287, 0 skips, at `6575c9f`. Treat that number as stale, not as
evidence: nothing in this session touched Go, but nothing re-verified it either.

Running it is cheap and should be the first thing a fresh session does:

```powershell
docker compose up -d postgres redis nats     # if not already up
$env:HOLLER_TEST_DATABASE_URL="postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable"
cd backend; go test -count=1 ./...
```

**Scale, measured on 2026-08-20:** `go test -list ".*" ./...` reports **253
top-level test functions across 18 packages**, and compiled in 3s against a warm
build cache. The 287 figure above counts subtests (`t.Run`) as well, so the two
numbers are not in conflict — expect the run to report more than 253.

**Duration is an estimate, not a measurement** (nobody has timed this suite):
likely **under two minutes** warm. The ~29 Postgres-backed integration tests
dominate, packages run in parallel, and the containers were healthy at session
close. A cold checkout adds Go compile time on top. If it runs much longer than
that, suspect a container that is up but not healthy rather than slow tests.

`-count=1` is not optional: without it Go serves cached results and the run
proves nothing. Do not set `HOLLER_SKIP_PG_TESTS` — that restores exactly the
silent green the zero-skip CI assertion exists to prevent.

### The §66 financial suite is falsifiable, not just green

Invariants 11 (discount), 12 (split conservation) and 13 (invoice print) were
each **deliberately broken and observed to fail** before being trusted — see
`tests/e2e-scenario/README.md` "Self-falsification". Invariant 9 stayed green
through the discount break, which is exactly why 11 had to exist.

The suite also fails on **absent data**, not only wrong data. `REQUIRED_SHAPES`
(`orchestrator/src/types.ts`) counts what a run actually produced and fails if
any count is zero. Last run: 24 discounts applied non-zero, 54 permission
refusals, 54 reason refusals, 21 multi-part splits, 54 bills enqueued, 54
printed. This closed a real hole — invariants 9/10 had passed 54/54 for three
tracks while every invoice carried a zero discount and no bill was ever queued.

---

## 2. M3 functional acceptance — what was and was not observed

**Read the scope line carefully before quoting this as acceptance.**

Verified on dev hardware (Windows 11, no printer) with the file-backed printer
transport active (`HOLLER_PRINTER_FILE_SINK_DIR`, `edge/printer/src/transport/file_sink.rs`).

Observed through the **real shipped command surface** (`*_impl` functions the
Tauri IPC layer calls), driven by the e2e harness, 54 scenarios:

| Flow | Verified where |
|---|---|
| Order → confirm → send to kitchen | `kot` rows; KOT print files in the sink dir |
| Per-line discount applied | persisted `invoice_line.discount_paise` non-zero; value checked against the contract formula computed independently |
| Discount refused (no permission) | `DISCOUNT_PERMISSION_DENIED`, and **zero `invoice` rows left behind** |
| Discount refused (no reason) | `DISCOUNT_REASON_REQUIRED`, same atomicity check |
| Split bill, N parts | `invoice` rows sharing one `split_group_id`, `split_index` 1..N, quantities reconstruct the order exactly |
| Split payment (CASH + UPI) | `payment` rows; over-settlement refused with `FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE` |
| Invoice enqueue + print | `print_job` read back **by id** from the spool, `status='PRINTED'`, at the `BILL`-role printer specifically |
| Rendered bill content | `.escpos` byte stream on disk, starting `1B 40` (ESC @), plus a `.txt` companion showing legal name, GSTIN, `TAX INVOICE`, `Bill: n of N`, `HSN/SAC 9963`, `Payment: Cash + UPI` |

Real figures produced by the engine for a worked example (2 × ₹40 chai + 1 ×
₹220 thali, 10% on the thali, split 2 ways):

```
part 1  taxable 8000   disc 0     CGST 200  SGST 200  round_off 0    total 8400
part 2  taxable 19800  disc 2200  CGST 495  SGST 495  round_off +10  total 20800
```

### CORRECTION (2026-08-20) — the business-day bucketing is wrong in shipped code

Filed during M4 planning. **This section previously claimed a correctness it does
not have.**

`business_date_from` (`apps/pos/src-tauri/src/commands/billing.rs:71`) and the
display-number reset (`edge/database/src/repo.rs`) bucket by **UTC calendar day**.
In IST the UTC day rolls at **05:30 local**, so for any outlet trading past
midnight, every invoice number and every day-end / cash-shift reconciliation
between local midnight and 05:30 is assigned to the **previous** business day.
CLAUDE.md states the business day may cross midnight; this code assumes it does
not.

It was carried as a "known limitation" in §5 below and in two source comments.
That framing was wrong — the consequence was never quantified in business units.
Full write-up in `docs/retro.md`, 2026-08-20.

Scope of the correction: none of §2's observed flows are invalidated — they ran
inside a single UTC day — but **M3 must not be recorded as having correct invoice
numbering or day-end reconciliation** until this closes. The fix is a
schema-level decision in contracts v0.5.0 (ADR-018 §9.2), in the M4 pre-track.

### CORRECTION 2 (2026-08-20) — PostgreSQL `payment` was never append-only

`postgres/0007_m3_billing.sql:286` carried the comment "APPEND-ONLY
(docs/spec/payments.md §Conflict policy)" and **nothing behind it**. The SQLite
side got real triggers at 0.4.5; PostgreSQL got the sentence. So the guarantee
ADR-016 leans on — a tender is corrected by an appended reversal, never a
mutation — was structural at the edge and prose in the cloud, which is the one
environment where an engineer has a psql prompt and "just fix the row" is a
keystroke away.

Fixed at contracts 0.5.0 (`postgres/0018_payment_append_only_triggers.sql`), and
a lint (`every_append_only_claim_has_a_trigger_behind_it`) now fails the build
on any table claimed APPEND-ONLY or IMMUTABLE without enforcement behind it. It
found two more on its first passing run — `audit_event` and `cash_movement` —
both filed in `docs/backlog-m2.md`.

**This is the second M3 defect found during M4 planning.** Fixing it does not
retire the fact that M3 was reported complete while carrying both this and the
UTC business-date bucketing. Both stayed invisible for the same reason, now
named in `docs/retro.md` (2026-08-20): a claim that nothing verifies.

### What this did NOT cover — do not overstate it

- **The manual GUI runbook was written but NOT executed.** A 10-step tickable
  checklist exists (published artifact; steps mirror the table above and add
  cash-shift close). Nobody has walked it through the POS window. The evidence
  above comes from the harness driving the same `*_impl` surface, not from a
  human operating the UI.
- **Cash-shift open/close was NOT part of the file-backed run.** The harness has
  no cash-shift op and records payments with `cash_shift_id: null`. It is
  covered by `apps/pos/src-tauri/tests/billing_flow.rs` only — proven by test,
  not observed in this run.
- **No real printer.** See §3.
- **Not the ADR-013 target machine.** See §3.
- Aggregator KOTs, expo screen, label printers and the waiter app remain out of
  scope (M2 exclusions, unchanged).

---

## 3. ADR-013 — STILL OPEN. Two hardware gates block true acceptance

**M3 is code-complete and functionally exercised. It is NOT acceptance-complete.
Do not mark it accepted until both of these clear. Neither can be closed by any
test, harness or emulation — both need physical hardware.**

**(a) Real thermal printer, ESC/POS verified on paper.** The file sink replaces
only the final write; everything upstream is the shipping path, so the bytes are
real, but nothing proves a printer accepts them. Untested: vendor ESC/POS
dialect differences, whether the 80mm layout fits **58mm** paper, the **cutter**,
**codepage** and non-ASCII glyphs, USB and Bluetooth-SPP timing, paper-out and
mid-print disconnect. The spool's retry/backoff has never met a device that
failed for a physical reason.

**(b) Bare 4GB Windows 10 target, for the resource envelope.** Nothing has ever
been built or run on the machine the product ships to. Untested: the installer
completing **offline** with embedded WebView2 + VC++ runtimes (`tauri.conf.json`
still has no `bundle.windows` section, so it would try to download the
bootstrapper and fail), memory headroom under WebView2 at 4GB, SQLite
open/decrypt latency on a spinning disk, cold start, and crash recovery after a
real power cut. Full checklist in `docs/backlog-m2.md` "Clean Windows 10 VM
validation".

---

## 4. M2 acceptance item 5 — was silently red, now re-evidenced

**Item 5 ("one real KDS↔edge socket session") had been failing since ADR-017 and
nobody knew.** `tests/integration/kds-lan-bridge` stopped compiling when
`server::start` gained a `DeviceTokenVerifier` and `MenuItem` gained
`tax_profile_id` / `hsn_sac`. The `lan-integration` CI job was therefore failing
at `cargo build` — not proving a socket session at all — while item 5 was
recorded as met.

Fixed this session: one Argon2id-hashed `device_credential_cache` row seeded, a
real `CachedCredentialVerifier` wired, and the token published on the bridge's
ready line so the driving test presents a genuine credential rather than
bypassing the check. **Re-verified: 4/4 tests pass** against a real socket
(`cd tests/integration/kds-lan && pnpm test`).

**Action for a future session:** M2 acceptance item 5 needs re-confirming
wherever M2 acceptance is tracked (CLAUDE.md's milestone block still lists it as
an open acceptance item). It is now genuinely evidenced; the record should say
so, and should note the period it was falsely green.

Guard added so a tenth break fails fast: **`rust-seams` CI job + `make
check-seams`** compiles every cross-workspace Rust consumer. The repo is
deliberately several cargo workspaces, so `cargo check` in the crate you edit
proves nothing about its callers — this had broken nine times.

---

## 5. Open defects a new session must not lose

Closed since the last resume: invoice enqueue path, split-bill unreachable,
per-line discounts unreachable, `devseed` seeds no printer.

**Mint-counter wrap: FIXED, not open.** `format_order_display_number`
(`edge/database/src/repo.rs`) uses bijective base-26 blocks, so the formatter
never repeats for any index up to `i64::MAX`, plus a per-business-day counter
reset. Regression test `formatter_never_repeats_past_the_old_wrap_point` drives
past the old collision point (25975, where `#Z999` rolled to `#A1`).

**Contracts are FROZEN at v0.4.7** (was 0.4.6 in the last resume; CLAUDE.md now
says 0.4.7 with migrations through 0012). 0.4.7 added `printer_role` as a join
table rather than a column on `printer`, so nothing that compiled stopped
compiling. A printer with no role row is a candidate for **neither** path —
absence is never read as consent.

| Where | What |
|---|---|
| `backend/internal/{auth,menu,tables}` | Never call `postgres.Migrate`; they seed into an assumed schema. CI works around it with a `go run ./cmd/devseed` step that also injects dev fixture data into the test database. Real fix: have them migrate. |
| `packages/contracts/openapi/openapi.yaml` | **Nothing machine-checks it** against handlers or TS/Go types. Drift check is TS↔Go only. It silently drifted on three `MenuItem` fields for two versions. |
| `edge/database/src/lib.rs` | `Db::connection()` is plain `pub`; three sibling crates hold it. `payment` is trigger-protected (0.4.5); `cash_shift` is not and cannot be — OPEN→CLOSED is a legitimate UPDATE. |
| `apps/pos/src/store/cashShift.ts` | Shift recovery runs on `BillingScreen` mount, not app-global startup. |
| `edge/database` (`payment_allocation`) | Assumes one payment settles at most one invoice. A tender spanning two invoices is unmodelled. |
| `edge/database` | `PAID_IN`/`PAID_OUT` emit no outbox event; they ride inside `CashShiftOpened`/`Closed`. Visibility latency, not a money defect. |
| `edge/sync/src/config.rs` | Empty `device_credentials` is not an error, unlike empty `users` — "none enrolled" and "cloud forgot" are indistinguishable. |
| `edge/database/src/invoice/numbering.rs` | `{OUTLET}` token derives from `outlet.name`; no `outlet.code` in the frozen contract. |
| `edge/database/src/repo.rs` | Display-number reset buckets by **UTC** day, not outlet-local business day. Same limitation in `business_date_from` (`commands/billing.rs`). |
| config authoring | A non-`NEVER` `reset_policy` whose prefix lacks the matching date token yields duplicate invoice numbers across periods. Caught by the UNIQUE index; not validated at config-write time. |
| `backend/internal/compliance` | Writes gate on `outlet.manage`; **no `billing.manage`** exists in the frozen `Permission` enum. Whoever may rename a table may set the GSTIN printed on every invoice. |
| `apps/pos/src-tauri/tests` | `cargo fmt --check` reports pre-existing diffs; not CI-enforced (crate is not built on Linux, ADR-013). Same for the two test bridges under `tests/`. |
| `tests/e2e-scenario` | The HSN check is folded into `9_tax_reconciliation`, so a tax arithmetic error and a missing compliance field share one invariant id. |
| `apps/pos` | **No dev-server smoke test.** Filed in `docs/backlog-m2.md` with the constraint that matters: it must drive `pnpm dev`, not the build — `optimizeDeps` is dev-server-only and `vite build` never reads it. |
| `tests/e2e-scenario` | `cancel_kitchen_items_with_outbox` still has no Tauri command; `#132-C` cancellation is unreachable from the shipped surface. |

### Operational gate — read before any rollout

`menu_item.hsn_sac` is NULL on every row of every existing edge database, and
the edge **rejects invoice issuance** when any line's code is NULL or blank.
**No outlet can issue any invoice until its catalogue is configured.** That is
correct and deliberate (no fallback: a wrong code that looks configured is worse
than a missing one), but catalogue configuration must be part of any rollout.

Same shape now applies to printing: an outlet with **no `BILL`-role printer**
cannot print a bill, and `print_invoice` fails loudly by name rather than
queueing into nothing.

---

## 6. Process notes

Carried forward, all still binding:

- **A contract change is a multi-crate change.** Enumerate consumers, build
  them, list them in the ADR. Run `make check-seams`.
- **Build-green ≠ dev-works for the Tauri/web apps** (new in CLAUDE.md's coding
  rules). Two incidents now: the KDS detached-global (browser-only) and the POS
  white screen (dev-server-only, a stale `node_modules/.vite` prebundle). The
  build output, the dev server and the browser are three runtimes; a failure in
  one is invisible from the others. Say which runtime a frontend change was
  observed in. First move on a blank Tauri window: check `node_modules/.vite`
  mtime against its source, and check the Network tab, not only the console.
- **An invariant nobody has watched fail is not a gate**, and a green invariant
  whose subject never occurred is worse than no invariant. Count the shapes.
- **Verify the runner, not the file.** A migration on disk but absent from
  `MIGRATIONS` never applies (0009–0011 sat dead; 0005 before them).
- **Never quote a number without the command and the date.**

Docker is not started automatically after a restart:
`docker compose up -d postgres redis nats`.

---

## 7. Repo hygiene

Untracked and **not created by any Holler track** — decide whether they are
wanted: `.github/copilot-instructions.md`, `.github/instructions/`,
`HOLLER_DEV_MENU_SPEC.md`, `imgs/og.png`, `holler-website-v8.html`, and a set of
`website/holler-website-v*.html` plus `website/holler-site/`.

Prune: five `worktree-agent-*`, `wip/edge-database-stash`, and
`wip/t13-retry-partial` (`ca6c44a`, does not build — T13 was redone from
scratch; the branch is dead).

Dev conveniences added this session, both opt-in and off by default:
`scripts/dev-bootstrap.ps1 -WithBilling` (seeds tax/fiscal/series/discounts/
printers — required before the POS can issue any bill) and
`-PrinterFileSinkDir <dir>` (routes prints to files).
