# M3 resume state — 2026-08-15

`main` is committed, clean and green at `89587a8`. Nothing is in flight.
**Nothing is pushed** — `main` is 21 commits ahead of `origin/main`, so no CI
run has executed against any of this session's work.

Read with `docs/adr/ADR-016-m3-billing-contracts.md` (incl. the 0.4.4
addendum), `docs/adr/ADR-017-device-enrollment-credential.md`, and the
**2026-08-14 entry in `docs/retro.md`** — that entry is the most important
output of the session and the rules in it bind future tracks.

---

## Verified green at `89587a8`

| Suite | Result | How |
|---|---|---|
| `backend` | **285**, 12 packages, **0 skips** | `HOLLER_TEST_DATABASE_URL=... go test -count=1 ./...`, native Windows + Docker Postgres |
| `edge/database` | **170** | `cargo test`, clippy clean |
| `edge/printer` | **30** | `cargo test`, clippy clean |
| `apps/pos` | **137** | vitest/jsdom |
| `apps/pos/src-tauri` | **49** | `cargo test` |
| `apps/kds` | 30 | vitest |
| `packages/contracts` TS / Go | 39 / ok | vitest, `go test` |
| **e2e harness** | **54 scenarios, 0 fatals, 10/10 invariants passing** | `cd tests/e2e-scenario/orchestrator && pnpm test`, ~78s, verified directly 2026-08-15 |

Harness per-invariant: `1_state_machine` 54/54 · `2_kot_conservation` 20/20 ·
`3_kds_fidelity` 20/20 · `4_no_station_explicit` 34/34 · `5_money` 54/54 ·
`6_durability` 4/4 · `7_outbox` 54/54 · `8_status_echo` 17/17 ·
`9_tax_reconciliation` 54/54 · `10_payment_settlement` 54/54.

### Two ways a green number here has lied before — read before trusting one

1. **`HOLLER_TEST_DATABASE_URL` unset used to skip 29 backend tests silently
   while reporting green.** Fixed: `backend/internal/platform/testdb` now
   `t.Fatal`s by default; `HOLLER_SKIP_PG_TESTS=1` is a deliberate opt-in. CI
   now also asserts the skip count is zero.
2. **The e2e harness did not compile for two features**, and "54 scenarios, 0
   invariant violations" was quoted for weeks as evidence. A harness that
   cannot start reports zero violations. Always run it; never quote the number.

Docker is not started automatically after a restart:
`docker compose up -d postgres redis nats`.

---

## Landed this session (all gated)

| Commit | Track | Gate |
|---|---|---|
| `945b1a7` | **T7c** append-only payments + cash shift, §39 variance reason | PASS |
| `a90e225` | **contracts 0.4.4** — eight compliance write routes in OpenAPI | orchestrator |
| `eef7464` | **T13 retry** credential write + `config_version` bump atomic; outlet-scoping guard | PASS |
| `4c5b045` | **T10** GST invoice render template | PASS — renderer only, see below |
| `fcc3c95` | **T9** POS billing UI | **FAIL** → fixed by `dc1c5be` |
| `460b957` | **T11** §66 property suite, real `pgx.Tx` rollback test, fail-loud DB gate | PASS |
| `8c64893` | **CI** Postgres service + zero-skip assertion | orchestrator |
| `dc1c5be` | **T9 retry** edge-side over-settlement guard, cash-shift recovery | PASS |
| `2ed7fdb` | **T11b** harness money invariants (invariants 9 & 10) | **FAIL** → fixed by `89587a8` |
| `89587a8` | **T11b retry** adopt `invoice_id`, run on real `main`, falsify invariant 10 | verified directly |

### What the gates actually proved
- **T13:** the verifier reproduced the mutation that defeated the *previous*
  T13 gate — dropping `dc.outlet_id` from `ListEdgeCredentials` — in a scratch
  copy against real Postgres, and watched it go red, leaking outlet B's
  Argon2id hash to an outlet-A pull. The guard is real.
- **T7c:** the gate's leading hypothesis (that `expected_cash_paise` excluded
  cash-tender sales, firing §39's mandatory-reason rule on a phantom variance
  every shift) was traced and **disproven**. A disproven hypothesis is a
  successful gate.
- **T9:** found a live **double-settlement** hole. `isFullySettled` existed and
  was unit-tested but was never imported into `BillingScreen`; edge-side
  `validate_forward` checked only `amount > 0`. Nothing at either layer capped
  tenders. Fixed at the edge (not the UI) in the retry, which required wiring
  `payment_allocation` — present in the schema since 0.4.0 and entirely unused.
- **T9 retry:** the feared regression (that gating forward tenders would also
  block **refunds** on a settled bill) was checked and is absent —
  `BillingScreen.tsx:398` gates Void/Refund on `canVoid`, not on settlement.
- **T11b retry:** invariant 10 falsified deliberately — 12/12 scenarios red,
  each naming itself with exact paise — then reverted to green.

---

## Open defects, with locations

| Where | What |
|---|---|
| `edge/database/src/invoice/assemble.rs:226` | **`hsn_sac` is hard-coded `None`.** Every invoice line prints with no HSN/SAC code, which a GST tax invoice legally requires. **Compliance defect. Needs a track.** Found only once something rendered an invoice. |
| `packages/contracts` (`print_job`) | `print_job.kot_id` is a `NOT NULL` FK to `kot(id)` — there is **no way for an invoice to become a print job**, so T10's renderer has zero callers and no invoice can reach a printer. Proposed as 0.4.5 item 3; needs a shape decision. |
| `apps/pos/src-tauri/src/commands/billing.rs` | Split-bill invoicing unreachable: `issue_split_invoices_with_outbox` exists in `edge/database` but is excluded from the M3 command surface, so `issue_invoice` always bills the whole order at `split_count == 1`. The "split bill parts sum to the whole" invariant cannot be exercised. |
| `apps/pos/src-tauri/src/commands/billing.rs` | Per-line discounts unreachable: `build_invoice_lines` hard-codes `discount_per_unit_paise: 0`. No command lets a cashier apply one, so the discount invariant cannot be exercised. |
| `edge/database/src/lib.rs:180` | `Db::connection()` is plain `pub` returning `&Connection` (`execute` takes `&self`), used by `edge/device`, `edge/printer`, `edge/sync` **in production code**. Its doc comment claims it is not exposed beyond the crate — **false**. Append-only payments are discipline, not structure. Narrowing it is not cheap; the structural fix is SQLite triggers (0.4.5 item 2). |
| `backend/internal/outlet/device_service_test.go` | Atomicity is covered by a real `pgx.Tx` rollback test as of T11 — **this entry is now closed**, retained only to record that fake-only coverage was the original state. |
| `packages/contracts/postgres/0008_device_enrollment.sql` | `device_credential` has no per-row `config_version`; `/sync/config` filtering stays outlet-granular. Proposed as 0.4.5 item 1. |
| `apps/pos/src/store/cashShift.ts` | Shift recovery runs on `BillingScreen` **mount**, not app-global startup. A cashier who restarts and never opens Billing won't see recovery until they do. Deferred, not lost. |
| `edge/database` (`payment_allocation`) | Assumes one payment settles at most one invoice. Correct for every wired path; a tender spanning two invoices is unmodelled. |
| `edge/database` | `PAID_IN`/`PAID_OUT` emit no outbox event — they travel only inside `CashShiftOpened`/`CashShiftClosed`. A paid-out on a long-open shift is invisible to cloud reporting until close. Visibility latency, not a money defect. |
| `edge/sync/src/config.rs` | Empty `device_credentials` is not an error, unlike empty `users` — "none enrolled" and "cloud forgot" are indistinguishable to the edge. |
| `edge/database/src/invoice/numbering.rs` | `{OUTLET}` token derives from `outlet.name`; no `outlet.code` in the frozen contract. |
| `edge/database/src/repo.rs` | Display-number reset buckets by **UTC** day, not outlet-local business day. |
| config authoring | A non-`NEVER` `reset_policy` with a prefix lacking the matching date token yields duplicate invoice numbers across periods. Caught loudly by the UNIQUE index; not validated at config-write time. Documented in the OpenAPI `prefix_template` description. |
| `backend/internal/compliance` | Writes use `outlet.manage`; **no `billing.manage` exists** in the frozen `Permission` enum. Whoever may rename a table may set the GSTIN printed on every invoice. POS billing likewise gates on `order.modify` / `order.void`. |
| `packages/contracts/openapi/openapi.yaml` | **Nothing machine-checks this document against the handlers.** Drift check covers TS↔Go only. Route parity was hand-verified at 0.4.4 (8 = 8) — not a standing gate. |

**Corrected this session:** the entry claiming `invoice_line.description`/`hsn_sac`
*"read the current `menu_item`, so a renamed item changes a reprinted invoice"*
was **wrong about reprints**. `assemble.rs` reads live config only at issue time
and **stores** the result on `invoice_line`; reprints select stored columns with
no join to `menu_item`. Reprints are safe. Two invoices issued at different
times for the same item legitimately differ — expected, not a defect.

---

## Awaiting a decision — contracts 0.4.5

| # | Change | Kind | State |
|---|---|---|---|
| 1 | Per-row `config_version` on `device_credential` | Postgres additive + backfill | written, self-reviewed, **needs go** |
| 2 | `BEFORE UPDATE`/`BEFORE DELETE` triggers on `payment` | SQLite additive | written, self-reviewed, **needs go** |
| 3 | `print_job` gains an invoice reference | Schema — makes `kot_id` nullable | **needs a shape decision** |

Item 1 is narrower than it looks: `EdgeDeviceCredential` already declares
`config_version`, so the **wire type does not change** and the TS/Go drift tests
are untouched. It does require inverting the write order inside the transaction
(bump first, then insert with the returned version) and stamping the new version
on revoke — safe only because T13 put all three methods in one transaction.

Item 2 covers `payment` only. `cash_shift` is legitimately mutable
(`close_cash_shift_in_tx` does the OPEN→CLOSED transition), so a blanket
no-update trigger would break a correct path.

Item 3 options: a nullable `invoice_id` with a CHECK that exactly one of
`kot_id`/`invoice_id` is set (one spool, one retry path — recommended), or a
separate `invoice_print_job` table (cleaner authority, duplicated spool).

---

## Not started

- **`hsn_sac`** — GST compliance defect above. Highest-value unstarted item.
- **Split-bill invoicing** and **per-line discounts** — edge capability exists
  for the first, neither is reachable from the POS command surface.
- **ADR-013: nothing has ever been built or run on bare Windows 10.** Must
  clear before shipping. Now also covers USB/Bluetooth printing.
- **Latency re-measure at an outlet.** M2 measured 150–183ms against a 250ms
  target over real WiFi; headroom is ~30%, not an order of magnitude.
- `devseed` seeds no printer, so the print path is unexercised in development —
  the harness logs `no active printer routed for station ...` on every run.

---

## Process notes for whoever resumes

The 2026-08-14 retro entry has the full reasoning. In short:

- **Partition parallel tracks by interface, not only by directory.** This
  session's one process failure: `tests/` and `apps/pos` were cleanly separated
  by directory, but the harness *calls* `apps/pos`, so a signature change broke
  it and T11b's own commit did not build.
- **A track must verify against the tree it commits to**, never a worktree
  taken before a sibling merge.
- **Ask verifiers to falsify a property the builder did not target.** Every
  gate that found something real did this; the two that found nothing still
  disproved a specific hypothesis, which is also a result.
- **Commit before the gate.** Every track did this session.
- **An invariant or test nobody has watched fail is not a gate.**

## Repo hygiene

- Untracked, **not created by any Holler track**: `.github/copilot-instructions.md`,
  `.github/instructions/`. Flagged by four separate gates. Decide whether wanted.
- Prune: five `worktree-agent-*`, `wip/edge-database-stash`, and
  `wip/t13-retry-partial` (`ca6c44a`, does not build — T13 was redone from
  scratch, this branch is dead).
