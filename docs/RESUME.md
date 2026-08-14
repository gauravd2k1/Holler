# M3 resume state — 2026-08-14 (session 2)

`main` is committed, clean and green at `eef7464`. Nothing is in flight.

Read this with `docs/adr/ADR-016-m3-billing-contracts.md` (including the 0.4.4
addendum) and `docs/adr/ADR-017-device-enrollment-credential.md` — the binding
conditions live there, not here.

---

## Verified green at `eef7464`

All re-executed this session on a cold machine after a restart, environments
named:

| Suite | Result |
|---|---|
| `backend` | **281 tests**, 12 packages, `go test -count=1 ./...`, native Windows against Docker Postgres, **zero skips** |
| `edge/database` | **163** (150 lib + 13 integration), `cargo test`, native Windows; clippy clean |
| `apps/pos` | **114**, vitest/jsdom |
| `apps/pos/src-tauri` | **45**, `cargo test` |
| `apps/kds` | **30**, vitest |
| `packages/contracts` TS | **39**, vitest/Node |
| `packages/contracts` Go | ok, `go test -count=1` |

Not re-run this session: the e2e harness (54 scenarios).

### The green above is conditional — read this before trusting a future run

`HOLLER_TEST_DATABASE_URL` is **not set in this environment**. A bare
`go test ./...` passes all 12 packages while **silently skipping 29
Postgres-backed tests**, including `TestBuildRouter_SyncConfigEndToEnd` — the
exact test M2 acceptance item 4 names. Every backend run must be:

```
cd backend && HOLLER_TEST_DATABASE_URL="postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable" go test -count=1 ./...
```

and must be confirmed with `-v ./... | grep -c -- "--- SKIP"` → `0`. **A green
backend suite with a nonzero skip count is not a pass.** This has now been the
shape of two separate M2 acceptance failures; making the suite fail loudly on an
unset URL is unclaimed work.

Docker is not started automatically after a machine restart. Bring the cloud
stack up with `docker compose up -d postgres redis nats` before any backend run.

---

## Passed their gate this session

Both verified by a read-only gate that independently falsified a property the
builder had **not** targeted.

| Track | Delivered | Commit |
|---|---|---|
| **T7c** | Append-only payments (reversal via `reverses_payment_id`, non-positive amount) + cash shift with §39 mandatory variance reason | `945b1a7` |
| **T13 retry** | Credential write and `config_version` bump made atomic under `WithTx`; outlet-scoping guard on the credential-hash endpoint | `eef7464` |

**Contracts 0.4.3 → 0.4.4** (orchestrator-serialized): the eight compliance
config write routes documented in OpenAPI. Additive, paths only — no schema, no
aggregate, no handler touched. See the ADR-016 0.4.4 addendum.

### What the gates actually proved

- **T13's outlet-scoping guard is real.** The verifier reproduced the exact
  mutation that defeated the *previous* T13 gate — dropping `dc.outlet_id` from
  `ListEdgeCredentials` while keeping only `tenant_id` — in a scratch copy
  outside the repo, and ran it against real Postgres. It went RED, returning
  outlet B's `device_id` and Argon2id hash to an outlet-A pull. The test uses
  two outlets under **one** tenant and asserts the *absence* of the other
  outlet's credential, not merely the presence of its own.
- **T7c's expected-cash derivation is sound.** The gate's primary hypothesis —
  that `expected_cash_paise` excluded cash-tender sales, which would fire §39's
  mandatory-reason rule on a phantom variance every shift — was traced and
  **disproven**. `cash_movement_for` posts `CASH_SALE`/`CASH_REFUND` rows for
  every cash tender on an open shift and the sum covers all movement kinds.
- Grepped every `r.pool` call in `device_postgres.go`: none falls inside the
  three transaction-scoped methods, so no stray pool call defeats the
  transaction. `WithTx` mirrors `compliance`'s implementation.

---

## Open defects, with locations

New this session at the top; the rest carried forward.

| Where | What |
|---|---|
| `edge/database/src/lib.rs:180` | **`Db::connection()` is plain `pub`** and returns `&rusqlite::Connection`, whose `execute` takes `&self` — so any sibling crate (device, printer, sync) can issue a raw `UPDATE payment`. Its doc comment claims it is "not exposed as `pub` beyond this crate's modules", which is **false**; `lib.rs:2019` already uses the pattern against `"order"`. Append-only payments are therefore **discipline, not structure**, and the comment asserts a guarantee the code does not provide. Pre-existing and crate-wide, not introduced by T7c. Fix the comment first — a future builder will trust it. |
| `backend/internal/outlet/device_service_test.go` | Atomicity is tested **only against `fakeDeviceRepo`'s hand-written snapshot/restore `WithTx`** — a fake whose rollback semantics the same builder wrote. No test exercises a real `pgx.Tx` rollback. Consistent with `compliance`'s own suite, so not a regression, but the property is unproven against real Postgres. This is the "tested only against its own fakes" shape behind M2 acceptance item 5. |
| `edge/database` | `payment_allocation` (payment↔invoice settlement) is unimplemented. `payment` ties directly to `order_id`; a split-bill settlement track will need it. Disclosed by T7c. |
| `edge/database` | `PAID_IN`/`PAID_OUT` movements emit no outbox event — they travel only inside `CashShiftOpened`/`CashShiftClosed`. A paid-out on a shift that stays open across a sync window is invisible to cloud reporting until close. Judged a visibility latency, not a money defect (`cash_shift` is edge-authoritative per ADR-016). |
| `packages/contracts/postgres/0008_device_enrollment.sql` | `device_credential` has no per-row `config_version`, so `/sync/config` filtering is outlet-granular. **Proposed as 0.4.5, awaiting approval — see below.** |
| `edge/sync/src/config.rs` | Empty `device_credentials` is not an error, unlike empty `users` — "no devices enrolled" and "cloud forgot to send them" are indistinguishable to the edge |
| `edge/database/src/invoice/assemble.rs` | `invoice_line.description` / `hsn_sac` read the **current** `menu_item` at issue time; `order_item` carries no name snapshot. A renamed item changes a reprinted invoice — a §31 reproducibility concern, disclosed by T7b |
| `edge/database/src/invoice/numbering.rs` | `{OUTLET}` token derived from `outlet.name`; no `outlet.code` column in the frozen contract |
| `edge/database/src/repo.rs` | Display-number reset buckets by **UTC** calendar day, not outlet-local business day |
| config authoring | A non-`NEVER` `reset_policy` with a prefix lacking the matching date token yields duplicate invoice numbers across periods. Caught by the UNIQUE index (fails loudly), not validated at config-write time. Now documented in the OpenAPI `prefix_template` description; validation still unwritten. |
| `backend/internal/compliance` | Writes use `outlet.manage`; **no `billing.manage` permission exists** in the frozen `Permission` enum. Whoever may rename a table may also set the GSTIN that prints on every invoice. Now documented in the OpenAPI spec text; splitting the permission is a semantic change needing its own ADR. |
| `packages/contracts/openapi/openapi.yaml` | **Nothing machine-checks this document against the handlers.** The CI contract-drift check covers TS↔Go types only; no test parses the OpenAPI file. Route parity was verified by hand at 0.4.4 (8 = 8, both directions) but that is not a standing gate — the spec can drift tomorrow and nothing goes red. A generator or a spec-vs-router test is the fix. |

---

## Proposed and awaiting approval — contracts 0.4.5

Per-row `config_version` on `device_credential`. Deliberately **not** dispatched
alongside T13: it rewrites the same `ListEdgeDeviceCredentials` function T13 was
repairing, and running both concurrently would recreate the seam pattern that
produced three production-blocking bugs last session. T13 has now landed, so it
is unblocked.

Narrower than it first appeared: `EdgeDeviceCredential` in
`packages/contracts/src/types/identity.ts` **already carries `config_version`**,
so the wire type does not change and the TS/Go drift tests are untouched. The
change is a new Postgres column plus a backend query that reads it per row
instead of substituting the outlet's.

The full proposal and its rubric self-review are in the session handover, not
here. Do not apply it without approval — it is a frozen-contract schema change
(CLAUDE.md, contract review rubric).

---

## Not started

**T9** POS billing UI · **T10** GST invoice print template · **T11** the §66
financial suite + harness money invariants. All three were blocked on T7c, which
has now passed, so all three are unblocked.

---

## Carried-forward gates that still bind

- **ADR-013: nothing has ever been built or run on the bare Windows 10 target.**
  This must clear before M2/M3 ships. Also covers USB/Bluetooth printing — only
  network printers have been exercised against a real socket.
- **Latency headroom is ~30%, not an order of magnitude.** M2 measured 150–183ms
  against a 250ms target over real WiFi; the harness measures P50 13ms on one
  machine, so WiFi adds ~140ms. Re-measure at an outlet with several screens.
- **`devseed` seeds no printer**, so the print path is unexercised in
  development.

---

## Process notes for whoever resumes

- **Worktrees are created from a stale base in this environment** — three tracks
  hit it last session. Every brief must open with a base check, and tracks
  should run in the main checkout partitioned by directory. This worked cleanly
  for T13 and T7c in parallel: two builders, two directories, zero conflicts,
  both committing to `main` without touching each other's files.
- **Commit each track before its gate.** Both tracks did this session.
- **The seam is where the defects are.** Gate the composition, not the
  components.
- **Ask verifiers to falsify a different property than the builder did.** Both
  gates this session did exactly that; one confirmed a guard was real by
  reproducing the mutation that defeated the previous gate, and one disproved
  the orchestrator's own leading hypothesis. A disproven hypothesis is a
  successful gate, not a wasted one.
- **Verify counts by reading the mount, not the summary.** The compliance route
  gap was carried as "six routes" through two documents; reading
  `compliance.Handler.Mount` shows eight.

---

## Repo hygiene

- Untracked and **not created by any Holler track**:
  `.github/copilot-instructions.md`, `.github/instructions/`. Both gates flagged
  them as foreign. Decide whether they are wanted.
- Stale branches to prune: five `worktree-agent-*`, `wip/edge-database-stash`,
  and `wip/t13-retry-partial` (`ca6c44a`, does not build — T13 was redone from
  scratch and this branch is now dead).
