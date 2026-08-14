# M3 resume state — 2026-08-14

Written before a machine restart. `main` is committed, clean and green at
`3d2bba7`. Nothing is in flight.

Read this with `docs/adr/ADR-016-m3-billing-contracts.md` and
`docs/adr/ADR-017-device-enrollment-credential.md` — the binding conditions
live there, not here.

---

## Verified green at `3d2bba7`

All executed, environments named:

| Suite | Result |
|---|---|
| `backend` | **12 packages**, `go test -count=1 ./...`, native Windows against Docker Postgres (`holler-postgres-1`), zero skips |
| `edge/database` | **142** (134 lib + 8 integration), `cargo test`, native Windows |
| `packages/contracts` TS | **39**, vitest/Node |
| `packages/contracts` Go | ok, `go test -count=1` |
| event-type drift | 17 frozen, 13 referenced by Rust, 4 deferred with reasons |
| contracts resolution | 2 consumers on 0.4.3 |

`apps/pos` (114, vitest/jsdom), `apps/pos/src-tauri` (45), `apps/kds` (30 +
1 Playwright/Chromium) and the e2e harness (54 scenarios, 0 invariant
violations) were green at their last run and untouched since.

**Rust `target/` was deleted in a disk cleanup — the first build in each Rust
crate is cold (minutes).** Disk went 8GB → 56GB free.

---

## Passed their gate this session

Each was verified by a read-only gate that independently falsified a property
the builder had **not** targeted.

| Track | Delivered |
|---|---|
| **T1** | Device enrollment, cloud: enrollment/rotate/revoke, `DeviceAuthenticate`, `/sync/config` off human tokens |
| **T2** | Unrouted-line guard: mixed orders reject naming the items, zero partial KOT writes; harness tightened to per-invariant assertions |
| **T3** | Cart surface: quantity as a single UPDATE, modifier deltas in line totals, `#132-A` (passed on retry) |
| **T4** | Enrollment edge/KDS: token out of the query string, first-frame auth, **offline verification from the local credential cache** (passed on retry) |
| **T5** | POS cart UI: tap-to-increment, modifier attachment, string-based money parsing |
| **T6** | Cloud tax engine: effective-dated resolution, ADR-016 rounding, four §66 properties (passed on retry) |
| **T7 pt1** | `ItemQuantityChanged` emission + `display_number` minting |
| **T7a** | Edge tax engine — line-for-line port of T6, 10 parity fixtures, + follow-up closing a zero-test gap in `resolve.rs` |
| **T7b** | **GST invoice issuance, offline numbering, split bills**, mint-wrap fix |
| **T8** | Billing ingest + **device-credential gating of all ingest** — cleared the pilot blocker |
| **T12** | Print path reads `display_number` → **UUID-on-KOT defect CLOSED**, `#132-C` and modifier-catalogue commands |
| **T14** | Committed parity-regeneration path; drift now fails the existing backend CI job |

**Contracts 0.4.0 → 0.4.3** (orchestrator-serialized): billing schema both
stores, TS/Zod + Go mirrors, OpenAPI, `ItemQuantityChanged`,
`menu_item.tax_profile_id`, `EdgeDeviceCredential`, `order.display_number`.

---

## FAILED gate — start here

### T13 device-credential path — one retry remaining

Config write path, raw-SQL removal and trailing-slash repair **passed** and are
merged (`3e4ad90`, `1852b84`). Two defects blocked it:

**Defect 1 — credential write and version bump are not atomic.**
`backend/internal/outlet/device_service.go` — `EnrollDevice`,
`RotateCredential`, `RevokeCredential` call `InsertCredential` /
`RevokeActiveCredential` and then `BumpOutletConfigVersion` as **separate,
non-transactional pool calls**. There is no `Begin`/`WithTx`/`pgx.Tx` anywhere
in `internal/outlet`.

A crash between them leaves a committed credential whose change was never
announced. An edge pulling at or above the un-bumped `config_version` never
sees it, and a retried enroll returns "device already enrolled; rotate
credential instead" — a success-shaped message while the device silently never
works. ADR-013 says a flaky connection at install time is the normal case.

Correct pattern already exists in this milestone: `compliance.Service` wraps
every write in `WithTx` and bumps inside it. Mirror it, and add a test that
fails if they are split again.

**Defect 2 — no test guards outlet isolation on the endpoint that ships
credential hashes.** The gate removed the `outlet_id` filter from
`ListEdgeCredentials` (`backend/internal/outlet/device_postgres.go`), keeping
only `tenant_id`, and **every existing test still passed**. A cross-outlet leak
in a two-branch tenant — the ordinary case — would ship silently.

Needs a test with two outlets under one tenant, a credential at each, asserting
a pull for A returns only A's. Falsify by removing the filter.

**Partial work:** branch `wip/t13-retry-partial` (`ca6c44a`) holds a 2-line
unused `pgx` import from the stopped retry. **Does not build. Do not merge.**
Start fresh.

---

## Open defects, with locations

| Where | What |
|---|---|
| `backend/internal/outlet/device_service.go` | Non-atomic credential write + `config_version` bump (T13 Defect 1) |
| `backend/internal/outlet/device_postgres.go` | `ListEdgeCredentials` outlet-scoping untested (T13 Defect 2) |
| `edge/sync/src/config.rs` | Empty `device_credentials` is not an error, unlike empty `users` — "no devices enrolled" and "cloud forgot to send them" are indistinguishable to the edge |
| `packages/contracts/postgres/0008_device_enrollment.sql` | `device_credential` has no per-row `config_version`, so `/sync/config` filtering is outlet-granular. Needs a contracts migration to close properly |
| `edge/database/src/invoice/assemble.rs` | `invoice_line.description` / `hsn_sac` read the **current** `menu_item` at issue time; `order_item` carries no name snapshot. A renamed item changes a reprinted invoice — a §31 reproducibility concern, disclosed by T7b |
| `edge/database/src/invoice/numbering.rs` | `{OUTLET}` token derived from `outlet.name`; no `outlet.code` column in the frozen contract |
| `edge/database/src/repo.rs` | Display-number reset buckets by **UTC** calendar day, not outlet-local business day |
| config authoring | A non-`NEVER` `reset_policy` with a prefix lacking the matching date token yields duplicate invoice numbers across periods. Caught by the UNIQUE index (fails loudly), but not validated at config-write time — candidate for `compliance.Service` validation |
| `backend/internal/compliance` | Writes use `outlet.manage`; no dedicated `billing.manage` permission exists in the frozen `Permission` enum |
| `packages/contracts/openapi/openapi.yaml` | The six compliance config write routes are implemented but **not** in the spec. Additive bump needed (orchestrator-only) |

---

## Not started

**T7c** — payments + cash shift at the edge (`edge/database`). Depends on T7b,
which is merged, so it is unblocked. Append-only payments: a void or refund is
a NEW row with `reverses_payment_id` and a non-positive amount, never an
update. Cash shift needs a reason for any non-zero variance (§39).

Then: **T9** POS billing UI · **T10** GST invoice print template · **T11** the
§66 financial suite + harness money invariants.

---

## Precise next action

1. Re-dispatch the **T13 retry** with the two defects above. It is the only
   FAILED gate and it sits on the pilot path. Do not resume
   `wip/t13-retry-partial`.
2. In parallel (no file overlap): dispatch **T7c** against `edge/database`.
3. Everything else waits on those.

## Process notes for whoever resumes

- **Worktrees are created from a stale base in this environment** — three
  tracks hit it. Every brief must open with a base check (`ls` a file that only
  exists on current `main`), and tracks should run in the main checkout
  partitioned by directory instead.
- **Commit each track before its gate.** Builders left work uncommitted twice;
  it was found only because a merge reported "Already up to date".
- **The seam is where the defects are.** Three production-blocking bugs this
  session were each between two separately-verified halves: ingest auth (T1/T4),
  `device_credentials` never populated (T13), and the print path never reading
  `display_number` (T7 pt1/T12). Gate the composition, not the components.
- **Ask verifiers to falsify a different property than the builder did.** Every
  gate that found something real did exactly this.
