# Demo script

**Work item 7 of `docs/demo-kickoff.md`.** This file is what gets read aloud and
followed click by click on the day.

**STATUS: the day-of checklist below is written and current. THE SIX STEPS ARE
NOT WRITTEN YET** — they are drafted **Monday evening, from the operator's phone
runs**, because each needs its exact clicks, its expected screen and its
fallback written from an observed run rather than from memory. **Tuesday is
rehearsals and the recording only.** Do not read the absence of a step as a step
that passed.

---

## 0. Day-of checklist — in this order, before anything else

Every line has a check you can see. A step whose result you did not look at has
not been done.

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
.\scripts\dev-bootstrap.ps1 -RotateKey -LanHost <hotspot-ip> -DbKeyHex <64-hex-key> `
    -UpiVpa <vpa> -UpiPayeeName "Gong"
```

**`-UpiVpa` only has to be passed ONCE per machine** — the bootstrap remembers
it in the same state file as the device ids and reuses it on every later run.
Pass it again only to change it.

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

> **If `[3c/4]` reports 404:** that is a missing DEVICE, not a missing route.
> A reset drops every `device` row while
> `%LOCALAPPDATA%\Holler\dev-bootstrap-state.json` keeps naming their ids. As of
> 2026-09-12 the bootstrap detects this and enrols fresh; on an older build,
> delete that state file and re-run. See `docs/lan-setup.md` §5.

### 4. Start the POS

```powershell
.\apps\pos\run-dev.ps1
```

From a terminal **you** own: a Tauri window launched from a tool with redirected
stdio never appears.

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
