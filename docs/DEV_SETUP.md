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
# 1. everything except the frontends; also writes apps\pos\.env.dev and
#    apps\kds\.env.dev
.\scripts\dev-bootstrap.ps1

# 2. frontend dev server (leave running)
cd apps\pos
pnpm install
pnpm dev

# 3. in a SECOND terminal: the blessed launch command. This also starts the
#    KDS LAN server (embedded in the POS process — see "Milestone 2" below).
.\apps\pos\run-dev.ps1
```

Login: `cashier@holler.test` / `holler123`

`run-dev.ps1` is the **one** way to start the POS. It reads device identity and
the encryption key from `apps\pos\.env.dev` (written by the bootstrap in step
1), validates them, and refuses to launch with a clear message if Vite is not
up — rather than opening a blank window. Do not hand-set the `HOLLER_*`
variables; regenerate `.env.dev` by re-running the bootstrap instead.

`.env.dev` is gitignored because it carries the edge database's encryption key.
`apps/pos/.env.dev.example` documents its shape and is tracked.

Re-running `dev-bootstrap.ps1` is safe — both seeders upsert against fixed
development ids. Add `-SkipInfra` when the containers are already up.

Want to see a kitchen ticket land on a screen? Skip to
["Milestone 2: KDS LAN server and the item-1 runbook"](#milestone-2-kds-lan-server-and-the-item-1-runbook)
below once the quick start above is running.

## What the bootstrap actually does

| Step | Command | Effect |
|---|---|---|
| 1 | `docker compose up -d postgres redis nats` | Cloud infra only. The `backend` compose service is **deliberately skipped** — see Known gaps. |
| 2 | `go run ./cmd/devseed` (in `backend/`) | Applies `packages/contracts/postgres/*.sql` via the existing `postgres.Migrate`, seeds tenant/brand/outlet/role/cashier/menu, prints ids + the Argon2id hash |
| 3 | `cargo run --bin devseed` (in `edge/database/`) | Opens the encrypted edge SQLite at the POS app-data dir, seeds outlet/device/cashier/tables/menu/**a kitchen station and a KDS device row (T12)**, re-seals, and verifies offline login |
| 4 | — | Writes `apps\pos\.env.dev` and `apps\kds\.env.dev`, prints the credentials |

### Running the backend natively

The bootstrap does not start the backend, because nothing in the M1 acceptance
path needs it (see below). To run it anyway:

```powershell
cd backend
$env:DATABASE_URL = "postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable"
go run ./cmd/api      # health endpoint on :8080
```

## The env vars

`apps/pos/src-tauri/src/state.rs` reads the first three at startup and
**panics if any is missing** — there is no device enrollment flow yet, so
they stand in for it. You should not need to set them by hand: the bootstrap
writes them to `apps\pos\.env.dev` and `run-dev.ps1` loads them.

| Variable | Meaning |
|---|---|
| `HOLLER_OUTLET_ID` | Which outlet this till belongs to. Scopes login, menu, tables and orders. |
| `HOLLER_DEVICE_ID` | This till's device row. Stamped onto every order. |
| `HOLLER_DB_KEY_HEX` | 64 hex chars (32 bytes) — AES-256-GCM key for the edge database's encryption at rest (ADR-011). |
| `HOLLER_LAN_BIND_ADDR` | Optional (T12). Bind address for the embedded KDS LAN server. Defaults to `0.0.0.0:9310` if unset — see the Milestone 2 section below. Never fatal to POS startup if binding fails. |

The key in the quick start is a **development key**. It must never be used for
anything holding real data. Changing it makes an existing `edge.db.enc`
undecryptable; delete the file and re-run the bootstrap if you rotate it.

## Where the edge database lives

`%APPDATA%\com.holler.pos\edge.db.enc` — Tauri's `app_data_dir()` for the
`com.holler.pos` identifier in `tauri.conf.json`.

Only the sealed file should ever be at rest there. While the POS is running you
will also see `edge.db`, `-wal`, `-shm` and an `edge.db.open-marker` — that is
the decrypted working copy, and it is expected for the lifetime of the process.

After the POS exits, `edge.db.enc` should be the only file left. The database
seals itself on a normal exit (the `RunEvent::Exit` hook) and on drop, so
plaintext surviving an exit now means the process was killed outright — power
loss, `Stop-Process -Force`, End Task. That is recoverable, not fatal: the next
`Db::open` folds the committed data back into the sealed file and wipes the
plaintext. Running `scripts\dev-bootstrap.ps1` again is the easiest way to
trigger it.

Override the location for both the seeder and your own tooling with
`HOLLER_EDGE_DATA_DIR`.

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

## Milestone 2: KDS LAN server and the item-1 runbook

### Read this before you run any of this on a real network

**The KDS LAN port has no authentication.** It binds `0.0.0.0` by default —
reachable from anywhere on the LAN — and `edge/device/src/server.rs`'s
`handle_connection` accepts any `outlet_id`/`device_id` pair that is simply
non-empty. There is no device lookup, no token, no TLS. `device_id` is
identity, not a credential (see `docs/backlog-m2.md`, "Device enrollment" —
a HARD TRIGGER that blocks any pilot deployment, not just a dev nicety).
Concretely: anyone on the same LAN who can reach this port can read every
kitchen ticket for the outlet and can call `set_kot_status` — marking food
SERVED when it never left the kitchen, or CANCELLED on a live ticket, with
nothing on the server side distinguishing them from the real screen. **Do not
run this outside a network you fully control**, and never on an outlet's
production LAN until device enrollment closes this hole.

### How it actually starts

The POS process **embeds** the LAN server — `apps/pos/src-tauri/src/state.rs`
calls `holler_edge_device::server::start` itself, over the POS's own
`Arc<Mutex<Db>>`, and hands the resulting `Hub` to every kitchen command
(`commands/kitchen.rs`) so `send_order_to_kitchen`/`transition_kot_status`
push straight to connected KDS screens. This is a deliberate choice, not the
only possible one: the wire protocol (`lan.ts`) has no message for "another
process changed something, please rebroadcast", so the process that mutates
`kot` state and the process holding the `Hub` that announces it **must be the
same process**. `.\apps\pos\run-dev.ps1` therefore starts the LAN server too
— there is no separate script, because there is no separate process in the
normal path.

Default bind address: `0.0.0.0:9310` (fixed, not an ephemeral port — a KDS's
`VITE_KDS_LAN_URL` has to name a port ahead of time). Override with
`HOLLER_LAN_BIND_ADDR` in `apps/pos/.env.dev`. If the port is already taken,
the POS logs a warning and keeps running with kitchen tickets simply not
reaching any screen — Milestone 1's "cashier can create orders fully
offline" acceptance must hold even when this Milestone 2 feature cannot bind
its port.

### The standalone `kds-lan-server` binary — what it is for, and the one rule

`edge/device` also ships a `kds-lan-server` bin
(`edge/device/src/bin/kds_lan_server.rs`) that opens the same sealed
`edge.db.enc` and serves the LAN protocol on its own, with no POS/Tauri
process involved. Useful for testing a KDS screen's connectivity, snapshot
rendering and reconnect behaviour without building the full POS. Run it with:

```powershell
$env:HOLLER_DB_KEY_HEX = "<the dev key from apps\pos\.env.dev>"
cd edge\device
cargo run --bin kds-lan-server
```

**Never run this at the same time as the POS against the same
`edge.db.enc`.** `Db::open` decrypts the whole sealed file to a plaintext
working copy and re-seals on close; a second `Db::open` against a file
another process still has open is indistinguishable, by design, from crash
recovery (`crypto::recover_crash_leftovers`) — it will fold the first
process's in-progress state into a fresh seal and wipe the plaintext out from
under it. This is not a race worth chancing. Stop one before starting the
other. (For the same reason, `run-dev.ps1` and `kds-lan-server` also cannot
both bind `0.0.0.0:9310` at once — the second one to start will simply fail
to bind, which is the visible symptom if you forget this rule.)

### Item-1 runbook: two machines, one kitchen ticket

What to actually type to prove a cashier's send-to-kitchen reaches a KDS
screen, on a second machine over the LAN. Verified end to end while writing
this (see the T12 task report for the exact transcript: `create_order` ->
`confirm_order` -> `send_order_to_kitchen` -> a real WebSocket client
receiving `kot_upserted` within about a second).

**On the laptop running the POS (call it the outlet machine):**

```powershell
.\scripts\dev-bootstrap.ps1     # writes apps\pos\.env.dev and apps\kds\.env.dev
cd apps\pos; pnpm install; pnpm dev      # terminal 1 -- leave running
.\apps\pos\run-dev.ps1                   # terminal 2 -- POS + embedded LAN server on :9310
```

Find this machine's LAN IP (`ipconfig`, look for the adapter your KDS device
is actually on — `dev-bootstrap.ps1` guesses one and prints it, but guesses
wrong on a machine with several NICs, e.g. a VPN or Docker virtual adapter).
`apps\kds\.env.dev` is written with that guess already — open it and fix
`VITE_KDS_LAN_URL`'s host if it is wrong.

**On the second machine (or a second terminal on the same one):**

```powershell
cd apps\kds
pnpm install
pnpm dev --host 0.0.0.0 --mode dev
```

`--host 0.0.0.0` is required — Vite binds `localhost` only by default, and a
phone or a second PC cannot reach that. `--mode dev` is also required and is
easy to miss: Vite loads `.env.[mode]` where the default mode is
`development`, not `dev`, so a plain `pnpm dev` silently ignores
`apps/kds/.env.dev` and the app throws "`VITE_KDS_LAN_URL` is not
configured" — which reads like a missing file, not an unread one. Both flags
were verified by actually running the command and confirming `pnpm build
--mode dev`'s output bundle contains the configured LAN URL.

Then open `http://<laptop LAN IP>:5174` from the KDS device's browser (or the
same machine, in a second browser window). It connects, gets an empty
snapshot (`type: "snapshot", kots: []`), and starts receiving heartbeats.

**Back on the POS**, log in (`cashier@holler.test` / `holler123`), start an
order with the seeded "Masala Chai" item, confirm it, and send it to the
kitchen. The KDS screen should show the ticket within about a second —
`kot_upserted` for `NEW`/`ACKNOWLEDGED`/`PREPARING`/`READY`, `kot_removed`
(never a lingering "upserted" card) the moment it reaches `SERVED` or
`CANCELLED`, from either side driving the transition.

### What reaches a screen, and what does not (T12)

| POS-side change | Reaches a KDS screen? |
|---|---|
| `send_order_to_kitchen` (new tickets) | Yes — `kot_upserted` |
| `transition_kot_status` (POS-driven, e.g. a manual override) | Yes — `kot_upserted`, or `kot_removed` if the new status is SERVED/CANCELLED |
| A KDS-driven `set_kot_status` | Yes — was already wired (Milestone 2's original scope); unchanged by T12 |
| `cancel_kitchen_items_with_outbox` (the `#132 -> #132-C` cancellation ticket, `docs/spec/kitchen.md`) | **No caller exists anywhere in `apps/pos/src-tauri` or its frontend.** The function is implemented and tested in `edge/database` (`Db::cancel_kitchen_items_with_outbox`), but no Tauri command wraps it and no screen offers a "cancel this line" action. There is nothing to wire a notification onto — adding a command with no UI caller would repeat the exact "component with no caller" failure mode this task exists to fix (see `docs/retro.md`, 2026-08-11). Tracked as open work, not fixed here. |

## Known gaps this setup works around

Seven problems have been found so far. Four are worked around here, one is
fixed in the product (6), and two remain open (1, 7).

1. **`backend` compose service build is broken.** `docker compose up` including
   the `backend` service fails to produce a working image. Not diagnosed —
   `dev-bootstrap.ps1` starts only `postgres`, `redis` and `nats`, and the
   backend is run natively. Tracked as a separate issue.
2. **`@tauri-apps/cli` was missing from `apps/pos/package.json`.** Without it
   there is no `tauri` binary to run. Now a devDependency; `pnpm install` picks
   it up. There is no `tauri` npm *script*, so the underlying invocation is
   `pnpm exec tauri dev` — which `run-dev.ps1` wraps.
3. **Vite watcher looped on Rust rebuilds.** `vite.config.ts` now sets
   `server.watch.ignored: ["**/src-tauri/**"]` so Rust build artifacts do not
   retrigger the frontend. Committed.
4. **`beforeDevCommand` is empty** in `tauri.conf.json`, so `tauri dev` does
   **not** start Vite for you. Run `pnpm dev` in its own terminal first, or
   `tauri dev` opens a window pointed at a dead `localhost:5173`.
   `run-dev.ps1` checks for this and refuses to launch rather than opening a
   blank window, but it does not start Vite for you either — starting a
   background process the caller did not ask for is worse than a clear error.
5. **Nothing seeded the databases.** Postgres had no migrations applied
   (`cmd/api` is still the Milestone 0 health-only entrypoint and never calls
   `postgres.Migrate`), and the edge database had no rows. This document and
   `scripts/dev-bootstrap.ps1` are the fix.
6. **The edge database was never sealed on exit.** Found while running the POS
   for the first time: `Db::close` was the only thing that sealed and wiped,
   and nothing called it — no `Drop` impl, no Tauri exit hook — so every exit,
   including a clean window close, left the decrypted SQLite file and its
   cached Argon2id hashes on disk, against ADR-011. Fixed in the product
   (`RunEvent::Exit` hook plus seal-on-drop), not worked around here.
7. **The KDS LAN port has no authentication (T12, open).** Documented at the
   top of the Milestone 2 section above and in `docs/backlog-m2.md`, "Device
   enrollment" — a HARD TRIGGER against any pilot deployment, not something
   this dev setup works around. Also open: nothing calls
   `cancel_kitchen_items_with_outbox` from the POS or its frontend, so a
   kitchen cancellation ticket cannot reach a screen because there is no path
   that produces one yet (see "What reaches a screen" above).

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
| KDS shows "connecting..." forever, or throws `VITE_KDS_LAN_URL is not configured` | You ran plain `pnpm dev` — Vite ignored `.env.dev` because the mode was `development`, not `dev`. Re-run with `pnpm dev --host 0.0.0.0 --mode dev`. |
| KDS never receives a snapshot even though it connected | `VITE_KDS_LAN_URL`'s host is wrong for this network (common on a laptop with a VPN/Docker virtual adapter — `dev-bootstrap.ps1`'s IP guess can pick the wrong one). Check `ipconfig` on the POS machine and fix the host in `apps\kds\.env.dev`. |
| `kds-lan-server: KDS LAN server failed to bind ... address in use` (or the POS logs the same) | Something else is already bound to `:9310` — most likely the standalone `kds-lan-server` bin and `run-dev.ps1`'s embedded server both running at once. Stop one; see "the one rule" in the Milestone 2 section. |
| Send-to-kitchen does not reach the KDS, but the KDS is connected and showed a snapshot | The order's item is not routed to a station (`menu_item_station` has no row for it) — `send_order_to_kitchen` then produces zero KOTs, so there is nothing to notify. The seeded "Masala Chai"/"Veg Thali" items are both routed to `MAIN_KITCHEN`; a custom item needs the same. |
