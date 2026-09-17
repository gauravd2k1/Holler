# Holler demo runbook — the one file

**This is the only demo document you need to follow.** It runs from a cold
laptop to a finished demo: hotspot, IP, encryption key, firewall, one command,
device roles, waiter credentials, on-screen checks, what to do when something
looks wrong, and how to stop.

It supersedes, for demo purposes, `docs/demo-script.md`, `docs/lan-setup.md`,
`docs/demo-reset.md`, `docs/demo-wednesday.md` and
`docs/demo-client-walkthrough.md`. Those stay for their detail and their
history; this file is what you read on the day.

**Where this file and a script disagree, the script wins.** Every command below
was read out of the script that runs it, and the file:line is named so you can
check.

---

## 0. The hardware, and who plays what

You have **one or two laptops** and **up to three phones**. Every role below
can move; only the first two are fixed.

| Role | Runs on | Why it can't move |
|---|---|---|
| **The till (POS)** | **Laptop 1, always** | It is a Tauri desktop app and it owns the edge database, the LAN hub (9310) and the captain server (9320). Everything else connects *to* it |
| **The backend + Postgres** | **Laptop 1, always** | Docker containers plus the cloud API on 8080 |
| **The KDS** | Laptop 1 browser, **a phone**, or **laptop 2** | It is a web page served on 5174. It works out the till's address from the address it was served from, so there is nothing to reconfigure |
| **The captain** (waiter ordering) | **Phones** | A web page on 9320, served by the till |
| **The admin console** | Laptop 1 or laptop 2 browser | A web page on 5175 |

### Two sensible layouts

**One laptop, three phones** — the common case:

- Laptop: till + backend + admin console, and the KDS in a second browser window.
- Phone 1: KDS (prop it where the "kitchen" is).
- Phones 2 and 3: captain, one waiter each.

**Two laptops, three phones** — better if you have it:

- Laptop 1: till + backend. Nothing else on screen.
- Laptop 2: KDS full-screen, and the admin console in another tab.
- Phones 1–3: captain, three waiters. (Or keep one spare, charged.)

**The KDS reads better on a laptop than a phone.** If you only have one laptop,
put the KDS on a phone and give the client the till screen — a cramped KDS is
easier to forgive than a cramped till.

---

## 1. Before the day — four things you do once

### 1.1 Mint the edge database encryption key

The edge SQLite is encrypted at rest (ADR-011) and **there is deliberately no
default key** — a default would mean every install ships a database anyone who
read this repository can decrypt. Mint one **once** and reuse it forever on this
machine:

```powershell
# ONCE. Save the output somewhere you can find it on the day.
-join ((1..32) | ForEach-Object { '{0:x2}' -f (Get-Random -Max 256) })
```

64 hex characters. `dev-bootstrap.ps1` and `run-dev.ps1` both validate it and
refuse to run without one.

**If you have already bootstrapped this machine before, the key is already on
disk** — read it rather than minting a new one. A new key means the existing
edge database cannot be opened:

```powershell
Select-String -Path apps\pos\.env.dev -Pattern 'HOLLER_DB_KEY_HEX'
```

> `apps\pos\.env.dev` and `apps\kds\.env.dev` hold the encryption key and the
> device tokens. They are **deny-ruled to agents** — no assistant can read,
> write or pre-run anything that touches them. Those steps are yours.

Keep it in your shell for the session:

```powershell
$env:HOLLER_DB_KEY_HEX = '<your 64 hex chars>'
```

### 1.2 Say which restaurant this is — `seed\outlet.toml`

One file decides the restaurant name on every screen, the GSTIN and address on
the bill, the invoice prefix, the bill footer, the timezone, the business-day
start and **the UPI payee behind the QR**. It is gitignored and
per-installation, and every seeder plus `dev-bootstrap.ps1`, `demo-reset.ps1`
and `demo-up.ps1` **refuse to run without it**. There is no fallback to the
example file.

```powershell
Get-Content seed\outlet.toml          # if this errors:
Copy-Item seed\outlet.example.toml seed\outlet.toml
notepad seed\outlet.toml
```

Check `restaurant_name`, `gstin`, `invoice_prefix` and `upi_vpa` read what you
expect. **If `upi_vpa` is missing there is no QR at all** on the bill screen or
the receipt — there is no empty-QR state and nothing on screen says why.

Onboarding a restaurant is writing that one file. Never editing code.

### 1.3 Open the firewall — elevated PowerShell, once

The phones reach nothing through a closed firewall, and that failure shows up
on the phone, at the demo, and nowhere earlier. `demo-up.ps1` **checks** these
rules and refuses to create them (`scripts/demo-up.ps1:495-507`): creating a
rule needs elevation, and a script that silently opens ports on your laptop is
worse than one that tells you which are shut.

#### What `$subnet` is, and what it is not

`-RemoteAddress` says **which machines are allowed to connect in**, so `$subnet`
must be the network **the phones and the second laptop get their addresses on**
— which is the network of the **hotspot adapter**, not of this machine
generally.

- It is derived from the **hotspot (or demo router) adapter's** IPv4 address —
  the one `lan-ip.ps1` ranks first — with the last octet replaced by `0` and a
  `/24` suffix. Hotspot adapter `192.168.137.1` → `192.168.137.0/24`.
- It is **not your machine's "main" IP**. The WiFi or Ethernet lease
  (`192.168.0.x`, `10.x.x.x`) is the adapter facing your *upstream* network. Put
  that subnet here and every phone on the hotspot is refused, because none of
  them has an address in it.
- It is **not a single host address**. `192.168.137.1/24` or a bare
  `192.168.137.1` allows one machine, not the phones.
- It is **never the WSL/Hyper-V switch** (`172.28.x.x`).

If you are on a physical demo router instead of the hotspot, use **that**
adapter's subnet by the same rule.

**Derive it rather than typing it**, so it cannot disagree with the address the
rest of the run uses:

```powershell
$ip = & .\scripts\lan-ip.ps1 -Bare          # the same ranked pick demo-up uses
$subnet = ($ip -replace '\.\d+$', '.0') + '/24'
$subnet                                      # sanity-check: 192.168.137.0/24
```

**Turn the hotspot on BEFORE you run that.** With the hotspot down there is no
hotspot adapter to rank, so it returns your ordinary WiFi lease and you get a
subnet for the wrong network — which then refuses every phone. Run against a
live hotspot and confirm the value starts `192.168.137.` before using it.

**Run PowerShell as Administrator** (the `$subnet` line below is the literal
form, if you would rather type it):

```powershell
$subnet = "192.168.137.0/24"      # the HOTSPOT ADAPTER's subnet -- see above

New-NetFirewallRule -DisplayName "Holler demo - KDS LAN WS 9310"  -Direction Inbound -Action Allow -Protocol TCP -LocalPort 9310 -Profile Private -RemoteAddress $subnet
New-NetFirewallRule -DisplayName "Holler demo - Captain HTTP 9320" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 9320 -Profile Private -RemoteAddress $subnet
New-NetFirewallRule -DisplayName "Holler demo - KDS page 5174"     -Direction Inbound -Action Allow -Protocol TCP -LocalPort 5174 -Profile Private -RemoteAddress $subnet
```

**A program-path rule works instead of the 9310/9320 port rules**, and is what
earlier runs used. Either satisfies `demo-up`'s check, which matches on the
`Holler demo -*` name. If you use it, it must name the **release** binary's
exact path — a rule bound to the dev-build path, or to `cargo`, or to a path
that moves, is a phone that loads nothing with no error anywhere, because
Windows drops the inbound connection silently:

```powershell
Remove-NetFirewallRule -DisplayName "Holler demo - POS release (KDS 9310, captain 9320)" -ErrorAction SilentlyContinue
New-NetFirewallRule -DisplayName "Holler demo - POS release (KDS 9310, captain 9320)" `
  -Direction Inbound -Action Allow -Program "C:\Code\Holler\apps\pos\src-tauri\target\release\holler-pos.exe" -Profile Private
```

Add these **only if** the admin console or the API is opened from the second
laptop:

```powershell
New-NetFirewallRule -DisplayName "Holler demo - Admin page 5175" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 5175 -Profile Private -RemoteAddress $subnet
New-NetFirewallRule -DisplayName "Holler demo - Cloud API 8080"  -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8080 -Profile Private -RemoteAddress $subnet
```

**Check:**

```powershell
Get-NetFirewallRule -DisplayName "Holler demo -*" | Select-Object DisplayName, Enabled, Direction, Action
```

Three (or five) rows, all `Enabled = True`, `Inbound`, `Allow`.

### 1.4 Prove the release binary builds

`demo-up.ps1 -Release` builds every run and `check-release-binary.ps1` refuses a
stale binary. Do one full run before the day so the first build is not happening
while someone waits. It takes minutes, not seconds.

**Never build the demo binary with `cargo build --release`.** It does not
produce a production Tauri app — it produces a **dev-mode** app in the release
profile, whose window fetches its UI from `http://localhost:5173` instead of the
frontend compiled into it, and shows "can't reach this page" when no Vite is
running. Only `pnpm exec tauri build` embeds the frontend, which is what
`demo-up.ps1` runs (`scripts/demo-up.ps1:873`) and what `scripts\demo-build.ps1`
runs if you build by hand.

This is not hypothetical: a binary was in that state for three sessions while
every check passed — the file existed, it was newer than `dist`, its hash was
recorded twice. `check-release-binary.ps1` is the guard, and it works by
requiring the binary to **contain the current `dist` entry chunk's hashed
filename** — positive evidence that *this* frontend is inside, never the absence
of a dev string.

---

## 2. The hotspot — turning it on and putting devices through it

Everything talks to the till over one network. A phone hotspot from your own
handset works, and a venue's guest WiFi usually does not — guest networks
commonly have **client isolation**, which blocks device-to-device traffic
entirely and looks exactly like a broken app.

### 2.1 Turn on Windows Mobile Hotspot

1. **Settings ▸ Network & internet ▸ Mobile hotspot.**
2. **Share my Internet connection from:** pick your WiFi or Ethernet adapter.
3. **Share over:** **Wi-Fi**.
4. **Edit** — set a network name and a password you can type on a phone under
   pressure. Avoid characters that are awkward on a phone keyboard.
5. **Band:** choose **2.4 GHz** if offered. Longer range, and every phone
   supports it. 5 GHz is faster and you do not need the speed.
6. Turn **Mobile hotspot** on.

**You do not need internet for the demo.** The outlet is offline-first by
design (ADR-013) and the whole story runs on the LAN. If your laptop has no
upstream connection the hotspot may still start; if Windows refuses, share from
any adapter that is up.

### 2.2 Join every device to it

On each phone and on laptop 2: WiFi settings, pick the hotspot name, enter the
password. **Turn mobile data OFF on the phones** — with data on, a phone can
route the till's address out to the internet and fail, and the failure looks
like the app.

Windows shows connected devices under Mobile hotspot. **Count them before the
client arrives.**

### 2.3 Find the address the phones must type

**This is the most expensive silent failure in the system.** `-LanHost` is not
checked against the machine's own addresses, so a wrong value gets written into
the KDS config, every check passes, and the KDS loads, looks completely normal
and never connects — for ever, with no error on either side. It has cost an
evening and a session already.

```powershell
.\scripts\lan-ip.ps1 -Urls
```

That prints every address **ranked**, says what each is for, and gives you the
URLs to type. Use it rather than reading `ipconfig` yourself, because this
machine has several addresses and the wrong ones look plausible:

| Address | What it is | Use it? |
|---|---|---|
| `192.168.137.1` | Windows Mobile Hotspot | **Yes** — this is the one |
| `192.168.0.x` | Your home/office WiFi lease | No. Correct at home, wrong at the venue, and it moves on a DHCP renewal |
| `172.28.176.1` | The WSL/Hyper-V virtual switch | **Never.** No phone on earth can route to it. This is what "just take the first address" picks |

Windows ICS almost always assigns the hotspot `192.168.137.1` and hands clients
`192.168.137.x`. That is Windows behaviour, not something this repository sets.
You generally **cannot** set a static IP on that adapter — ICS reasserts it —
and you do not need to; it is stable for the length of a demo.

Write the address down. The next command needs it.

---

## 3. Starting everything — one command

### 3.1 Close the till first

```powershell
Get-Process holler-pos -ErrorAction SilentlyContinue      # expect nothing
```

A running POS holds the edge database open and owns 5173, 9310 and 9320.
`demo-reset.ps1` refuses while it is up and names the pid — but the refusal
still costs you a restart of the run.

### 3.2 Run it

```powershell
$env:HOLLER_DB_KEY_HEX = '<your 64 hex chars>'

.\scripts\demo-up.ps1 -Release -Fresh -DbKeyHex $env:HOLLER_DB_KEY_HEX -LanHost 192.168.137.1
```

**That is the whole thing.** Do **not** run `docker compose` or
`demo-reset.ps1` yourself first: `-Fresh` already runs
`docker compose up -d postgres redis nats` (`scripts/demo-up.ps1:549`) and
`demo-reset.ps1 -Force` (`:582`). Running either by hand resets the stack twice
and the second reset discards the first.

Ten steps, one checkpoint each, **stopping at the first failure with the reason
and the next action**:

| Step | What |
|---|---|
| 1 | Preflight — ports, firewall rules, `seed\outlet.toml` |
| 2 | Docker: postgres, redis, nats |
| 3 | Reset cloud + edge to the demo seed (`-Fresh` only) |
| 4 | Backend API on 8080, in its own window |
| 5 | Bootstrap — edge seed, env files, POS and KDS credentials |
| 6–9 | Captain build, WAITER enrolment, POS, KDS |
| 10 | The captain URL, the pair token and the checks to make |

**What it prints at the end is what you act on.** The captain URL and the pair
token appear only there, and **the token is written nowhere else**.

### 3.3 Prove the running binary is the one just built

```powershell
Get-Process holler-pos | Select-Object Id, StartTime, @{n='BuiltAt';e={(Get-Item $_.Path).LastWriteTime}}
```

**`StartTime` is not `BuiltAt`.** A process that started a minute ago can be
running a binary from yesterday.

### 3.4 Day-of shortcut

Once you have had three clean runs, swap in `-NoBuild`:

```powershell
.\scripts\demo-up.ps1 -Release -NoBuild -Fresh -DbKeyHex $env:HOLLER_DB_KEY_HEX -LanHost 192.168.137.1
```

It skips the compile only — `check-release-binary.ps1` still runs and still
refuses a stale binary. Use it to save build minutes between the last rehearsal
and the client, **never** after a code change.

---

## 4. Putting each device on its screen

All ports are on the till laptop. Substitute your hotspot address.

| Surface | URL | Sign-in |
|---|---|---|
| **Till** | The Tauri window `demo-up` opened | `cashier@holler.test` / `holler123` |
| **KDS** | `http://192.168.137.1:5174/` | none |
| **Captain** (phones) | `http://192.168.137.1:9320/` | paste the pair token |
| **Admin console** | `http://localhost:5175` (or the LAN address from laptop 2) | `owner@holler.test` / `holler123` |

### 4.1 The KDS, on a phone or the second laptop

Open `http://192.168.137.1:5174/` in the browser. Nothing to configure:
the KDS **derives the till's address from the address the page was served
from** (`apps/kds/vite.config.ts` binds all interfaces for exactly this
reason), so serving it from the hotspot address is what points it at the till.

**Check the indicator reads CONNECTED.** `demo-up` cannot check that for you —
it tests that the LAN port accepts a TCP connection, which is necessary and not
sufficient. "Connected" is a WebSocket the browser opens, and the only place it
is visible is that indicator.

**If it loads but never connects**, the usual cause is a stale
`VITE_KDS_LAN_URL` left in `apps\kds\.env.dev`. The healthy state is for that
line **not to exist**. `demo-up` warns loudly if it is there. Delete the line
and reload.

On a phone: landscape, brightness up, and turn off auto-lock for the demo.

### 4.2 The captain, on each phone

1. Join the hotspot, mobile data off.
2. Open `http://192.168.137.1:9320/`.
3. Paste the pair token. It is stored in `localStorage`, so the phone stays
   paired across reloads.
4. **The phone must leave the pair screen and reach TABLES.** That is the proof
   the credential verified against the till — not merely that the page loaded.

---

## 5. Waiter credentials — minting one per phone, live

`demo-up.ps1` enrols **exactly one** WAITER device and prints **one** token.
That is enough for one phone.

Sharing one token across three phones **does work** and is a legitimate
fallback — but all three are then the same device, and every order reads as
having come from the same waiter. An order's author is recorded from the
credential that created it (`captain.rs` attributes to the resolved
credential's own `device_id`, never to the till and never to anything the
request body claims). Enrolling one each is what makes "Rahul's phone" and
"Priya's phone" separate names on the same screen.

### 5.1 Add another phone

```powershell
.\scripts\add-waiter.ps1 -Name "Rahul's phone"
```

It calls the cloud API and prints a token. **It starts, stops, restarts and
reconfigures nothing** — it is safe to run with the whole stack live and the
client in the room.

Use a person's name. "Panel phone 2" is fine; "WAITER-2" tells you nothing on a
screen three minutes later.

Useful options: `-LanHost` to print the pairing URL with a specific address
(otherwise resolved by the same ranked detector `demo-up` uses), and `-Name` is
the only required one.

### 5.2 The sixty seconds — read this before you debug

The captain listener verifies a token against the edge's **local**
`device_credential_cache`, and that cache is written **only** by the config
pull, which the POS runs at startup and then **every 60 seconds**.

So a phone paired immediately after enrolment sees **"That device token was
rejected"** — the *same message a mistyped token gives* — until the next pull
lands.

- **Enrol every extra phone before the panel arrives.** This is the fix.
- If you must enrol live: enrol, wait, *then* hand the phone over.
- The till's own window prints `config pull applied a new bundle` when it lands.
  That is the only in-product signal.
- **Do not debug it in the first thirty seconds.** It is not broken, it is early.

---

## 6. The checks to make before anyone is watching

`demo-up` prints these; they are repeated here because they are the ones that
matter. **A step whose result you did not look at has not been done.**

1. **The till** — sign in. The outlet name reads your restaurant, not a dev
   fixture name. The menu renders real categories. **The sync banner is ABSENT,
   not empty.**
2. **The QR** — ring up anything, open the bill. "Scan to pay via UPI" with a QR,
   the amount and the payee beneath it. "UPI QR not configured" means the VPA
   never reached the bundle.
3. **The KDS** — the indicator reads **CONNECTED**.
4. **The phone** — it leaves the pair screen and reaches **TABLES**.
5. **The chain** — pick a table, add an item, Send. A ticket appears on the KDS.
   **This is the least-exercised path in the whole demo. Rehearse it properly.**
   It is also the only thing that proves the WAITER device row was minted.

---

## 7. Known on stage — read before the first rehearsal

Everything here is **expected**, has a workaround, and is not a reason to stop a
run or to debug in front of the client. The rule that covers all of it: **carry
on, note it, look afterwards.** Debugging live is what turns one odd screen into
a dead demo.

### The sync banner

**A "kept locally" count is expected and is not an error.** The till keeps
kitchen tickets and stock counts locally because this build has no route to send
them. The banner shows them as a **muted count**, deliberately apart from the
attention list.

- **Muted "N records kept locally"** — expected on every run. If asked: "those
  are kitchen and stock-count records kept on the till."
- **The attention list must be EMPTY.** That is the line that matters.
- **If a red attention line appears: do not retry, do not press anything to
  clear it.** Note the order id and the reason and carry on. The row is already
  blocked and the banner is already telling the truth; a retry changes nothing
  and costs you the room's attention.

### Captain

- **A send error on the phone: check the KDS before re-tapping Send.** The order
  may have reached the kitchen and only the phone's confirmation was lost.
  - Ticket on the KDS → it worked, move on.
  - No ticket → tap Send once more.
  - Two tickets → **void on the till.** The captain has no void and no bill
    screen, deliberately.
- **Every phone needs a fresh pair token after each `demo-up` run.** The WAITER
  credential is rotated on every run by design.

### Billing

- **Never bill a bar item.** Alcohol sits on a zero-rate tax profile — VAT is
  inexpressible under the frozen contract — so a bar line prints a tax figure of
  **0** that is correct and looks wrong. Order food only.

### The till after a KDS bump

The till subscribes to its own kitchen hub, so the status should change **on its
own within a second or two** with the Kitchen panel open and untouched.

- **Alt-tabbing does not test it and does not fix it** (`refetchOnWindowFocus`
  is false).
- **If it does not update on its own:** navigate away from the panel and back.
  Note it; do not debug it live.
- Verify this in rehearsal. If rehearsal shows it stale, end the step on the
  **KDS bump** and do not invite the client to look at the till for confirmation.

### Other things you may see

| You see | On stage |
|---|---|
| An order in admin with a total and **no lines** | Stale rows only; a `-Fresh` reset clears them. Rehearse from a reset |
| The banner reading four problems for one order | Cosmetic, expected. Sized per queued event, not per order |
| A login refused with a correct password | Rate limiting, indistinguishable from a wrong password by design. `demo-up` widens the budget to 50. Wait; do not retype faster |
| A 16×16 placeholder app icon | Expected. Needs artwork |
| The cloud's stock ledger empty right after seeding | **Intended.** The till replays those rows; the cloud does not author them |

### Not in the demo — do not go looking

- **Stock variance in admin is cut.** No cloud read route exists for inventory.
- **Stock is shown on the till**, not in admin.

---

## 8. Stopping, resetting, and starting over

```powershell
.\scripts\demo-down.ps1        # stop everything demo-up started
```

To start clean again, run the same `demo-up` line from section 3.2. `-Fresh` is
what makes it a clean reset; without it, the cloud and edge keep whatever state
they hold.

**Rehearse from a reset every time.** A step that only works on the second
attempt has not passed, and a run not started from a reset proves nothing about
the state the client will see.

---

## 9. When something is wrong

| Symptom | Almost always | Do this |
|---|---|---|
| KDS loads, never connects | Stale `VITE_KDS_LAN_URL`, or wrong `-LanHost` | Delete that line from `apps\kds\.env.dev`; re-run with the address `lan-ip.ps1` ranks first |
| Phone cannot open any URL | Firewall, or phone on mobile data / another network | Section 1.3 rules; turn mobile data off; confirm the phone is on the hotspot |
| Phones refused even though the firewall rules exist | `-RemoteAddress` names the wrong network — usually your WiFi lease's subnet instead of the **hotspot adapter's**, often because the rules were added with the hotspot off | `Get-NetFirewallRule -DisplayName "Holler demo -*" \| Get-NetFirewallAddressFilter` and compare against the phone's own IP. Remove and re-add with the right `$subnet` |
| "That device token was rejected" | The 60-second config pull, **or** a typo | Wait for the till window to print `config pull applied a new bundle`, then retry. Section 5.2 |
| `demo-up` stops at a step | It stopped on purpose | Read the reason and the next action it printed. Do not re-run blindly |
| Till shows no menu / a dev fixture name | `seed\outlet.toml` or the seed did not apply | Re-run with `-Fresh` |
| No UPI QR on the bill | `upi_vpa` missing from `seed\outlet.toml` | Fix the file, re-run |
| Port already in use | A previous run still up | `.\scripts\demo-down.ps1`, then re-run |
| Rust build fails with `LNK1104` | The virus scanner holding a freshly-linked binary | **Re-run.** Cargo keeps every binary that did link; two or three retries reach green |

### The machine itself, before the software

Five settings, none about Holler, every one able to end the demo:

| Do | Check |
|---|---|
| Plug the laptop in | Charging, not on battery |
| **Settings ▸ System ▸ Power** — Screen and Sleep both **Never** while plugged in | Both read `Never`. **A laptop that sleeps drops the hotspot, and the phones and KDS lose the till mid-demo** |
| Do not disturb **on** | No notification can land over the till, the KDS or the captain page |
| Screen recording running on the laptop | It is capturing, and you can see that it is |
| Screen recording running on a phone | The phone is the half no laptop capture can see |

The first three protect the hotspot specifically: Windows Mobile Hotspot turns
itself off when the machine sleeps, and **it does not come back on its own.**

---

## 10. Every port, in one place

| Port | What | Reached from |
|---|---|---|
| 8080 | Cloud API | Till laptop; laptop 2 only if it runs the admin console |
| 9310 | KDS LAN WebSocket (inside the POS process) | KDS device |
| 9320 | Captain HTTP (inside the POS process) | Phones |
| 5174 | KDS page (Vite, binds all interfaces) | KDS device |
| 5175 | Admin console | Laptop browser |
| 5173 | POS Vite dev server | Till laptop only, and **not used** under `-Release` |
