# Development setup

How to get a fresh clone to the Milestone 1 acceptance state: a cashier logging
in and creating an order with the internet disconnected.

**None of this describes an outlet.** Outlet machines are bare Windows 10 with
no Docker, no Postgres and no developer toolchain (ADR-013). Everything below
is developer convenience for the *cloud* side plus a stand-in for device
enrollment that does not exist yet.

## Prerequisites
- Docker (any host: WSL2, Hyper-V, remote engine — WSL2 is not required)
- Go 1.22+, Rust/cargo, Node + pnpm — all run **natively on Windows**, not in WSL
- WebView2 runtime (present on most Windows 11 machines)

## Quick start

```powershell
# 1. everything except the frontend
.\scripts\dev-bootstrap.ps1

# 2. frontend dev server (leave running)
cd apps\pos
pnpm install
pnpm dev

# 3. in a SECOND terminal: env vars from step 1, then the POS
cd apps\pos
$env:HOLLER_OUTLET_ID  = "0191a000-0000-7000-8000-00000000000a"
$env:HOLLER_DEVICE_ID  = "0191a000-0000-7000-8000-00000000000b"
$env:HOLLER_DB_KEY_HEX = "5ff0c2a1b93d4e6f8a7c1d2e3f405162738495a6b7c8d9eafb0c1d2e3f405162"
pnpm exec tauri dev
```

Login: `cashier@holler.test` / `holler123`

Re-running `dev-bootstrap.ps1` is safe — both seeders upsert against fixed
development ids. Add `-SkipInfra` when the containers are already up.

## What the bootstrap actually does

| Step | Command | Effect |
|---|---|---|
| 1 | `docker compose up -d postgres redis nats` | Cloud infra only. The `backend` compose service is **deliberately skipped** — see Known gaps. |
| 2 | `go run ./cmd/devseed` (in `backend/`) | Applies `packages/contracts/postgres/*.sql` via the existing `postgres.Migrate`, seeds tenant/brand/outlet/role/cashier/menu, prints ids + the Argon2id hash |
| 3 | `cargo run --bin devseed` (in `edge/database/`) | Opens the encrypted edge SQLite at the POS app-data dir, seeds outlet/device/cashier/tables/menu, re-seals, and verifies offline login |
| 4 | — | Prints the three env vars and the credentials |

### Running the backend natively

The bootstrap does not start the backend, because nothing in the M1 acceptance
path needs it (see below). To run it anyway:

```powershell
cd backend
$env:DATABASE_URL = "postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable"
go run ./cmd/api      # health endpoint on :8080
```

## The three env vars

`apps/pos/src-tauri/src/state.rs` reads these at startup and **panics if any is
missing** — there is no device enrollment flow yet, so they stand in for it.

| Variable | Meaning |
|---|---|
| `HOLLER_OUTLET_ID` | Which outlet this till belongs to. Scopes login, menu, tables and orders. |
| `HOLLER_DEVICE_ID` | This till's device row. Stamped onto every order. |
| `HOLLER_DB_KEY_HEX` | 64 hex chars (32 bytes) — AES-256-GCM key for the edge database's encryption at rest (ADR-011). |

The key in the quick start is a **development key**. It must never be used for
anything holding real data. Changing it makes an existing `edge.db.enc`
undecryptable; delete the file and re-run the bootstrap if you rotate it.

## Where the edge database lives

`%APPDATA%\com.holler.pos\edge.db.enc` — Tauri's `app_data_dir()` for the
`com.holler.pos` identifier in `tauri.conf.json`.

Only the sealed file should ever be at rest there. A plaintext `edge.db`
(or `-wal`/`-shm`) sitting in that directory means a process died without
calling `Db::close`. Override the location for both the seeder and your own
tooling with `HOLLER_EDGE_DATA_DIR`.

Never copy this file or its backups anywhere unencrypted (ADR-011).

## Schema vs. data: what arrives when

This trips people up, so it is worth stating exactly:

- **Schema arrives by itself.** `Db::open` applies the frozen contract SQL from
  `packages/contracts/sqlite/` on every open, tracked with `PRAGMA
  user_version`. A brand-new device gets every Milestone 1 table.
- **Data does not.** The cloud-to-edge config pull (`GET /sync/config`) is
  implemented in `edge/sync/src/config.rs`, but `apps/pos/src-tauri/src/lib.rs`
  never starts the sync worker. So a fresh `edge.db.enc` has every table and
  **zero rows**, and login fails with a credential mismatch no matter how
  healthy the backend is.

`edge/database`'s `devseed` binary exists to fill that gap. When device
enrollment and sync startup are wired up, it should be deleted, not extended.

## Known gaps this setup works around

Five bootstrap problems have been found so far. Four are worked around here.

1. **`backend` compose service build is broken.** `docker compose up` including
   the `backend` service fails to produce a working image. Not diagnosed —
   `dev-bootstrap.ps1` starts only `postgres`, `redis` and `nats`, and the
   backend is run natively. Tracked as a separate issue.
2. **`@tauri-apps/cli` was missing from `apps/pos/package.json`.** Without it
   there is no `tauri` binary to run. Now a devDependency; `pnpm install` picks
   it up. There is no `tauri` npm *script*, so use `pnpm exec tauri dev`.
3. **Vite watcher looped on Rust rebuilds.** `vite.config.ts` now sets
   `server.watch.ignored: ["**/src-tauri/**"]` so Rust build artifacts do not
   retrigger the frontend. Committed.
4. **`beforeDevCommand` is empty** in `tauri.conf.json`, so `tauri dev` does
   **not** start Vite for you. Run `pnpm dev` in its own terminal first, or
   `tauri dev` opens a window pointed at a dead `localhost:5173`.
5. **Nothing seeded the databases.** Postgres had no migrations applied
   (`cmd/api` is still the Milestone 0 health-only entrypoint and never calls
   `postgres.Migrate`), and the edge database had no rows. This document and
   `scripts/dev-bootstrap.ps1` are the fix.

## Milestone 1 acceptance: pull the network cable

The acceptance criterion is that the cashier can still create orders with the
internet disconnected. Worth knowing why that works:

Login is Argon2id verification against the hash cached in the edge SQLite
(`repo::verify_offline_login`), and order creation writes locally with an
outbox row for later replay. **Neither touches the backend.** You can stop every
container and log in and take orders. Postgres matters for the cloud side that
Milestone 2 builds on, not for this acceptance run.

## Troubleshooting

| Symptom | Cause |
|---|---|
| POS panics: `HOLLER_OUTLET_ID ... required` | Env vars not set in *this* terminal. They are read by the Rust process, so they must be set where `tauri dev` runs, not where Vite runs. |
| Login fails with correct credentials | Edge DB seeded at a different path, or `HOLLER_OUTLET_ID` does not match the seeded outlet. `app_user` is unique per `(outlet_id, email)`. |
| `HOLLER_DB_KEY_HEX must be exactly 64 hex characters` | Key is not 32 bytes hex-encoded. |
| Blank POS window | Vite is not running. See gap 4. |
| `docker compose` warns about obsolete `version` | Harmless; the `version:` key in `docker-compose.yml` is a no-op in current Compose. |
