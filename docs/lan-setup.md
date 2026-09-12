# LAN setup — Wednesday-morning demo checklist

**Who this is for:** the operator, alone, on demo morning, with no agent
available. Every command here is runnable as written in **Windows PowerShell
5.1**. Every port, variable and URL below was read out of the repository and is
cited with its file path — where something could **not** be established from the
repository, it says so in bold rather than guessing.

Companion documents:

- `docs/demo-status.md` — the current state of every demo work item and what is
  gated on a human. **Read it before the morning.** It is the authority on what
  exists; this file is only the authority on how to bring it up.
- `docs/demo-kickoff.md` — the six-step story being demonstrated.
- `docs/demo-reset.md` + `scripts/demo-reset.ps1` — the reset to a clean seed.
- `docs/DEV_SETUP.md` — the longer development runbook this compresses.

**Shell rules used throughout.** No `&&`, no `||`, no ternary, no `??` — all are
parser errors in PowerShell 5.1. Sequencing is `;` and `if ($?) { }`.

---

## 0. Three things that are true and will otherwise look like breakage

Read these three now. Each of them looks exactly like a broken demo and is not.

1. **Only `order` and `table_session` ever reach the cloud.**
   `edge/sync/src/route.rs` maps those two aggregate types and nothing else
   (carried gap **A7**). **KOTs, invoices, payments, cash shifts, stock counts
   and item-availability changes never replay** — they sit in `local_outbox` on
   the till forever, by known omission. If you watch an admin screen waiting for
   a kitchen ticket or a bill to arrive, it will never arrive. Do not treat that
   as a failure on the morning.

2. **No exit path on this build seals the edge database** (carried gap **A6**).
   Every run leaves a **plaintext `edge.db` beside the encrypted `edge.db.enc`**
   in the edge data directory. That is why `scripts/demo-reset.ps1` deletes both
   (`scripts/demo-reset.ps1`, the destructive-action banner names
   `$edgePlaintextPath` as "gap A6's leftover plaintext copy"). Delete the
   plaintext file after the demo; never copy it anywhere.

3. **A device credential that has never synced to the till cannot authenticate
   there. There is no cloud fallback in the POS process.** Both LAN listeners
   inside the POS verify **only** against the local `device_credential_cache`
   table:
   - KDS: `apps/pos/src-tauri/src/state.rs:782` —
     `CachedCredentialVerifier::new(db, "KDS", None)`, the `None` being the
     absent cloud fallback, deliberately.
   - Captain: `apps/pos/src-tauri/src/captain.rs:505-540` — "No cloud fallback —
     a credential absent from the local cache is `401`".

   That cache is populated **only** by a successful `GET /sync/config` pull
   (`edge/sync/src/config.rs:769`, `repo::replace_device_credential_cache`), and
   **nothing seeds it locally** — `edge/database/src/bin/devseed.rs` seeds
   `device` rows but no `device_credential_cache` row at all (verified: no
   match for `credential` in that file). So the order is always **enrol in
   the cloud → POS pulls config → device can connect**, never the other way
   round. As of T15, `scripts/dev-bootstrap.ps1` performs the cloud enrolment
   step for you, for both the POS's own sync credential and the KDS's, when
   it can reach `-CloudBaseUrl` at bootstrap time (steps 3b/3c) — see
   section 5.1. It cannot pull the config for you; that still happens only
   when the POS actually starts.

### Files you own, that no agent can touch

`apps/pos/.env.dev` and `apps/kds/.env.dev` carry the edge database encryption
key and device tokens. They are **deny-ruled to agents**. Every step below that
edits them is yours to run; nobody can have pre-run it for you.

---

## 1. The hub and its fixed IP

The **POS machine is the hub**. It runs the till, the KDS LAN WebSocket server
and the captain HTTP listener — both listeners live *inside* the POS process
(`apps/pos/src-tauri/src/state.rs:229` and `:260`), so if the POS is not running,
neither the KDS nor a phone can connect to anything.

**Every URL in this document is baked with the hub's IP address.** The KDS's
`VITE_KDS_LAN_URL` is a literal string in a file; the phone's captain URL is
typed into a browser. If the hub's address changes mid-demo — a Wi-Fi reconnect,
a DHCP lease expiring, the hotspot being toggled — **the KDS silently shows
"Disconnected from kitchen system" and the phone gets a connection error.**
Nothing repairs itself. Pin the address before anything else.

### 1a. If you are using Windows Mobile Hotspot

Windows Internet Connection Sharing assigns the hotspot adapter
**`192.168.137.1`** and hands out `192.168.137.x` to clients. That is a Windows
behaviour, **not something this repository defines** — no file in this repo sets
or asserts it. It is used as the worked example below because it is the address
Windows almost always picks.

**You generally cannot set a static IP on the Mobile Hotspot adapter**: ICS owns
it and will reassert `192.168.137.1`. That is fine — the address is already
stable for the length of a demo. What you must do is **confirm** it:

```powershell
Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike "127.*" -and $_.IPAddress -notlike "169.254.*" } | Select-Object InterfaceAlias, IPAddress, PrefixOrigin
```

**Check:** one row whose `InterfaceAlias` is the hotspot ("Local Area Connection*
N" is typical) with `IPAddress` `192.168.137.1`. `ipconfig` shows the same thing
if you prefer reading it that way — look for the adapter whose IPv4 address is
`192.168.137.1` and note it.

If more than one non-loopback IPv4 address is listed, **write down which one is
the hotspot** — and pass it to the bootstrap explicitly:

```powershell
.\scripts\dev-bootstrap.ps1 -LanHost 192.168.137.1   # ...plus your usual arguments
```

**`-LanHost` is the demo-day setting, and it beats every rule below.** On a
phone hotspot the hub's address changes when it reconnects, and an address
baked into `apps\kds\.env.dev` from a previous network produces a KDS that
loads, looks fine, and never connects.

**What the bootstrap does when you do not pass it:** it takes the IPv4 address
of the interface that owns the **default route**, because that is the interface
another device on the same network can actually reach. It prints which adapter
it chose:

```
       LAN address 192.168.0.106 on 'Wi-Fi' (the default-route interface)
```

**Read that line.** It used to pick the *first* non-loopback address instead,
and on a developer machine with WSL installed that was
`172.28.176.1` — the Hyper-V virtual switch, which no phone and no second PC
can route to. It looks like a perfectly good LAN address and answers only on
the machine that printed it. If the adapter named on that line is not the one
you are demoing over, re-run with `-LanHost`.

With **no default route at all** (an offline outlet — the normal case per
ADR-013) the bootstrap falls back to the first address that is *not* on an
obviously virtual adapter (`vEthernet`, `WSL`, `Hyper-V`, `VirtualBox`,
`VMware`, Bluetooth, TAP) and says so in yellow. If only virtual adapters
exist it warns in red and tells you to pass `-LanHost`.

### 1b. If you are using a physical router instead

Set a **static IPv4 address** on the hub's adapter, or a DHCP reservation on the
router. Static, from an elevated PowerShell (substitute your own adapter alias,
address, prefix length and gateway):

```powershell
Get-NetAdapter | Select-Object Name, InterfaceDescription, Status, ifIndex
New-NetIPAddress -InterfaceAlias "Wi-Fi" -IPAddress 192.168.1.50 -PrefixLength 24 -DefaultGateway 192.168.1.1
Set-DnsClientServerAddress -InterfaceAlias "Wi-Fi" -ServerAddresses 192.168.1.1
```

**Check:** `Get-NetIPAddress -InterfaceAlias "Wi-Fi" -AddressFamily IPv4` shows
your address with `PrefixOrigin` = `Manual`.

To undo afterwards:

```powershell
Remove-NetIPAddress -InterfaceAlias "Wi-Fi" -IPAddress 192.168.1.50 -Confirm:$false
Set-NetIPInterface -InterfaceAlias "Wi-Fi" -Dhcp Enabled
Set-DnsClientServerAddress -InterfaceAlias "Wi-Fi" -ResetServerAddresses
```

**From here on, `<HUB_IP>` means the address you just confirmed. The worked
examples use `192.168.137.1`.**

---

## 2. Every port and environment variable

### Ports

| Port | What listens | Env var that overrides it | Default | Defined in |
|---|---|---|---|---|
| **9310** | KDS LAN **WebSocket** server, inside the POS process | `HOLLER_LAN_BIND_ADDR` | `0.0.0.0:9310` | `apps/pos/src-tauri/src/state.rs:50` |
| **9310** | the same server run standalone (`kds-lan-server` binary) | `HOLLER_LAN_BIND_ADDR` | `0.0.0.0:9310` | `edge/device/src/bin/kds_lan_server.rs` (`DEFAULT_BIND_ADDR`) |
| **9320** | captain **HTTP** listener, inside the POS process — serves `apps/captain/dist` at `/` and JSON at `/api/` | `HOLLER_CAPTAIN_BIND_ADDR` | `0.0.0.0:9320` | `apps/pos/src-tauri/src/captain.rs:56` |
| **8080** | cloud backend API (`go run ./cmd/api`) | `PORT` | `8080` | `backend/internal/platform/config/config.go:44` |
| **5173** | POS Vite dev server (the Tauri window loads this) | — (`strictPort: true`) | `5173` | `apps/pos/vite.config.ts:11-12` |
| **5174** | KDS Vite dev server | — (`strictPort: true`) | `5174` | `apps/kds/vite.config.ts:11-12` |
| **5175** | admin console Vite dev server | — (`strictPort: true`) | `5175` | `apps/admin/vite.config.ts:16-17` |
| **5176** | captain Vite dev server (**dev only** — on the morning the till serves the built `dist` on 9320 instead) | — (`strictPort: true`) | `5176` | `apps/captain/vite.config.ts:16-17` |
| **5432** | Postgres (Docker) | — | `5432` | `docker-compose.yml:17` |
| **6379** | Redis (Docker) | — | `6379` | `docker-compose.yml:33` |
| **4222 / 8222** | NATS (Docker) client / monitoring | — | `4222`, `8222` | `docker-compose.yml:48-49` |

**Never run the standalone `kds-lan-server` binary at the same time as the POS**
against the same `edge.db.enc` — two processes, one SQLite file. The POS already
embeds that server; the standalone binary exists for connectivity testing
without a POS build (`apps/pos/run-dev.ps1` header comment).

### `HOLLER_*` — the POS process (read from `apps/pos/.env.dev` by `apps/pos/run-dev.ps1`)

| Variable | Required? | Default | What reads it |
|---|---|---|---|
| `HOLLER_OUTLET_ID` | **required** | none | `apps/pos/src-tauri/src/state.rs:213` |
| `HOLLER_DEVICE_ID` | **required** | none | `apps/pos/src-tauri/src/state.rs:215` |
| `HOLLER_DB_KEY_HEX` | **required**, 64 hex chars | **no default, deliberately** | `state.rs`; validated in `run-dev.ps1` and `dev-bootstrap.ps1` |
| `HOLLER_EDGE_DATA_DIR` | optional | `%APPDATA%\com.holler.pos` | `edge/device/src/bin/kds_lan_server.rs` (`default_app_data_dir`), `dev-bootstrap.ps1`, `demo-reset.ps1` |
| `HOLLER_LAN_BIND_ADDR` | optional | `0.0.0.0:9310` | `state.rs:229` |
| `HOLLER_CAPTAIN_BIND_ADDR` | optional | `0.0.0.0:9320` | `state.rs:260` |
| `HOLLER_CAPTAIN_DIST_DIR` | optional | compile-time path to `apps/captain/dist` | `captain.rs:67` |
| `HOLLER_CLOUD_BASE_URL` | **all three or none** | none | `state.rs:736` |
| `HOLLER_TENANT_ID` | **all three or none** | none | `state.rs:739` |
| `HOLLER_DEVICE_TOKEN` | **all three or none** | none | `state.rs:740` |
| `HOLLER_SYNC_PUMP_INTERVAL_SECS` | optional | **60 seconds** | `state.rs:75`, `:93`, `:102` |
| `HOLLER_PRINTER_FILE_SINK_DIR` | optional | unset = print to a device | `edge/printer/src/transport/file_sink.rs`; wired by `dev-bootstrap.ps1 -PrinterFileSinkDir` |
| `HOLLER_DEMO_UPI_VPA` | optional (needed for the UPI QR **on the receipt**) | none | `edge/printer/src/upi.rs:45` |
| `HOLLER_DEMO_UPI_PAYEE_NAME` | optional | none | `edge/printer/src/upi.rs` |

**The sync trio is all-or-nothing.** With any one of `HOLLER_CLOUD_BASE_URL` /
`HOLLER_TENANT_ID` / `HOLLER_DEVICE_TOKEN` missing, the POS logs
`sync worker disabled (… are required together); the outlet works offline and
nothing replays` (`state.rs:757`) and **no config pull ever happens** — which
means no device credential ever reaches the cache, which means **the KDS and the
phone can never authenticate**. This is the single most likely silent failure of
the morning.

### `VITE_*` — the web apps

| Variable | App | Required? | What reads it |
|---|---|---|---|
| `VITE_KDS_LAN_URL` | KDS | **required**, throws if unset | `apps/kds/src/lib/lanConfig.ts` |
| `VITE_KDS_OUTLET_ID` | KDS | **required**, throws | same |
| `VITE_KDS_DEVICE_ID` | KDS | **required**, throws | same |
| `VITE_KDS_DEVICE_TOKEN` | KDS | **required**, throws | same |
| `VITE_KDS_STATION` | KDS | optional — unset means this screen receives every station | same |
| `VITE_ADMIN_API_BASE_URL` | admin | **required** (empty is reported as an error on screen) | `apps/admin/src/lib/api.ts:27` |
| `VITE_ADMIN_OUTLET_ID` | admin | **required** | `apps/admin/src/lib/api.ts:28` |
| `VITE_ADMIN_TENANT_ID` | admin | **required** | `apps/admin/src/lib/api.ts:31` |
| `VITE_CAPTAIN_API_BASE_URL` | captain | optional — empty means same-origin, which is what you want when the till serves it on 9320 | `apps/captain/src/lib/api.ts:15` |
| `VITE_HOLLER_DEMO_UPI_VPA` | POS | optional (needed for the UPI QR **on screen**) | `apps/pos/src/domain/upi.ts:31` |
| `VITE_HOLLER_DEMO_UPI_PAYEE_NAME` | POS | optional | `apps/pos/src/domain/upi.ts:32` |

**The UPI pair is doubled on purpose.** The Rust printer reads the unprefixed
names; the POS frontend reads the `VITE_`-prefixed ones
(`edge/printer/src/upi.rs:16-24`). **Set all four**, or the QR appears on one
surface and not the other.

### Backend environment (cloud machine)

| Variable | Required? | Default | Defined in |
|---|---|---|---|
| `DATABASE_URL` | **required** — startup error if missing | none | `backend/internal/platform/config/config.go:50` |
| `TOKEN_SIGNING_KEY` | **required** — startup error if missing | none | `config.go:54` |
| `HOLLER_CORS_ALLOWED_ORIGINS` | required **if a browser talks to it** (the admin console does) | none; empty = no CORS headers at all | `config.go:47` |
| `PORT` | optional | `8080` | `config.go:44` |
| `CONTRACTS_DIR` | optional | `../packages/contracts/postgres` | `config.go:46` |
| `ACCESS_TOKEN_TTL` | optional | `15m` | `config.go:62` |
| `REFRESH_TOKEN_TTL` | optional | `720h` | `config.go:65` |

### KDS `.env.dev` is read only with `--mode dev`

Vite's default mode is `development`, not `dev`. `apps/kds/.env.dev` is loaded
**only** when the KDS is started with `--mode dev`
(`scripts/dev-bootstrap.ps1`, the comment above `$kdsEnvLines`;
`scripts/dev-up.ps1:208` passes it). Omit the flag and the KDS starts with **no
configuration at all** and throws on load.

---

## 3. The URLs, by hub IP

| Surface | Template | Worked example (`<HUB_IP>` = `192.168.137.1`) |
|---|---|---|
| KDS → till, LAN socket | `ws://<HUB_IP>:9310/kds` | `ws://192.168.137.1:9310/kds` |
| KDS page (dev server on the hub) | `http://<HUB_IP>:5174` | `http://192.168.137.1:5174` |
| Captain (waiter phone) | `http://<HUB_IP>:9320/` | `http://192.168.137.1:9320/` |
| Admin console | `http://<HUB_IP>:5175` | `http://192.168.137.1:5175` |
| Cloud API | `http://<CLOUD_IP>:8080` | `http://192.168.137.1:8080` (or the second PC's address — §7) |

**Which scheme is which:** the KDS hop is **`ws://`** — a WebSocket, not a web
page. The captain hop is **`http://`**. The KDS *page* is served over `http://`
on 5174 and then opens a `ws://` connection to 9310; those are two separate
hops and two separate ports, and only the second one is in `.env.dev`.

The path matters. `/kds` is the handshake path
(`apps/kds/src/lib/lanConfig.ts` — the URL is built as
`ws://host:port/kds?outlet_id=…&device_id=…`). `buildConnectionUrl` appends the
query parameters; do not add them by hand.

---

## 4. Windows firewall

Both hops are **plaintext HTTP / WS on a flat LAN**. That is accepted for a demo
on our own hotspot and is stated in the code itself
(`apps/pos/src-tauri/src/captain.rs` header; `docs/captain-api.md`, "Plaintext
HTTP on a flat LAN is accepted for the demo, on our own hotspot"). **The security
gate review for a LAN-facing device is filed for pilot, not done.** Do not put
this on a network you do not control.

First, find out what profile Windows has assigned the hotspot adapter — the rules
below are scoped to `Private`, and a hotspot is often classified `Public`:

```powershell
Get-NetConnectionProfile | Select-Object InterfaceAlias, NetworkCategory, InterfaceIndex
```

If the hotspot adapter shows `Public`, either change it (elevated):

```powershell
Set-NetConnectionProfile -InterfaceIndex <ifIndex> -NetworkCategory Private
```

…or change `-Profile Private` to `-Profile Any` in the rules below.

### Add the rules (elevated PowerShell)

Scoped to the hotspot subnet, not to the whole world. Adjust `192.168.137.0/24`
if your subnet differs.

```powershell
$subnet = "192.168.137.0/24"
New-NetFirewallRule -DisplayName "Holler demo - KDS LAN WS 9310" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 9310 -Profile Private -RemoteAddress $subnet
New-NetFirewallRule -DisplayName "Holler demo - Captain HTTP 9320" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 9320 -Profile Private -RemoteAddress $subnet
New-NetFirewallRule -DisplayName "Holler demo - KDS page 5174" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 5174 -Profile Private -RemoteAddress $subnet
```

Add these two **only if** that surface is opened from another machine:

```powershell
New-NetFirewallRule -DisplayName "Holler demo - Admin page 5175" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 5175 -Profile Private -RemoteAddress $subnet
New-NetFirewallRule -DisplayName "Holler demo - Cloud API 8080" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8080 -Profile Private -RemoteAddress $subnet
```

**8080 goes on whichever machine runs the backend** — see §7. If the backend is
on the second PC, the rule belongs there, not on the hub.

**Check:**

```powershell
Get-NetFirewallRule -DisplayName "Holler demo -*" | Select-Object DisplayName, Enabled, Direction, Action
```

Five (or three) rows, `Enabled` = `True`, `Direction` = `Inbound`, `Action` =
`Allow`.

Better check — prove the port is actually reachable **from the second machine**,
once the POS is running:

```powershell
Test-NetConnection -ComputerName 192.168.137.1 -Port 9310
```

`TcpTestSucceeded : True`. `False` means either the POS is not running (nothing
is listening) or the rule is not taking effect — distinguish them by running
`Get-NetTCPConnection -LocalPort 9310 -State Listen` **on the hub**.

### Remove the rules afterwards

A demo laptop should not keep ports open. Run this when the demo is over:

```powershell
Get-NetFirewallRule -DisplayName "Holler demo -*" | Remove-NetFirewallRule
```

**Check:** `Get-NetFirewallRule -DisplayName "Holler demo -*"` returns nothing
(and prints a "No MSFT_NetFirewallRule objects found" error, which is the
expected result here).

---

## 5. Enrolling a second PC as the KDS

> **A `[3c/4]` failure that says 404 is a MISSING DEVICE, not a missing route.**
> `POST /devices/{id}/credentials/rotate` exists and is served
> (`backend/internal/outlet/device_http.go:29`). The bootstrap remembers
> `(cloud, outlet, kind, name) -> device id` in
> `%LOCALAPPDATA%\Holler\dev-bootstrap-state.json` so it can rotate a
> credential instead of enrolling a second device — and
> `scripts\demo-reset.ps1` drops the whole schema, which deletes every device
> row while those remembered ids survive. Rotating one then 404s.
>
> Fixed on 2026-09-12 in three places, so this should no longer reach you: on a
> local cloud the database is authoritative even when it says "no such device";
> a rotate that 404s falls back to enrolling; and the reset prunes the state
> entries it just invalidated. If you hit it on an older build, delete that
> state file and re-run the bootstrap.

The KDS needs **four** things in `apps/kds/.env.dev`. **As of T15,
`scripts/dev-bootstrap.ps1` writes all four** — `VITE_KDS_LAN_URL`,
`VITE_KDS_OUTLET_ID`, `VITE_KDS_DEVICE_ID` and `VITE_KDS_DEVICE_TOKEN` — as
long as `-CloudBaseUrl` was reachable when the bootstrap ran (its `[3c/4]`
step). **Confirm it worked**: the bootstrap prints `KDS credential ENABLED
(token written to apps\kds\.env.dev only)`. If instead it printed `[3c/4] KDS
credential SKIPPED: ...` (in **red**, not the yellow used for the POS's own
sync-credential skip — the KDS is unstartable without this one), the token
line was **not** written, `apps/kds/src/lib/lanConfig.ts` will throw at
startup, and you have two options: re-run the bootstrap once the reason
printed above that line is fixed (usually "no API at ..." — start the
backend first), or do the enrolment by hand below and paste the token into
`apps\kds\.env.dev` yourself.

**One correction to keep in mind either way**: the bootstrap's `-KdsDeviceId`
(default `0191a000-0000-7000-8000-00000000000d`, the seeded local `device`
row) and the id `POST /devices/enroll` returns for the KDS's *credential* are
**deliberately two different device ids**, not a mismatch to fix. Verification
(`edge/device/src/auth.rs`, `CachedCredentialVerifier::check_cached_row`)
checks a cached credential's `outlet_id` and `device_kind` only — never
`device_id` — so any KDS-kind credential enrolled for this outlet
authenticates the connection. `VITE_KDS_DEVICE_ID` has a separate job: it is
the value the edge stamps into `kot_status_history.changed_by_device_id`,
which carries a real `REFERENCES device(id)` foreign key
(`packages/contracts/sqlite/0005_m2_kitchen_stations_printers.sql:114`) —
that row must exist in the **local** `device` table, which only the seeded
id does (nothing syncs the `device` table itself down from the cloud; only
`device_credentials` travels in the config bundle). So: leave
`VITE_KDS_DEVICE_ID` as the seeded value; only the token changes.

### 5.1 Enrol a KDS device against the cloud, by hand (fallback)

Only needed if the bootstrap's `[3c/4]` step was skipped. On the machine that
can reach the backend. `POST /devices/enroll` is gated on `outlet.manage`,
which the seeded cashier does **not** hold — log in as `owner@holler.test`
(`scripts/dev-bootstrap.ps1`, `$SyncEnrollEmail`).

Request shape and response fields are from
`backend/internal/outlet/device_http.go:55-60` (`enrollDeviceRequest`:
`outlet_id`, `kind`, `name`, `label`) and `:35-41` (`enrolledDeviceResponse`:
`device_id`, `outlet_id`, `kind`, `name`, `credential_id`, `token`).

```powershell
$cloud    = "http://localhost:8080"     # or the second PC's address - see section 7
$tenantId = "<HOLLER_TENANT_ID from apps\pos\.env.dev>"
$outletId = "<HOLLER_OUTLET_ID from apps\pos\.env.dev>"

$login = Invoke-RestMethod -Uri "$cloud/auth/login" -Method Post -ContentType 'application/json' `
  -Headers @{ 'X-Tenant-ID' = $tenantId } `
  -Body (@{ email = 'owner@holler.test'; password = 'holler123'; outlet_id = $outletId } | ConvertTo-Json)

$hdrs = @{ 'X-Tenant-ID' = $tenantId; 'Authorization' = "Bearer $($login.access_token)" }

$kds = Invoke-RestMethod -Uri "$cloud/devices/enroll" -Method Post -ContentType 'application/json' `
  -Headers $hdrs `
  -Body (@{ outlet_id = $outletId; kind = 'KDS'; name = 'Demo Kitchen Screen'; label = 'demo' } | ConvertTo-Json)

$kds.device_id
$kds.token
```

**Check:** two lines print — a UUID and a token of the form
`<credential_id>.<secret>`. **The plaintext token is returned exactly once**
(ADR-017; `device_http.go:32` — "carries the plaintext token EXACTLY ONCE").
Copy both values somewhere now.

**If you get a 409**, this device name is already enrolled. Rotate instead —
`POST /devices/<deviceId>/credentials/rotate` with body `{"label":"demo"}`
returns the same shape with a fresh token (`device_http.go:28`). Or enrol under a
different `name`; the cloud matches an existing device by (tenant, outlet, name).

**If you get a 401**, note that ADR-012 returns the *same* response for a wrong
password and for a throttled one — five attempts per fifteen-minute window, held
in process memory. Restarting the backend clears it.

### 5.2 Write the KDS env file (operator only — `.env.dev` is deny-ruled)

On the **hub**, edit `apps\kds\.env.dev` so it reads exactly:

```
VITE_KDS_LAN_URL=ws://192.168.137.1:9310/kds
VITE_KDS_OUTLET_ID=<outlet id>
VITE_KDS_DEVICE_ID=0191a000-0000-7000-8000-00000000000d
VITE_KDS_DEVICE_TOKEN=<token from step 5.1>
```

One thing to get right:

- **`VITE_KDS_LAN_URL` must carry `<HUB_IP>`, never `localhost`.** The bootstrap
  guesses this with `Get-LanIPv4`, which takes the first non-loopback address —
  on a multi-NIC machine that is often the wrong one. Correct it against what
  you confirmed in §1.

**Leave `VITE_KDS_DEVICE_ID` as the seeded `0191a000-0000-7000-8000-00000000000d`
— do not replace it with the id this enrol call returned.** See the note at
the top of this section: the credential's `device_id` and the connection's
`device_id` serve different, deliberately unrelated purposes, and only the
seeded id satisfies the local `kot_status_history` foreign key.

### 5.3 The credential must reach the till before the KDS can connect

The POS verifies against `device_credential_cache` with **no cloud fallback**
(§0.3). That table is filled by a `GET /sync/config` pull, which runs at POS
startup and then on the periodic loop — **every 60 seconds by default**
(`state.rs:93`). So:

1. Confirm the POS has its sync trio set (`HOLLER_CLOUD_BASE_URL`,
   `HOLLER_TENANT_ID`, `HOLLER_DEVICE_TOKEN` in `apps\pos\.env.dev`).
2. Start or restart the POS **after** the enrol in 5.1.
3. Watch the POS terminal.

**Check:** the POS terminal prints

```
holler-pos: sync worker hosted, cloud at http://...
holler-pos: <phase> config pull applied a new bundle
```

(`state.rs:746` and `:472`). If instead you see
`sync worker disabled (… are required together)`, stop here and fix
`apps\pos\.env.dev` — nothing downstream will work.

To make the wait short during setup, set
`HOLLER_SYNC_PUMP_INTERVAL_SECS=10` in `apps\pos\.env.dev` and restart the POS.
Decide before the demo whether to leave it there; 60s is the shipped default.

### 5.4 Start the KDS and confirm it actually connected

On the hub (serving to the second PC):

```powershell
cd C:\Code\Holler\apps\kds
pnpm install
pnpm dev --mode dev --host 0.0.0.0
```

`--mode dev` is not optional — without it `.env.dev` is not read at all.
`--host 0.0.0.0` is not optional either if the page is opened from another
machine.

On the KDS machine, open `http://192.168.137.1:5174`.

**How to tell it connected, rather than merely loaded.** `ConnectionBanner`
renders **nothing at all when the status is `connected`**
(`apps/kds/src/components/ConnectionBanner.tsx:14` — `if (status === "connected")
return null;`). So:

- **Connected** = no banner. The absence of the banner *is* the signal.
- **Not connected** = a visible banner reading "Disconnected from kitchen
  system" or "Connecting…".

Absence of a banner is a weak signal on its own, so prove it positively:
**send a test order from the till and watch a ticket appear on the KDS.** A
loaded-but-disconnected KDS renders an empty board that looks identical to a
connected one with no tickets.

Second confirmation, on the hub:

```powershell
Get-NetTCPConnection -LocalPort 9310 | Select-Object State, RemoteAddress, RemotePort
```

**Check:** one `Listen` row plus at least one `Established` row whose
`RemoteAddress` is the KDS machine.

---

## 6. Enrolling a phone as the WAITER device

`WAITER` is already a valid `device.kind` — `backend/internal/outlet/device.go:18`
(`DeviceKindWaiter DeviceKind = "WAITER"`) and
`packages/contracts/src/types/identity.ts`. **No contract change is needed**, and
`docs/demo-status.md` item 0 confirms this.

### 6.1 Enrol

Same call as 5.1 with `kind = 'WAITER'`:

```powershell
$waiter = Invoke-RestMethod -Uri "$cloud/devices/enroll" -Method Post -ContentType 'application/json' `
  -Headers $hdrs `
  -Body (@{ outlet_id = $outletId; kind = 'WAITER'; name = 'Demo Captain Phone'; label = 'demo' } | ConvertTo-Json)

$waiter.token
```

**Check:** a `<credential_id>.<secret>` token prints. Keep it — it is issued once.

### 6.2 Let it sync to the till

Exactly as 5.3. The captain listener resolves the token against
`device_credential_cache` and additionally requires `device_kind == "WAITER"`
(`captain.rs:547`). A credential that has not synced gives
`401 device credential not cached locally` (`captain.rs:539`) — which on the
phone appears as **"That device token was rejected. Check it and paste it again."**
(`apps/captain/src/components/PairScreen.tsx:30`). **That message is identical
for a mistyped token and for a not-yet-synced one.** So before blaming the
typing, confirm the pull ran.

**How the operator can tell the credential has arrived:** the POS terminal
printed `config pull applied a new bundle` (`state.rs:472`) **after** the enrol
call. That is the only in-product signal; there is no screen that lists cached
credentials.

### 6.3 Pair the phone

1. Put the phone on the same hotspot.
2. Build the captain page if `apps\captain\dist` is missing or stale:
   ```powershell
   cd C:\Code\Holler\apps\captain
   pnpm install
   pnpm build
   ```
   **Check:** `apps\captain\dist\index.html` exists with a recent timestamp.
   The POS serves this directory (`captain.rs:70`); `HOLLER_CAPTAIN_DIST_DIR`
   overrides where it looks.
3. Restart the POS if you just built `dist`.
4. On the phone, open `http://192.168.137.1:9320/`.
5. Paste the WAITER token on the pair screen and submit.

**Check:** the phone moves off the pair screen to the **tables** list
(`apps/captain/src/App.tsx:127`). The token is only stored after it validates
(`PairScreen.tsx:12` — "does not store anything, so a mistyped token is never
persisted as though it worked"), so reaching the tables screen is proof the
credential verified against the till, not just that the page loaded.

**End-to-end check:** pick a table, add an item, send. A ticket appears on the
KDS. `docs/demo-status.md` records that **no browser has ever talked to the real
captain listener** — the seam was confirmed by static comparison and a
hand-rolled HTTP client. Rehearse this one properly; it is the least-exercised
path in the whole demo.

---

## 7. Running the cloud on a second PC so it can be unplugged

This exists for **demo step 4**: *stop the cloud, take and bill an order, restart
the cloud, the banner clears.*

**Why a second machine is necessary.** Every "network disconnected" step in this
project since M1 was performed by switching Wi-Fi off against a cloud at
`http://localhost:8080`. That traffic never leaves the machine, so the step
passed identically with Wi-Fi on, off, or every adapter disabled — the offline
precondition was never established, in any run, in any milestone
(`scripts/check-cloud-unreachable.ps1`, "WHY THIS EXISTS"). Putting the backend
on a different box is what makes step 4 honest.

### 7.1 What moves

Everything cloud-side moves to **PC-B**: Docker (Postgres, Redis, NATS) and the
Go backend. The hub keeps the POS, the LAN listeners, and — your choice — the
KDS and admin dev servers.

On **PC-B**, with the repository checked out and Docker running:

```powershell
cd C:\Code\Holler
docker compose up -d postgres redis nats
cd backend
$env:DATABASE_URL    = 'postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable'
$env:TOKEN_SIGNING_KEY = 'holler-dev-signing-key-not-for-prod'
$env:HOLLER_CORS_ALLOWED_ORIGINS = 'http://192.168.137.1:5175'
go run ./cmd/api
```

The connection string and dev signing key are `scripts/dev-up.ps1`'s own
defaults (`$DatabaseUrl`, `$TokenSigningKey`). **`HOLLER_CORS_ALLOWED_ORIGINS`
must be the admin console's exact origin** — the API has no default, and an
unset allowlist emits no CORS headers at all, so every admin request fails with
"Failed to fetch" and nothing useful appears in either log
(`config.go:28-35`, `dev-up.ps1:182-190`). If the admin is served from the hub,
that origin is `http://<HUB_IP>:5175`, not `http://localhost:5175`.

Open 8080 inbound **on PC-B** (§4).

**Check, from the hub:**

```powershell
Invoke-RestMethod -Uri "http://<PC_B_IP>:8080/health"
```

### 7.2 What the POS must point at

In `apps\pos\.env.dev` on the hub (operator-owned file):

```
HOLLER_CLOUD_BASE_URL=http://<PC_B_IP>:8080
HOLLER_TENANT_ID=<tenant id>
HOLLER_DEVICE_TOKEN=<the POS device token>
```

Those are the real names, verified at `state.rs:736`, `:739` and `:740`. **All
three or none** — a partial set disables sync entirely.

The POS device token comes from `scripts/dev-bootstrap.ps1`'s step 3b, which
enrols a `POS` device named `Holler Dev Till` and writes the three lines for you
— but it enrols against **its own** `-CloudBaseUrl`, defaulting to
`http://localhost:8080`. Point it at PC-B:

```powershell
.\scripts\dev-bootstrap.ps1 -SkipInfra -WithBilling -CloudBaseUrl "http://<PC_B_IP>:8080"
```

**Check:** the bootstrap prints `sync ENABLED for this till (token written to
.env.dev only)`. If it prints `sync credential SKIPPED: no API at …`, PC-B is not
reachable — fix that before going further. The same applies to `[3c/4]` and the
KDS token; both enrolments run against whichever `-CloudBaseUrl` you passed.

**Note (fixed at T15):** the device-lookup step used to shell into a container
named `holler-postgres-1` on the **local** Docker unconditionally, so pointed
at PC-B it silently found nothing and the script took the *enrol* branch every
run — 409 on any re-run instead of rotating. It now detects whether
`-CloudBaseUrl`'s host is `localhost`/`127.0.0.1`: on the hub against
`http://<PC_B_IP>:8080` that is false, so it skips the local Postgres lookup
and instead remembers the device id it minted, per `(cloud url, outlet, kind,
name)`, in `%LOCALAPPDATA%\Holler\dev-bootstrap-state.json` on **this**
machine — machine-local bookkeeping, never committed, never containing the
token itself. A **re-run on the same hub machine** against the same PC-B
therefore rotates correctly. If that state file is missing or was cleared (a
fresh checkout, a different machine, or the device was enrolled by hand) and
the name is already taken on PC-B, the bootstrap does not guess: it throws a
message naming exactly that — no LIST route exists to recover the id
automatically, so the fix is a manual rotate (§5.1) with whatever id you
noted from the original enrolment, or enrolling under a different name.

Also update `apps\admin\.env.local` so `VITE_ADMIN_API_BASE_URL` is
`http://<PC_B_IP>:8080` (`apps/admin/src/lib/api.ts:27`), alongside
`VITE_ADMIN_OUTLET_ID` and `VITE_ADMIN_TENANT_ID`. Restart the admin dev server
after editing — Vite reads env files at server start.

### 7.3 Stopping and restarting it convincingly

**On PC-B**, stop the backend by port and confirm the port is free:

```powershell
$p = (Get-NetTCPConnection -LocalPort 8080 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty OwningProcess)
"old pid: $p"
if ($p) { Stop-Process -Id $p -Force }
Start-Sleep -Seconds 2
Get-NetTCPConnection -LocalPort 8080 -State Listen -ErrorAction SilentlyContinue
```

**Check:** the last command returns nothing.

For a fully convincing stop, also stop the containers: `docker compose stop`.
That is optional for step 4 — the POS only ever talks to 8080.

**On the hub**, prove the cloud is unreachable *from the till's point of view*:

```powershell
.\scripts\check-cloud-unreachable.ps1 -BaseUrl "http://<PC_B_IP>:8080" -Port 8080
```

**Check:** it exits 0 and prints offline-confirmed. It is fail-closed —
anything ambiguous reports REACHABLE and exits 1.

**One honest caveat about that script on a two-machine setup.** Its first probe
is `Get-NetTCPConnection -LocalPort <Port> -State Listen`, which inspects **the
machine it runs on**. On the hub there is never a listener on 8080, so that probe
passes trivially and carries no information. The probes that actually mean
something in this configuration are the HTTP ones against `-BaseUrl`, which
distinguish "the server answered (any status)" from "nothing answered"
(`ConnectFailure` / `Timeout` / `NameResolutionFailure`). Run the script on the
hub for the HTTP probes, and confirm the listener is gone **on PC-B** with the
snippet above.

**Restart, and verify by a NEW PID — never by the port answering.** The old
process answers identically, so a health check passing is entirely consistent
with "nothing restarted". This project has already lost debugging time to exactly
that: a backend restart failed to bind 8080, the original kept serving, and a
stale in-memory rate-limit window read as a credential fault.

On PC-B:

```powershell
cd C:\Code\Holler\backend
$env:DATABASE_URL    = 'postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable'
$env:TOKEN_SIGNING_KEY = 'holler-dev-signing-key-not-for-prod'
$env:HOLLER_CORS_ALLOWED_ORIGINS = 'http://192.168.137.1:5175'
Start-Process powershell -ArgumentList '-NoExit','-NoProfile','-Command','Set-Location C:\Code\Holler\backend; go run ./cmd/api'
Start-Sleep -Seconds 8
$new = (Get-NetTCPConnection -LocalPort 8080 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty OwningProcess)
"new pid: $new"
```

**Check:** `new pid` is non-empty **and different from** the `old pid` you
printed before killing it. Same pid = nothing restarted.

`scripts/demo-reset.ps1` already implements this whole kill-confirm-start-confirm
sequence (`scripts/demo-reset.ps1:164-234`) and fails loudly if the pid is
unchanged; read it if you want the reference version.

**Then watch the till recover.** After ≤ `HOLLER_SYNC_PUMP_INTERVAL_SECS`
(60s default), the periodic pump drains the outbox. The POS terminal prints
`holler-pos: <phase> drain complete: N published this pass, M row(s) still
pending in local_outbox` (`state.rs:663`).

**What "the banner clears" means, precisely.** The POS's `SyncBlockedBanner`
lists **blocked / persistently-failing outbox rows**, not "cloud is offline".
With the cloud merely stopped, rows are classified transient and **held**, not
blocked — so the banner may well be empty throughout, and stopping the cloud does
not by itself make it appear. Do not script the demo around a banner appearing;
script it around the order **landing** after the restart.

**And what will not happen:** per §0.1, the order replays, but **the KOT, the
invoice and the payment do not** — `edge/sync/src/route.rs` has no route for
them. Separately, `docs/demo-status.md` records that **`apps/admin` has no orders
screen** (three tabs: Menu and pricing, Suppliers, Goods receipts) and no cloud
read route to build one on. So "the order appears in admin" **is not
demonstrable on this build.** Confirm the story wording with whoever is
presenting before Wednesday. Check `docs/demo-status.md` for whether that
decision has since been taken.

---

## 8. Pre-flight checklist — the morning, in order

Run top to bottom. Do not skip a check.

| # | Do | Check |
|---|---|---|
| 1 | Confirm the hub IP (§1) | `Get-NetIPAddress` shows `192.168.137.1` on the hotspot adapter |
| 2 | Confirm the firewall rules (§4) | `Get-NetFirewallRule -DisplayName "Holler demo -*"` lists them, `Enabled` = `True` |
| 3 | Start Docker + backend on the cloud machine (§7.1) | `Invoke-RestMethod "http://<CLOUD_IP>:8080/health"` answers; note its **PID** |
| 4 | Reset to the clean seed: `.\scripts\demo-reset.ps1 -Force` | it prints its four assertions green: zero `sync_replay_block`, zero `stock_deduction_gap`, zero blocked `local_outbox` rows, empty banner proxy. **Needs `$env:HOLLER_DB_KEY_HEX` set to the same key `apps\pos\.env.dev` carries.** Rehearse with `-WhatIf` first |
| 5 | Verify `apps\pos\.env.dev` has all three sync variables and the right `HOLLER_CLOUD_BASE_URL` | open the file; three lines present |
| 6 | Start the POS **from a terminal you own**: `.\apps\pos\run-dev.ps1` (or `.\scripts\dev-up.ps1`) | a **window appears**; terminal prints `KDS LAN server listening on 0.0.0.0:9310` and `sync worker hosted, cloud at …` |
| 7 | Confirm the config pull ran | terminal prints `config pull applied a new bundle` |
| 8 | Confirm both LAN ports are listening | `Get-NetTCPConnection -LocalPort 9310,9320 -State Listen` returns two rows |
| 9 | Start the KDS dev server: `pnpm dev --mode dev --host 0.0.0.0` in `apps\kds` | prints `Local:` / `Network:` URLs including `:5174` |
| 10 | Open the KDS on the second PC at `http://<HUB_IP>:5174` | **no connection banner**; then send a test order from the till and watch the ticket land |
| 11 | Pair the phone at `http://<HUB_IP>:9320/` | reaches the **tables** screen, not the pair screen |
| 12 | Send a test order from the phone | ticket appears on the KDS |
| 13 | Start the admin console: `pnpm dev` in `apps\admin` | loads at `:5175` with no "VITE_ADMIN_… is not set" message |
| 14 | Log in on the till | `cashier@holler.test` / `holler123` (`dev-up.ps1:229`) |
| 15 | Delete the test data: re-run step 4 | assertions green again |

Steps 10, 11 and 12 are the ones that fail on the morning. Do them twice.

---

## 9. When it goes wrong

### The Tauri window never appears, though the process is running

**Symptom:** `Get-Process holler-pos` shows it running, `MainWindowTitle` empty,
nothing on screen.

**Cause:** the POS was launched from a process whose stdio is captured — a
background job, an agent tool, a CI step, a piped command. Observed 2026-08-27.
The window station is still `WinSta0` and `[Environment]::UserInteractive` is
still `True`, so neither of those detects it.

**Fix:** launch it from a terminal you are sitting at. `scripts/dev-up.ps1`
refuses outright when `[Console]::IsOutputRedirected` is true (`dev-up.ps1:115`),
which is the guard for exactly this. Never pipe the launch command into anything.

### A frontend loads blank — POS, KDS, admin or captain

**Symptom:** white screen, and the console often shows nothing useful.

**Cause, in the observed case:** a stale `node_modules/.vite` prebundle.
`optimizeDeps` is a **dev-server** mechanism that `vite build` never reads, so a
green `pnpm build` cannot catch it. Build-green is not dev-works.

**Fix:**

```powershell
Remove-Item -Recurse -Force C:\Code\Holler\apps\pos\node_modules\.vite
```

(same path under `apps\kds`, `apps\admin`, `apps\captain`). Then restart the dev
server. **Check the Network tab, not only the console** — a failed module fetch
shows there and nowhere else. Compare the mtime of `node_modules\.vite` against
the mtime of what it was built from.

### The KDS shows "Disconnected from kitchen system", or the phone says the token was rejected

Work through these in order; each is a different cause with the same symptom.

1. **Is the POS running?** Both listeners live inside it
   (`state.rs:229`, `:260`). No POS = no 9310 and no 9320.
   `Get-NetTCPConnection -LocalPort 9310 -State Listen` on the hub.
2. **Is the hub IP still what the URL says?** Re-run `ipconfig`. A changed
   address breaks the baked URL and nothing repairs it.
3. **Firewall.** `Test-NetConnection -ComputerName <HUB_IP> -Port 9310` from the
   second machine.
4. **The credential has never synced.** This is the most likely cause and the
   least obvious. Neither listener has a cloud fallback (§0.3). Confirm the POS
   terminal printed `config pull applied a new bundle` **after** you enrolled the
   device. If it printed
   `sync worker disabled (… are required together)`, `apps\pos\.env.dev` is
   missing one of the three sync variables and no credential will ever arrive.
5. **Wrong kind.** The captain rejects anything that is not `WAITER`
   (`captain.rs:547`); the KDS verifier is constructed for `"KDS"`
   (`state.rs:782`). A KDS token pasted into the phone fails, and vice versa.
6. **`--mode dev` missing** on the KDS launch: `.env.dev` is not read, and the
   app throws on `VITE_KDS_LAN_URL` rather than rendering a board.

### The admin console shows "Failed to fetch" on every request

**Cause:** `HOLLER_CORS_ALLOWED_ORIGINS` on the backend does not contain the
admin's **exact** origin. The API has no default and emits no CORS headers with
it unset (`config.go:28-35`). `http://localhost:5175` and
`http://192.168.137.1:5175` are different origins.

**Fix:** restart the backend with the right value — and confirm a **new PID**
(§7.3), because a failed rebind leaves the old process serving the old
allowlist.

### The demo reset refuses to run

`scripts/demo-reset.ps1` needs `-Force` (or `-WhatIf`) and needs
`$env:HOLLER_DB_KEY_HEX` set to the **same** key `apps\pos\.env.dev` carries. A
different key opens a different, empty database rather than erroring. Every
failure path in that script names the next action; read the yellow
`NEXT ACTION:` line rather than the stack trace.

### `LNK1104: cannot open file …exe` during a Rust build

McAfee holding the freshly-linked binary, not a code error. Compilation already
succeeded. Re-run — cargo keeps every binary that did link, so two or three
retries reach green. Do not reduce parallelism.

---

## 10. What this document could not establish

Stated here rather than guessed at, because a confidently wrong value on demo
morning is worse than a gap.

- **`192.168.137.1` is not defined anywhere in this repository.** It is the
  address Windows ICS conventionally assigns to a Mobile Hotspot adapter. Verify
  it with `ipconfig` (§1) and substitute whatever you actually see.
- **Whether the Windows Mobile Hotspot adapter can be given a static IP at all.**
  ICS reasserts its own address; this was not tested on the demo hardware. §1b
  covers the physical-router alternative if it turns out to matter.
- **`apps/pos/.env.dev`, `apps/kds/.env.dev`, `apps/kds/.env.dev.example`,
  `apps/admin/.env.example` and `apps/admin/.env.local` could not be read** —
  they are deny-ruled. Their variable names above come from the code that reads
  them (`state.rs`, `lanConfig.ts`, `apps/admin/src/lib/api.ts`) and from the
  writer that generates them (`scripts/dev-bootstrap.ps1`), which is a stronger
  source than the files themselves. But **the current contents of your machine's
  copies are unknown to this document** — open them and check.
- **Whether the demo story's "order appears in admin" step has been re-scoped.**
  `docs/demo-status.md` records it as an open decision with three options put to
  the operator. Check there for the answer before the morning.
