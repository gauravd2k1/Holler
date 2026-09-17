# Demo script

**Work item 7 of `docs/demo-kickoff.md`.** This file is what gets read aloud and
followed click by click on the day.

**DEMO DAY IS TODAY, 2026-09-17, THIS EVENING. THE TREE IS UNDER CODE FREEZE.**
From the freeze onward nothing lands except a revert or a one-line stage fix
with the operator's explicit approval.

**CUT-OFF: three clean runs from a clean reset before leaving.** Not three runs
— three runs each starting from `demo-up.ps1 -Fresh`. A step that only works on
the second attempt has not passed, and a run that was not started from a reset
proves nothing about the state the client will see.

**STATUS: the day-of checklist below is written and current. THE SIX STEPS ARE
NOT WRITTEN YET** — they are drafted **from the operator's phone runs**, because
each needs its exact clicks, its expected screen and its fallback written from
an observed run rather than from memory. Do not read the absence of a step as a
step that passed.

**Read "Known on stage" below before the first rehearsal.** Everything in it is
expected behaviour with a known workaround; none of it is a reason to stop a
run or to start debugging in front of the client.

---

## 0. Day-of checklist — three lines

Every line has a check you can see. A step whose result you did not look at has
not been done.

### 0.0 The machine, before the software

Five settings, none of which is about Holler and every one of which can end the
demo in front of the client. Do them first, because three of them cannot be
fixed while someone is watching.

| Do | Check |
|---|---|
| **Plug the laptop in.** | Charging, not on battery. |
| **Disable sleep and screen-off** — Settings ▸ System ▸ Power, both "Screen" and "Sleep" to **Never** while plugged in. | Both read `Never`. **A laptop that sleeps drops the hotspot, and the phone and KDS lose the till mid-demo.** |
| **Notifications off** — Focus assist / Do not disturb **on**. | No banner can appear over the till, the KDS or the captain page. Teams, Outlook and update prompts all land centre-screen. |
| **Recording running on the laptop** (item 8's clean run). | It is capturing, and you can see it is. |
| **Recording running on the phone** — screen record before you pick up a table. | The phone is the half no laptop capture can see, and step 1a is the part of the story the client came for. |

The first three also protect the hotspot specifically: Windows Mobile Hotspot
turns itself off when the machine sleeps, and it does not come back on its own.

### 0.0b `seed\outlet.toml` exists — check it ONCE, before the day

**`demo-up.ps1` refuses to run without it**, and it is the file that decides
the restaurant's name on every screen, the GSTIN and address on the bill, the
`SY/` invoice prefix and the UPI payee behind the QR. It is gitignored and
per-installation, so a fresh clone does not have one.

```powershell
Get-Content seed\outlet.toml       # if this errors, copy the template:
# Copy-Item seed\outlet.example.toml seed\outlet.toml   then edit it
```

Check: `restaurant_name`, `gstin`, `invoice_prefix` and `upi_vpa` read what you
expect. **If `upi_vpa` is missing there is NO QR at all** on the bill screen or
the receipt — there is no empty-QR state and nothing on screen says why, and
demo step 2 shows the QR.

Every field is validated when `demo-up` runs and a failure names the field, so
a typo stops the run before anything is reset rather than surfacing on a bill
in front of the client.

### 0.0c The release binary and the firewall rule

**The demo runs the RELEASE binary, and the firewall rule names its exact
path.** A rule bound to the dev-build path, or to `cargo`, or to a path that
moves, is a phone that loads nothing with no error anywhere — Windows drops the
inbound SYN silently.

```
C:\Code\Holler\apps\pos\src-tauri\target\release\holler-pos.exe
```

Built 2026-09-15 **16:29 IST** with `scripts\demo-build.ps1`.
SHA-256 `810dc5a7553a973d11fb4c4c16248a2e9788f28537bdfa76df1cefef4729c389`.

**Build it with `scripts\demo-build.ps1`, never `cargo build --release`** --
cargo produces a dev-mode binary whose window loads `localhost:5173` and shows
"can't reach this page". Verify with `scripts\check-release-binary.ps1`.

**The hash identifies the FILE, not the source.** This binary was relinked at
19:01 from a different commit than the 09:29 one and hashed differently; an MSVC
link is not reproducible. The source-identity check is the `git diff
--name-only` command in `docs/RESUME.md`, not this digest.

**The POS process listens on exactly two ports**, both enumerated from the
source rather than from memory — there is no third:

| Port | What | Where |
|---|---|---|
| **9310** | KDS LAN WebSocket | `edge/device/src/server.rs:188`, started at `state.rs:231` (`HOLLER_LAN_BIND_ADDR`) |
| **9320** | Captain HTTP (page + `/api/`) | `captain.rs:81`, started at `state.rs:264` (`HOLLER_CAPTAIN_BIND_ADDR`) |

5173 and 5175 are **dev servers** and are not part of a release run: the release
binary serves its own embedded UI.

**Starting it is `-Release`, not by hand.** `.\apps\pos\run-dev.ps1 -Release`
and `.\scripts\demo-up.ps1 ... -Release` both launch this exact binary and
print the path they resolved.

**THEY ARE NOT THE SAME ABOUT BUILDING IT.** `demo-up.ps1 -Release` REBUILDS
the frontend and the binary at step 6 every run, then checks the binary
contains the dist it just built. `run-dev.ps1 -Release` does NOT build — it
launches what is already there and only checks the binary is newer than
`dist`, which both being stale together passes. A POS started at 11:39 on
2026-09-16 was running a 00:40 build and looked entirely normal. **`BuiltAt`,
not `StartTime`, is the version you are looking at:**

```powershell
Get-Process holler-pos | Select-Object Id, StartTime, @{n='BuiltAt';e={(Get-Item $_.Path).LastWriteTime}}
```

<details>
<summary>FALLBACK ONLY — launching it by hand, if those scripts cannot run</summary>

The env parse below is `apps\pos\run-dev.ps1`'s own, copied, so the process
gets exactly the environment a script launch gives it. Prefer the switch: a
hand-assembled environment is one forgotten variable away from a till that
starts and then behaves oddly, and this copy cannot check that the binary is
newer than `apps\pos\dist`.

```powershell
cd C:\Code\Holler
Get-Content apps\pos\.env.dev | ForEach-Object {
  $t = $_.Trim()
  if ($t -and -not $t.StartsWith("#") -and $t -match '^([A-Za-z_][A-Za-z0-9_]*)=(.*)$') {
    Set-Item -Path "Env:\$($Matches[1])" -Value $Matches[2].Trim()
  }
}
.\apps\pos\src-tauri\target\release\holler-pos.exe
```

From a terminal **you** own — a Tauri window launched with redirected stdio
never appears. The data directory is the same either way: it comes from
Tauri's `app_data_dir()` for identifier `com.holler.pos`, so both profiles
open `%APPDATA%\com.holler.pos\edge.db.enc`. There is no separate release
database.

</details>

**One elevated PowerShell command** (Run as administrator), idempotent — it
removes any rule of the same name first, so re-running after a rebuild is safe:

```powershell
Remove-NetFirewallRule -DisplayName "Holler demo - POS release (KDS 9310, captain 9320)" -ErrorAction SilentlyContinue
New-NetFirewallRule -DisplayName "Holler demo - POS release (KDS 9310, captain 9320)" `
  -Direction Inbound -Action Allow -Protocol TCP -LocalPort 9310,9320 `
  -Program "C:\Code\Holler\apps\pos\src-tauri\target\release\holler-pos.exe" `
  -Profile Private,Public
```

`-Profile Private,Public` is deliberate: Windows classifies a Mobile Hotspot
adapter as **Public**, and a rule left on Private only is a chain that works on
home WiFi and fails on the hotspot — the single most demo-specific failure on
this list. Domain is excluded because no demo network is domain-joined.

**Check, and it is not "the rule exists":**

```powershell
Get-NetFirewallRule -DisplayName "Holler demo - POS release (KDS 9310, captain 9320)" |
  Get-NetFirewallApplicationFilter | Select-Object Program
```

The path it prints must be **character for character** the path above. Then,
with the POS running, confirm the binary answering is the one in the rule:

```powershell
Get-Process holler-pos | Select-Object Id, Path, StartTime
```

A `Path` under `target\debug` means the dev build is running and the rule does
not cover it.

**CHECKED 2026-09-14 19:05 IST, AND THE RULE IS NOT THERE.** `Get-NetFirewallRule`
returns **zero** enabled inbound rules whose program is the release binary, and
no rule by the name above exists at all. What IS present is a pair of enabled
inbound rules called `Holler POS` (TCP and UDP, Private+Public, all local
ports) naming **`target\debug\holler-pos.exe`** — the dev build, which a
`-Release` run does not start. So on the release binary today the phone and the
KDS are both blocked, and Windows will drop the inbound SYN with nothing on any
screen saying why.

**Run the elevated command above before the first rehearsal.** It needs Run as
administrator, so it is the operator's to run; re-run the check afterwards and
read the Program line rather than trusting that the rule exists.

**Rebuild = re-check.** A new release binary at the same path keeps the rule
valid; a binary anywhere else does not, and the failure is silent.

### 0.1 Then the three lines

1. **Hotspot up**, and note its IP (section 0.2 below).
2. ```powershell
   .\scripts\demo-up.ps1 -DbKeyHex <64-hex-key> -LanHost <hotspot-ip> -Fresh -Release
   ```
3. **The phone**: join the hotspot, open the captain URL `demo-up` printed, paste
   the pair token it printed.

#### 0.1a The full build + seed sequence, in order

**Do NOT run `docker compose` or `demo-reset.ps1` yourself before this.**
`demo-up.ps1 -Fresh` runs **both** — `docker compose up -d postgres redis nats`
at step `[2/10]` (`scripts/demo-up.ps1:549`) and `demo-reset.ps1 -Force` at step
`[3/10]` (`:582`). Running either by hand first resets the stack **twice**: it
costs several minutes and the second reset discards the first.

```powershell
# 1. Hotspot up first -- the till's LAN address is read from whatever network
#    exists when the bootstrap runs, and written into apps\kds\.env.dev.

# 2. The edge key. Yours; apps\pos\.env.dev is deny-ruled to agents.
$env:HOLLER_DB_KEY_HEX = '<64-hex-key from apps\pos\.env.dev>'

# 3. Close the till, or demo-reset refuses and names the pid.
Get-Process holler-pos -ErrorAction SilentlyContinue    # expect nothing

# 4. THE ONE COMMAND. Brings up docker, resets cloud + edge, bootstraps,
#    builds the captain page, enrols the WAITER device, starts POS and KDS.
.\scripts\demo-up.ps1 -Release -Fresh -DbKeyHex $env:HOLLER_DB_KEY_HEX -LanHost <hotspot-ip>

# 5. Prove the RUNNING binary is the one just built. StartTime is not BuiltAt:
#    a process started a minute ago can be running a binary from yesterday.
Get-Process holler-pos | Select-Object Id, StartTime, @{n='BuiltAt';e={(Get-Item $_.Path).LastWriteTime}}

# 6. The phone: join the hotspot, open the captain URL demo-up printed,
#    paste the pair token it printed. A NEW token every run -- the WAITER
#    credential is rotated deliberately.

# 7. The KDS: demo-up starts it. Confirm a ticket lands on it before the client
#    is in the room, not during step 1.
```

**Day-of variant, once the three clean runs are done:** swap `-Release` for
`-Release -NoBuild`. It skips the compile only — `check-release-binary.ps1`
**still runs** and refuses a stale binary, so `-NoBuild` cannot serve yesterday's
UI by accident. Use it to save the build minutes between the last rehearsal and
the client, never to skip a rebuild after a code change.

**What this build contains, confirmed against HEAD:**

| Fix | In HEAD? | Where |
|---|---|---|
| Sync banner split — muted "kept locally" count apart from the attention list | **YES** | `apps/pos/src/components/SyncBlockedBanner.tsx:194` (`b328f21`, `bece32f`) |
| Fix 5 — the cloud seeds no ledger rows, so the till's replay does not 409 | **YES** | `ca9ac49`, `aa79396` |
| **D14 — the till's stale kitchen status** | **YES, but NEVER VISUALLY VERIFIED** | `97bc3dc`, three parts. Live update is a LAN hub subscription forwarded as a Tauri event (`apps/pos/src/lib/kitchenEvents.ts`), **not** a polling interval — so grepping for `refetchInterval` finds nothing and proves nothing. VV-012/013/014 are the rows that close it |

`scripts\demo-up.ps1` is the one command. It runs the preflight, the reset
(`-Fresh`), the backend, the bootstrap, the captain build, the WAITER
enrolment, the POS and the KDS — one checkpoint per step, **stopping at the
first failure with the reason and the next action** — and finishes by printing
the captain URL, the pair token and the four checks to make on screen. Stop all
of it with `.\scripts\demo-down.ps1`.

Three things it does that are easy to undo by hand and expensive to get wrong:

- **It starts the POS itself, and that is the ONLY Vite.** Never run `pnpm dev`
  for the POS in another terminal first — `tauri.conf.json`'s `beforeDevCommand`
  starts Vite already, `vite.config.ts` sets `strictPort`, and a stale Vite
  serves a bundle built in a different environment (that is how the UPI QR went
  missing from the bill screen on 2026-09-12).
- **It rotates the WAITER credential on every run**, so the phone needs the new
  token each time. That is deliberate: it is what makes S-CAP-20 — a device
  paired before the fix staying permanently broken — structurally impossible.
- **It starts the backend with `PORT` set and the login budget widened to 50.**
  A throttled login is indistinguishable from a wrong password by design
  (ADR-012), and the demo signs in from three surfaces on one laptop.

**`-DbKeyHex` is yours** — `apps\pos\.env.dev` is deny-ruled to agents and no
agent supplies a literal key.

**The UPI payee comes from `seed\outlet.toml` now, not from `-UpiVpa`.** The
flags still work as a one-run override, print a warning and are **not** written
back — so a demo run that overrides the payee reverts on the next run. Change
it in the file.

Section 0.2 below is the **manual sequence**, which is both the
fallback when `demo-up` fails at a step and the explanation of what each step is
checking.

---

## 0.2 Manual sequence — in this order

### 1. Hotspot up

Start Windows Mobile Hotspot (or the demo router) **before** the bootstrap. The
till's LAN address comes from whatever network exists when the bootstrap runs,
and it is written into `apps\kds\.env.dev` as a literal string.

**Check:** the phone can see the network in its WiFi list.

### 2. Note the hotspot's IP

```powershell
Get-NetIPAddress -AddressFamily IPv4 |
  Where-Object { $_.IPAddress -ne '127.0.0.1' -and $_.IPAddress -notlike '169.254.*' } |
  Select-Object IPAddress, InterfaceAlias
```

**Check:** you can name which row is the hotspot — typically `192.168.137.1` on
an adapter called `Local Area Connection* N`. Write it down; the next command
needs it.

**Never assume.** On this machine the first non-loopback address is
`172.28.176.1`, the WSL Hyper-V switch, which no phone can route to. The
bootstrap now picks the default-route interface instead and prints which adapter
it chose — but on a hotspot you pass the address explicitly anyway, because a
hotspot's address changes every time it reconnects.

### 2b. Close the POS

**Before the bootstrap, not during it.** Step `[3/4]` seeds the edge database,
which a running POS holds open: it fails with `os error 32` **after** steps 1
and 2 have already reseeded the cloud, leaving a half-applied run.

Both `dev-bootstrap.ps1` and `demo-reset.ps1` now refuse at the top and name
the pid, so the cost of forgetting is a refusal rather than a reseed — but the
refusal still costs you a restart of the run, and the till takes a minute to
come back up.

**Check:** `Get-Process holler-pos` returns nothing.

### 3. Bootstrap

```powershell
.\scripts\dev-bootstrap.ps1 -LanHost <hotspot-ip> -DbKeyHex <64-hex-key> `
    -WithBilling -PrinterFileSinkDir .dev-prints `
    -UpiVpa <vpa> -UpiPayeeName "Shinjuku Yakitori"
```

**`-WithBilling` IS NOT OPTIONAL, and leaving it off is silent.** It is the only
thing that sets `HOLLER_SEED_BILLING=1`, and `seed_billing` is the only writer of
`outlet_fiscal_profile` — the legal name, address, GSTIN, FSSAI and footer
printed on every bill. A run without it seeds the menu happily and leaves the
bill header at whatever generation it was last written in. That cost an evening
on 2026-09-13: the header kept printing a previous name, from a database that
had just been re-seeded and re-sealed, because only this flag was missing.

**`-UpiVpa` and `-PrinterFileSinkDir` only have to be passed ONCE per machine** —
the bootstrap remembers both in the same state file as the device ids and reuses
them on every later run. Pass either again only to change it. Before the sink was
remembered, a re-run without it **deleted** the line, and a print then produced
no `.escpos`, no `.html` and no `.pdf` at all, with nothing on screen saying why.

`-RotateKey` is deliberately **not** here: it is only for changing the encryption
key, and the existing sealed database was encrypted under the current one.

`-DbKeyHex` is the key from `apps\pos\.env.dev`. It is **yours** — that file is
deny-ruled to agents, and no agent may supply a literal key.

**Check, four lines in the output:**

- `LAN address <hotspot-ip> on '<adapter>'` — and the adapter is the hotspot,
  not `vEthernet`/`WSL`/`Wi-Fi`.
- `[3b/4]` POS sync credential **enrolled or rotated** — not skipped.
- `[3c/4] KDS credential ENABLED (token written to apps\kds\.env.dev only)` —
  in cyan. **A red `[3c/4] ... SKIPPED` means the KDS will throw at startup**
  and must be fixed before step 5.
- `VITE_KDS_LAN_URL=ws://<hotspot-ip>:9310/kds` in the written env file — the
  hotspot IP, never `localhost` and never the WSL address.
- **`UPI QR ENABLED: <vpa>`** in cyan. A yellow `UPI QR DISABLED` means the
  invoice screen and the printed receipt will show **no QR at all** — there is
  no empty-QR state and nothing on screen says why, so this line is the only
  warning you get before demo step 2.
- **`billing config seeded: bills can be issued, discounted and split on this
  machine.`** Its absence means `-WithBilling` was missing and **the bill header
  is stale** — the name, address and GSTIN on the invoice will be whatever they
  were at the last run that carried the flag.
- **`printer FILE SINK: every print will be written to <dir>`** in yellow. A red
  `printer FILE SINK: DISABLED` means a print will write no file of any kind.

> **If `[3c/4]` reports 404:** that is a missing DEVICE, not a missing route.
> A reset drops every `device` row while
> `%LOCALAPPDATA%\Holler\dev-bootstrap-state.json` keeps naming their ids. As of
> 2026-09-12 the bootstrap detects this and enrols fresh; on an older build,
> delete that state file and re-run. See `docs/lan-setup.md` §5.

### 4. Start the POS — ONE command, and only this one

```powershell
.\apps\pos\run-dev.ps1 -Release
```

**`-Release` is the demo form, and it is not a preference.** The inbound
firewall rule names one exact program path (§0.0c), and `target\debug` is not
it — a rehearsal on the debug build proves nothing about the rule the phone
depends on, and a blocked inbound connection produces no error anywhere. It
also skips Vite and the 5173 guard entirely: the release binary serves its own
embedded frontend, so a dev server left running beside it is unrelated rather
than a conflict.

It **refuses** if the binary is missing, and refuses if the binary is OLDER
than `apps\pos\dist` — that frontend is embedded at link time, so rebuilding
dist alone changes nothing in the window and would otherwise be invisible.
`apps\captain\dist` is deliberately not checked: the captain page is served
from disk per request, so a captain rebuild needs no relink.

Both forms print the resolved build on start — `build  : RELEASE -- <path>` or
`build  : DEBUG (tauri dev, Vite on 5173)`. Read that line rather than assuming
which one is running.

**Without `-Release`** (a debug run, for development, not for the demo):

```powershell
Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue   # must return nothing
.\apps\pos\run-dev.ps1
```

From a terminal **you** own: a Tauri window launched from a tool with redirected
stdio never appears.

**Do not start `pnpm dev` first.** `run-dev.ps1` starts Vite itself
(`tauri.conf.json`'s `beforeDevCommand`), and `vite.config.ts` sets
`strictPort`, so a Vite started by hand makes the launch fail on 5173.
Corrected 2026-09-12: the bootstrap used to print a two-terminal sequence and
`run-dev.ps1` used to claim it would reuse a running Vite — both wrong, in the
same direction.

**If 5173 is already held**, `run-dev.ps1` refuses and names the pid. Stop that
process and re-run. This matters beyond the port clash: a Vite left from an
earlier shell serves a bundle built without the current `.env.local`, which is
exactly how the UPI QR went missing from the bill screen.


**Check:** the till window opens, sign in as `cashier@holler.test` /
`holler123`, the client menu renders with real categories, and **the sync banner
is absent** — not empty, absent.

**Check the QR before the rehearsal, not during it.** Ring up anything, open
the bill, and confirm **"Scan to pay via UPI"** with a QR under it and the
amount and payee beneath. No QR means the VPA did not reach
`apps\pos\.env.dev` — go back to step 3. This is worth thirty seconds
because the failure is silent: the screen looks finished without it.

### 5. Start the KDS

Per `docs/lan-setup.md` §5, on the KDS machine or a second browser.

**Check:** the connection indicator reads **connected**, against
`ws://<hotspot-ip>:9310/kds`. A KDS that loads but never connects is almost
always a stale `VITE_KDS_LAN_URL` from a previous network — go back to step 2.

### 6. The phone

Join the hotspot, open the captain page, pair with the WAITER token.

**Check:** the Tables screen appears after pairing.

> **The WAITER device must have been enrolled AFTER 2026-09-11 20:28.** The
> device-row fix has **no backfill**: a device paired before it stays broken and
> re-pairing does not help — it must be re-ENROLLED. (Scenario board S-CAP-20.)

> **ON A SEND ERROR, CHECK THE KDS BEFORE TAPPING SEND AGAIN.** The listener may
> have accepted the order and lost only the reply — the commonest shape of a
> WiFi drop mid-send — in which case the kitchen already has the round and a
> second tap cooks it twice. A duplicate ORDER can no longer happen (`52d8930`:
> the table now reports its open order, so a retry appends), but duplicate
> LINES still can, because no request carries an idempotency key. **Wrong line
> on the ticket: void it on the till.** The real fix is `client_order_id` on
> both write routes and is filed in `docs/pilot-readiness.md` §0 for before the
> first pilot — deliberately not done before the demo.

---

## Known on stage — read before the first rehearsal

Everything here is **expected**, has a workaround, and is not a reason to stop a
run or to debug in front of the client. The rule that covers all of it: **carry
on, note the row, look afterwards.** Debugging live is what turns one odd screen
into a dead demo.

### The sync banner

**A "kept locally" count is EXPECTED and is not an error.** The till keeps
kitchen tickets and stock counts locally because this build has no route to send
them (A7 — `edge/sync/src/route.rs` maps only `order` and `table_session`). The
banner shows them as a **muted count**, deliberately apart from the attention
list. Nothing is lost and nothing is broken.

- **Muted "N records kept locally"** — expected on every run. Say "those are
  kitchen and stock-count records kept on the till" if asked. Carry on.
- **The attention list must be EMPTY.** That is the line that matters.
- **If a red attention line appears: DO NOT RETRY, and do not press anything to
  clear it.** Note the order id and the reason, carry on with the next step, and
  hand the note over afterwards. A retry in front of the client spends time and
  changes nothing — the row is already blocked and the banner is already telling
  the truth.

### Captain (the phone)

- **A send error on the phone: check the KDS BEFORE re-tapping Send.** The order
  may well have reached the kitchen and only the phone's confirmation was lost.
  Re-tapping blind is how one order becomes two tickets in front of the client.
  - Ticket on the KDS → the send worked. Move on.
  - No ticket → tap Send once more.
  - Two tickets → **void on the till**, not on the phone. The captain has no
    bill screen and no void (deliberate, reduced scope).
- **The phone needs a fresh pair token after EVERY `demo-up` run.** The WAITER
  credential is rotated on every run by design. An old token fails to pair; that
  is not a defect (D15 is the reason the rotation exists).

### Billing

- **NEVER bill a bar item.** Alcohol sits on a **zero-rate** tax profile — VAT
  is inexpressible under the frozen contract — so a bar line prints a tax figure
  of **0** that is correct and looks wrong. Order food only. This is a content
  rule, not a bug to fix tonight.

### The till after the kitchen bumps a ticket — D14, FIXED BUT NEVER SEEN

**D14 is fixed in this build (`97bc3dc`) and has never been observed in the
Tauri release window.** The till subscribes to its own LAN kitchen hub and the
Kitchen panel invalidates on each frame (`apps/pos/src/lib/kitchenEvents.ts`) —
deliberately **not** a polling interval, so there is no interval to wait out.
The open-defect register still says D14 is OPEN; it was compiled about two hours
before the fix landed and is stale on this row.

**Expected:** with the Kitchen panel open and untouched, bumping on the KDS
changes the till's status **on its own, within a second or two**.

- **Alt-tabbing does not test it and does not fix it.** `refetchOnWindowFocus`
  is false, so a focus change refetches nothing.
- **If the status does NOT update on its own:** remount the panel — navigate
  away and back — and carry on. Note it; do not debug it live.
- **Verify this in rehearsal, not on stage** (VV-012/013/014). If rehearsal
  shows it stale, end step 1 on the **KDS bump** and do not invite the client to
  look at the till for confirmation.

### Other register rows a step can touch

| Row | What you would see | On stage |
|---|---|---|
| **D12** | An order in admin with a total and **no lines** | Cause fixed; only stale rows show it. A clean `-Fresh` reset clears them — which is why the cut-off is three runs *from a reset* |
| **D21** | The banner sized per outbox row, so one order with four queued events reads as four problems, taking ~28% of the window | Cosmetic, expected. Do not resize anything |
| **D16** | A login that fails with a correct password | Rate limiting, indistinguishable from a wrong password **by design** (ADR-012). `demo-up` widens the budget to 50. Wait, do not re-type faster |
| **D23** | The POS icon is a 16×16 placeholder | Expected. Needs artwork |
| **D22 / A6** | — | No exit path seals the edge database, so a plaintext `edge.db` is left beside the `.enc` on close. Harmless tonight; do not take a backup and assume it is current |
| **D18** | The cloud seeder failing loudly on a variant index | Only if a seed change ships without `-Force`. It fails loudly rather than silently, which is why it is not a blocker |

### What is NOT in the demo, so do not go looking

- **Stock variance in admin is CUT** (ruled 2026-09-12): no cloud read route
  exists for inventory. Step 5 is the **Orders tab and the received GRN**, those
  two only.
- **Stock for step 3 is shown ON THE TILL**, not in admin.
- The cloud's ledger is **empty until the till replays into it** — that is the
  intended state after `ca9ac49`, not a missing seed.

---

## Rules that bind anyone touching this stack before the demo

- **No test or probe starts, stops or binds anything on 8080, 9310, 9320, the
  admin port (5175) or the POS dev port (5173).** Scratch ports and scratch
  databases only. A test displaced the running backend three times on
  2026-09-12, and a leftover dev server held 5175 until the operator met it.
- **`demo-reset.ps1` refuses to run while the POS is open**, by design, and
  names the pid. Close the till first.
- **Verify any restart by NEW PID, never by the port answering.** The old
  process answers identically.

---

## 1–6. The six demo steps

**NOT WRITTEN YET — item 7, Tuesday.** The story they will cover is fixed
(`docs/demo-kickoff.md`):

1. Dine-in order at the till with a modifier → KOT on the KDS → kitchen bumps
   it. **1a:** an order placed from the waiter's phone reaches the KDS and the
   hub.
2. Bill: GST invoice, split cash + UPI, receipt printed to PDF and opened.
3. Stock screen: ingredients deducted per recipe from that sale; low-stock
   warning visible.
4. Stop the cloud. Take and bill another order. Restart the cloud. Banner
   clears, order appears in admin.
5. Admin: **the Orders tab (contracts 0.8.2) and the received GRN — those two,
   and nothing else.** Stock variance is CUT, ruled 2026-09-12: no cloud read
   route exists for inventory (`/inventory/*` are all POST-only), so a variance
   screen would need new OpenAPI paths.
6. Optional, only if clean three times: a fake ONDC order arrives, is accepted
   on the till, and is billed.

Each one needs, written from an observed run: the exact clicks, the screen to
expect, and the fallback if it does not appear. **Drafted MONDAY EVENING from
the operator's phone runs** — Tuesday is rehearsals and the recording only. **Do not bill a bar item** —
alcohol sits on a zero-rate tax profile (VAT is inexpressible under contracts
0.8.2), so a bar line would print a tax figure that is deliberately 0.
