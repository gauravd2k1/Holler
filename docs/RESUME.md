# M3 resume state — 2026-08-15

`main` is committed, clean and green at `4a1a37b`. Nothing is in flight.

**`origin/main` is very likely RED, and pushing the remaining commits is what
fixes it.** `origin/main` sits at `6ddb70b` — someone pushed mid-session, and
that tip is the worst possible cut point: it **includes** the CI strictness
(`8c64893`: a real Postgres service and a zero-skip assertion) and **excludes**
every fix that lets the pipeline satisfy it. On that commit the backend job
races on concurrent migration, `internal/{auth,menu,tables}` fail against an
empty schema, `edge/sync` does not compile, and all four edge crates fail
`cargo fmt --check`.

The 11 unpushed commits are precisely the repairs. Push them.

Read with `docs/adr/ADR-016-m3-billing-contracts.md` (0.4.4 + 0.4.5 addenda),
`docs/adr/ADR-017-device-enrollment-credential.md` (0.4.5 addendum), and **both
2026-08-14 and 2026-08-15 entries in `docs/retro.md`** — the rules in those
entries bind future tracks, especially the contract-change rules.

---

## Verified green at `6575c9f`

Every number below was produced by running the command named, on this machine,
at this commit.

| Suite | Result | Command |
|---|---|---|
| `backend` | **287**, 0 skips, 0 failures | `HOLLER_TEST_DATABASE_URL=... go test -count=1 ./...` |
| `edge/database` | **175** | `cargo test` |
| `edge/printer` | **31** | `cargo test` |
| `edge/sync` | **28** | `cargo test` |
| `edge/device` | **11** | `cargo test` |
| `apps/pos` | **137** | `pnpm test` |
| `apps/pos/src-tauri` | **49** | `cargo test` |
| `apps/kds` | **30** | `pnpm test` |
| `packages/contracts` TS / Go | **39** / ok | `pnpm test`, `go test` |
| **e2e harness** | **54 scenarios, 10/10 invariants, 0 fatals** | `cd tests/e2e-scenario/orchestrator && pnpm test` (~131s) |

All four edge crates are `cargo fmt --check` clean. Harness per-invariant:
`1` 54/54 · `2` 20/20 · `3` 20/20 · `4` 34/34 · `5_money` 54/54 · `6` 4/4 ·
`7` 54/54 · `8` 17/17 · `9_tax_reconciliation` 54/54 ·
`10_payment_settlement` 54/54.

### How a green number here has lied — three times now

1. **Unset `HOLLER_TEST_DATABASE_URL` skipped 29 backend tests** while reporting
   green. Fixed: `backend/internal/platform/testdb` `t.Fatal`s by default;
   `HOLLER_SKIP_PG_TESTS=1` is a deliberate opt-in. CI also asserts skips == 0.
2. **The e2e harness did not compile** for two features while
   "54 scenarios, 0 violations" was quoted as evidence for weeks.
3. **Three contract migrations were never registered** and were inert while
   their SQL had been "verified" by hand-application to a scratch database.

Always run the thing. Never quote a number without the command and date.

Docker is not started automatically after a restart:
`docker compose up -d postgres redis nats`.

---

## Contracts: FROZEN at v0.4.6

- **0.4.4** — the eight compliance config write routes documented in OpenAPI
  (additive, paths only). It was eight, not the six two documents recorded.
- **0.4.5** — four items: per-row `config_version` on `device_credential`;
  `payment` append-only triggers; `print_job.invoice_id` with an exactly-one
  CHECK; `menu_item.hsn_sac`. See the ADR-016/017 addenda.
- **0.4.6** — OpenAPI `MenuItem` had drifted on **three** fields
  (`tax_profile_id` since 0.4.2, `hsn_sac` since 0.4.5, `schema_version` since
  the type was written). Found by a build fix, not a check.

**Read the 2026-08-15 retro entry before making the next contract change.** One
nullable column in 0.4.5 broke five consumers, and its migration was inert. The
binding rule: **a contract change is a multi-crate change** — enumerate the
consumers, build them, and list them in the ADR.

---

## Landed this session (all gated unless noted)

| Commit | What | Gate |
|---|---|---|
| `945b1a7` | **T7c** append-only payments, cash shift, §39 variance reason | PASS |
| `a90e225` | contracts **0.4.4** | orchestrator |
| `eef7464` | **T13 retry** credential/bump atomicity + outlet-scoping guard | PASS |
| `4c5b045` | **T10** GST invoice render template | PASS (renderer only) |
| `fcc3c95` | **T9** POS billing UI | **FAIL** → `dc1c5be` |
| `460b957` | **T11** §66 properties, real `pgx.Tx` rollback, fail-loud DB gate | PASS |
| `8c64893` | CI: Postgres service + zero-skip assertion | orchestrator |
| `dc1c5be` | **T9 retry** edge-side over-settlement guard, cash-shift recovery | PASS |
| `2ed7fdb` | **T11b** harness money invariants | **FAIL** → `89587a8` |
| `89587a8` | **T11b retry** harness builds and runs on real `main` | verified directly |
| `39159f0`, `6ddb70b` | docs + retro | — |
| `02be0e9` | contracts **0.4.5** | orchestrator |
| `27af892` | backend adopts per-row `config_version` | PASS |
| `4a20321` | `postgres.Migrate` advisory lock + CI migrate step | verified directly |
| `9781400` | **HSN/SAC** resolution, issue-time guard, harness assertion | PASS |
| `cf81fe8` | `edge/sync` carries `tax_profile_id`/`hsn_sac` | verified directly |
| `e798c7b` | contracts **0.4.6** | orchestrator |
| `6ff663c` | `cargo fmt` all four edge crates (218 diffs) | mechanical |
| `c65600b` | `edge/printer` partial-index `ON CONFLICT`; `edge/device` build | verified directly |
| `92f3511` | migration-registration test; split-bill HSN test | verified directly |
| `6575c9f` | `apps/pos/src-tauri` fixtures carry `hsn_sac` | verified directly |

---

## CI was decorative. Five things break it, and four are still unpushed

Invisible for months because the backend job had no database and the edge crates
were never run. `8c64893` made the job honest — and is itself already pushed, at
`6ddb70b`, **ahead of its own repairs**. Items 2–5 below are fixed only in the
unpushed commits.

1. **No Postgres service** — the backend job had never once run an integration
   test. Fixed (`8c64893`).
2. **Concurrent-migration race** — `Migrate`'s check-and-apply was not atomic;
   parallel packages collided on an empty database. Fixed with a
   `pg_advisory_xact_lock` (`4a20321`).
3. **Three packages never migrate** — `internal/auth`, `internal/menu`,
   `internal/tables` seed straight into a schema they assume exists. Worked
   around with a CI `migrate` step; **the real fix is those packages calling
   `Migrate` themselves** (open item below).
4. **`edge/sync` did not compile** — broken since 0.4.2. Fixed (`cf81fe8`).
5. **218 `cargo fmt` diffs** across the four edge crates — the `edge` job would
   have failed on its first step. Fixed (`6ff663c`).

---

## Open defects, with locations

| Where | What |
|---|---|
| `edge/printer/src/spool.rs` | **No invoice enqueue path.** `print_job.invoice_id` exists (0.4.5) and the CHECK is satisfied, but nothing inserts an invoice job. T10's renderer still has **zero callers** — a rendered invoice cannot reach a printer. Needs `enqueue_invoice_job` with `ON CONFLICT (invoice_id, printer_id) WHERE invoice_id IS NOT NULL` plus a lookup. **This is what blocks "a real slip prints the short number".** |
| `backend/internal/{auth,menu,tables}` | These never call `postgres.Migrate`; they seed into an assumed schema. CI works around it with a `go run ./cmd/devseed` step, which also seeds *development fixture data* into the test database — a coupling worth removing. Real fix: have them migrate. |
| `packages/contracts/openapi/openapi.yaml` | **Nothing machine-checks this against the handlers or the TS/Go types.** The drift check covers TS↔Go only; no test parses the file. It silently drifted on three `MenuItem` fields for two versions. A generator or spec-vs-types test is unwritten. |
| `edge/database/src/lib.rs` | `Db::connection()` is plain `pub` returning `&Connection`; three sibling crates hold it. Doc comment corrected, but visibility unchanged. `payment` is now protected by triggers (0.4.5); `cash_shift` is not, and cannot be — its OPEN→CLOSED transition is a legitimate UPDATE. |
| `apps/pos/src-tauri/src/commands/billing.rs` | **Split-bill invoicing unreachable** — `issue_split_invoices_with_outbox` exists but is excluded from the M3 command surface; `issue_invoice` always bills the whole order at `split_count == 1`. The "split parts sum to the whole" invariant cannot be exercised end to end. |
| `apps/pos/src-tauri/src/commands/billing.rs` | **Per-line discounts unreachable** — `build_invoice_lines` hard-codes `discount_per_unit_paise: 0`. The discount invariant cannot be exercised. |
| `apps/pos/src/store/cashShift.ts` | Shift recovery runs on `BillingScreen` mount, not app-global startup. Deferred, not lost. |
| `edge/database` (`payment_allocation`) | Assumes one payment settles at most one invoice. Correct for every wired path; a tender spanning two invoices is unmodelled. |
| `edge/database` | `PAID_IN`/`PAID_OUT` emit no outbox event; they travel only inside `CashShiftOpened`/`CashShiftClosed`. Visibility latency, not a money defect. |
| `edge/sync/src/config.rs` | Empty `device_credentials` is not an error, unlike empty `users` — "none enrolled" and "cloud forgot" are indistinguishable to the edge. |
| `edge/database/src/invoice/numbering.rs` | `{OUTLET}` token derives from `outlet.name`; no `outlet.code` in the frozen contract. |
| `edge/database/src/repo.rs` | Display-number reset buckets by **UTC** day, not outlet-local business day. |
| config authoring | A non-`NEVER` `reset_policy` with a prefix lacking the matching date token yields duplicate invoice numbers across periods. Caught loudly by the UNIQUE index; not validated at config-write time. |
| `backend/internal/compliance` | Writes use `outlet.manage`; **no `billing.manage`** exists in the frozen `Permission` enum. POS billing gates on `order.modify`/`order.void`. Whoever may rename a table may set the GSTIN printed on every invoice. |
| `apps/pos/src-tauri/tests` | `cargo fmt --check` reports pre-existing diffs. Not CI-enforced (the crate is deliberately not built on a Linux runner, ADR-013). |
| `tests/e2e-scenario` | The HSN check is folded into `9_tax_reconciliation` rather than its own invariant, so a tax arithmetic error and a missing compliance field share one ID. Details are concatenated, not masked — but the pass/fail count cannot distinguish them. |

---

## Operational consequence of the HSN/SAC guard — read before any rollout

`menu_item.hsn_sac` is NULL on every row of every existing edge database (the
migration was previously inert, so nothing was ever populated). The edge now
**rejects invoice issuance** when any line's code is NULL or blank, naming the
offending items.

So **no outlet can issue any invoice until its catalogue is configured.** That
is correct — an outlet cannot legally bill without HSN/SAC codes — and no
fallback was added deliberately: a wrong code that looks configured is worse
than a missing one. But it is a hard gate, and catalogue configuration must be
part of any rollout or upgrade plan.

---

## Not started

- **Invoice enqueue path** (top of the defect table). Until it lands, M3 cannot
  claim the UUID-on-slip defect closed for invoices: the renderer is verified
  and has no caller.
- **Split-bill invoicing** and **per-line discounts** on the POS command
  surface — edge capability exists for the first.
- **ADR-013: nothing has ever been built or run on bare Windows 10.** Must
  clear before shipping. Also covers USB/Bluetooth printing.
- **Latency re-measure at an outlet.** M2 measured 150–183ms against a 250ms
  target over real WiFi; headroom is ~30%, not an order of magnitude.
- `devseed` seeds no printer, so the print path is unexercised in development —
  the harness logs `no active printer routed for station ...` every run.

---

## Process notes for whoever resumes

Both retro entries have the reasoning. In short:

- **A contract change is a multi-crate change.** Enumerate consumers (`grep` the
  type name across the workspace), build them, list them in the ADR. One
  nullable column broke five.
- **Verify the runner, not the file.** Inspect a database built by the
  application's own migration path.
- **Changing a constraint means opening its consumers.** `ON CONFLICT` must
  match a partial index's predicate, and no test in the owning crate says so.
- **Partition parallel tracks by interface, not only by directory.** `tests/`
  and `apps/pos` were cleanly separated by directory, but the harness *calls*
  `apps/pos`.
- **A track must verify against the tree it commits to.**
- **Ask verifiers to falsify a property the builder did not target.** Every gate
  that found something real did this.
- **An invariant or test nobody has watched fail is not a gate.**

## Repo hygiene

Untracked, **not created by any Holler track**:
`.github/copilot-instructions.md`, `.github/instructions/`,
`HOLLER_DEV_MENU_SPEC.md`, and something under `website/`. Flagged by several
gates. Decide whether they are wanted.

Prune: five `worktree-agent-*`, `wip/edge-database-stash`, and
`wip/t13-retry-partial` (`ca6c44a`, does not build — T13 was redone from
scratch, this branch is dead).
