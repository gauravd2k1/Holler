# Wednesday — the distributed demo at the client's restaurant

**One shot. This file is the whole plan: what is true, what to run, what to
say, and what will go wrong.**

It supersedes nothing. `docs/demo-script.md` is still the day-of checklist for
bringing the stack up; this is the demo that runs on top of it, with more
devices and an audience.

---

# PART 0 — READ THIS FIRST: what is real and what is not

You are going to be asked, in a room, to justify a month of spend. The fastest
way to lose that room is to promise something on stage that the product does
not do and be caught. The fastest way to win it is to show something genuinely
hard working in front of them and be honest about the edges.

**So here is the honest inventory, in plain English. No jargon.**

## What is genuinely built, and genuinely impressive

| In plain English | Why it is actually hard |
|---|---|
| **The restaurant keeps working when the internet dies.** Orders, bills, GST invoices, stock — all of it runs off a file on the till itself. | Most restaurant software is a website. No internet, no billing. Holler treats the cloud as optional and the outlet as the source of truth. |
| **Everything catches up by itself when the internet returns.** Nothing is typed twice, nothing is lost. | Every order carries its own identity and replays in order. The hard part is not sending — it is knowing what has and has not arrived, and not sending it twice. |
| **A waiter's phone becomes a terminal in about ten seconds.** No app store, no install. | The phone loads a page served by the till itself, over the restaurant's own network. It keeps working with the internet down, because it never needed the internet. |
| **The kitchen screen is live.** A ticket appears the moment it is sent, and a cook bumping it is recorded. | A persistent connection from the till to each kitchen screen, with the till as the only thing allowed to change a ticket's state. |
| **The bill is a legally correct GST invoice.** Correct tax split, HSN codes, sequential numbering, UPI QR. | Tax is computed once, in one place, in whole paise. The screen and the printed bill cannot disagree, because neither of them does the arithmetic. |
| **Money is never a decimal.** Every amount is whole paise, end to end. | Floating-point money is how a chain loses lakhs invisibly. This is a deliberate structural choice, not a detail. |
| **Stock falls as food is sold**, by recipe, down through sub-recipes. | Selling one dish deducts eleven ingredients at the right quantities. A missing recipe never blocks a sale — it is recorded and reported instead. |
| **Onboarding a restaurant is writing one file.** Name, GSTIN, address, invoice prefix, UPI ID, logo. No code changes. | This is the difference between a product and a project. Say it out loud. |

## What is NOT built — know these cold, because a sharp panel will find them

**Do not volunteer these. Do not lie about them either. If asked, answer in one
sentence and move on — every one of them has a real answer.**

1. **The waiter's phone cannot take payment or print a bill.** It takes orders.
   Billing is on the till. *If asked:* "Deliberate — money goes through one
   screen with one person accountable. Phone-side payment is a separate
   decision with its own controls."
2. **The phone only offers free choices** (spice level, no onion), not paid
   add-ons. *If asked:* "Anything that changes the price goes through the till,
   so the bill is never a surprise. Paid modifiers on the phone are a small
   extension, not a rebuild."
3. **Alcohol is seeded at zero tax.** The client's card is a bar menu and VAT
   is not yet expressible in the system. **Do not bill a bar item.** *If asked:*
   "GST is done; state VAT on liquor is the next tax module."
4. **Only orders reach the cloud today.** Kitchen tickets, invoices and stock
   counts stay on the till. The back office shows orders and received stock.
   *If asked:* "The outlet is the source of truth by design; we are widening
   what replays upward, one stream at a time."
5. **There is no printer here.** Bills render to PDF and open on screen.
   *If asked:* "Same bytes a thermal printer gets — we simply don't have one in
   the room."
6. **The back office cannot edit stock or show variance yet.** It shows orders,
   menu, suppliers and received goods.
7. **A closed till leaves its database unsealed.** Internal; never mention it.
   It does not affect anything they will see.

## The one thing that will kill the demo if you skip it

**Everything the phones and the second laptop touch goes over a network you
control.** Use **your laptop's Mobile Hotspot**, never the restaurant's WiFi.
Their WiFi will have client isolation, a captive portal, or a guest VLAN that
silently blocks device-to-device traffic — and the failure looks exactly like
your software being broken.

If the laptop cannot host a hotspot, a cheap travel router with nothing plugged
into its WAN port is the fallback. Both are fine. The restaurant's WiFi is not.

---

# PART 1 — THE CAST

| Device | Whose | Runs | Reaches it at |
|---|---|---|---|
| **The till** | Your laptop | Holler POS, release build | the screen in front of you |
| **Kitchen screen** | Second laptop, or a phone in landscape | KDS page | `http://<LAN-IP>:5174/` |
| **Captain 1** | Your phone | Captain page | `http://<LAN-IP>:9320/` |
| **Captain 2** | Panel member's phone | Captain page | `http://<LAN-IP>:9320/` |
| **Captain 3** | Panel member's phone | Captain page | `http://<LAN-IP>:9320/` |
| **Back office** | Second laptop, second tab | Admin console | `http://<LAN-IP>:5175/` |

`<LAN-IP>` is your laptop's hotspot address. You will write it on something
before you start. **Every device joins your hotspot first. No exceptions.**

Android and iPhone both work — it is a web page, not an app. Safari and Chrome
both work.

---

# PART 2 — WHAT TO RUN, IN ORDER

## The night before (Tuesday) — not on the day

### T-1. Build the release binary and the web bundles

```powershell
cd C:\Code\Holler
.\scripts\demo-build.ps1
```

One command. It builds captain, the KDS, the back office and the POS, in that
order, and then **proves the binary carries its UI** before saying it is done.

**Never `cargo build --release`.** That produces a DEV-MODE binary in the
release profile: the window loads `http://localhost:5173` instead of the UI
inside it, and shows "can't reach this page". Every release binary before
2026-09-15 01:01 was built that way and none of them could draw a window. The
build script refuses to finish on such a binary; so does `-Release`.

It also refuses up front if the POS is running, because the till holds its own
.exe open and the failure otherwise lands several minutes into the compile.

**The binary this plan was rehearsed against:**

```
C:\Code\Holler\apps\pos\src-tauri\target\release\holler-pos.exe
```

Built 2026-09-15 **16:29 IST**, SHA-256 `810dc5a7553a973d11fb4c4c16248a2e9788f28537bdfa76df1cefef4729c389`.
`scripts\check-release-binary.ps1` passed on it: *OK -- the release binary carries this
frontend.*

**The hash identifies the FILE, not the source.** An MSVC link is not
reproducible, so rebuilding from the same commit produces a different hash.
It tells you whether the .exe on disk is the one that was checked, and
nothing more. If it differs, re-run `scripts\check-release-binary.ps1` rather than assuming the
binary is wrong.

### T-2. The firewall rule — ONE elevated command, and it is not optional

The phones and the second laptop reach four ports on your laptop. Windows
blocks all of them by default, **silently** — a blocked connection produces no
error anywhere, on either side. It looks precisely like your software hanging.

Run **PowerShell as Administrator**:

```powershell
Remove-NetFirewallRule -DisplayName "Holler demo" -ErrorAction SilentlyContinue
New-NetFirewallRule -DisplayName "Holler demo" `
  -Direction Inbound -Action Allow -Protocol TCP `
  -LocalPort 9310,9320,5174,5175 `
  -Profile Private,Public
```

- **9310** kitchen screen's live connection · **9320** captain page
- **5174** kitchen screen page · **5175** back office page

`Public` is there because **Windows classifies a Mobile Hotspot as a public
network**. A rule left on Private only works on your home WiFi and fails at the
restaurant — the single most demo-specific failure on this list.

This rule is deliberately **port-scoped, not program-scoped**, because the KDS
and admin pages are served by Node, not by the POS binary. A program-scoped
rule naming only `holler-pos.exe` covers 9310 and 9320 and leaves the two
screens dark.

**Check it, and the check is not "the rule exists":**

```powershell
Get-NetFirewallRule -DisplayName "Holler demo" | Get-NetFirewallPortFilter
```

### T-3. Full dress rehearsal, on the hotspot, with every device

Turn on the hotspot. Join your phone and the second laptop. Run Part 3 end to
end, twice. **A rehearsal on your home WiFi proves nothing about Wednesday** —
different network class, different firewall profile, different addresses.

---

## On the day

### Step 0 — Hotspot up, and write the address down

Windows Settings ▸ Network ▸ **Mobile hotspot** ▸ On. Then:

```powershell
Get-NetIPAddress -AddressFamily IPv4 |
  Where-Object { $_.IPAddress -ne '127.0.0.1' -and $_.IPAddress -notlike '169.254.*' } |
  Select-Object IPAddress, InterfaceAlias
```

The hotspot row is usually `192.168.137.1` on an adapter called `Local Area
Connection* N`. **Ignore `172.28.x.x` — that is an internal Windows adapter and
no phone can reach it.**

**Write the address on a sticky note.** You will type it into three phones.

> **This is the number-one silent failure in this system.** If the address you
> pass does not match the address the machine actually has, the kitchen screen
> loads, looks completely normal, and never connects — forever, with no error.
> It cost a session on 14 September. Check it; do not assume it.

### Step 1 — The laptop, before anything else

- Plug in the power.
- Settings ▸ System ▸ Power: **Screen** and **Sleep** both to **Never**.
  *A laptop that sleeps drops the hotspot, and every device in the room
  disconnects at once.*
- **Do Not Disturb on.** Notifications land centre-screen, over the bill.
- Close Slack, Teams, Outlook.

### Step 2 — Bring the stack up

```powershell
cd C:\Code\Holler
.\scripts\demo-up.ps1 -DbKeyHex <your-key> -LanHost <hotspot-ip> -Fresh -Release
```

`-Fresh` resets to the demo seed. `-Release` starts the release binary.

It prints the **captain URL** and a **pair token** at the end. Keep that window.

Confirm the right binary is running:

```powershell
Get-Process holler-pos | Select-Object Id, Path, StartTime
```

`Path` must end `target\release\holler-pos.exe`. **Not `target\debug`.**

### Step 3 — The kitchen screen, on the second laptop

On **your** laptop the KDS window is already open. For the second laptop, open:

```
http://<LAN-IP>:5174/
```

Wait for the indicator to read **connected**. If it says "Connecting to the
till…" for more than ten seconds, the address is wrong or the firewall rule is
missing — those are the only two causes.

### Step 4 — Extra waiter phones, one command each

For each panel member's phone:

```powershell
.\scripts\add-waiter.ps1 -Name "Rahul's phone"
.\scripts\add-waiter.ps1 -Name "Priya's phone"
```

Each prints the captain URL and **its own** pair token. Use a real name — three
minutes later, "WAITER-2" tells you nothing and "Priya's phone" tells you
everything.

> **WAIT ONE MINUTE BETWEEN ENROLLING AND PAIRING.** The till refreshes its list
> of allowed devices every 60 seconds. A phone paired immediately will say *"That
> device token was rejected"* — the same message a mistyped token gives. **It is
> not broken, it is early.** Enrol all phones during Step 2 while the stack is
> coming up, and this never bites you.

**Fallback if it does bite you:** hand out the SAME token from `demo-up` to
every phone. All three work immediately. The cost is that every order reads as
coming from one device, so you lose the "three named waiters" line — but you
keep the demo. Decide this in five seconds, not five minutes.

### Step 5 — Hand the phones out

Say this while they join:

> "Join the WiFi called `<hotspot name>`, password `<password>`. Then open your
> browser and go to this address. No app to install."

Each person pastes their token once. The phone remembers it.

---

# PART 3 — THE DEMO, WITH YOUR DIALOGUE

Total: **18–22 minutes**. Each act has a **point**, an **action** and a
**fallback**. If an act fails, use the fallback and keep moving — the worst
outcome is debugging in front of them.

---

## ACT 1 — "Your restaurant, on your devices, in ten seconds" *(3 min)*

**Point:** no installation, no per-device cost, no waiting.

**Action:** Phones are already in their hands from Step 5.

> "Before I show you anything, look at what just happened. Three phones —
> two Android, one iPhone, none of them ours — became waiter terminals in about
> ten seconds. Nothing was installed. There is no app store, no MDM, no licence
> per device. Your staff's own phones are your terminals. When a waiter leaves,
> you revoke one token; you don't chase a device.
>
> And notice the name at the top of every screen — Shinjuku Yakitori, your
> menu, your prices. Putting a new restaurant on this system is filling in one
> file. It is not a software project."

**Fallback:** if one phone won't pair, carry on with two. Never debug here.

---

## ACT 2 — "Three waiters, one kitchen" *(5 min) — THE CENTREPIECE*

**Point:** this is a real distributed system, not a screen with mock data.

**Action — do it in this exact order:**

1. Ask **all three** to pick **different tables** and order **2–3 items each**.
2. Ask one to add a **free modifier** (spice level / no onion).
3. **"Everyone press Send — now."** All three at once.
4. Turn to the kitchen screen.

> "Three people, three phones, three different tables, all sending at the same
> moment. Watch the kitchen screen."

*(Tickets appear.)*

> "Three tickets. Right tables, right items, right modifiers, in the order they
> arrived. Nothing collided, nothing was lost, nothing needed refreshing. That
> is not three phones talking to a website — there is no website. Those phones
> are talking to **this laptop**, on this local network. If the restaurant's
> internet went down right now, that would keep working exactly as you just saw
> it."

**Then, the detail that sells it:**

> "And each ticket knows which phone sent it. Not 'a waiter' — Priya's phone.
> When a table says 'we never ordered this', you know who took it. That is not
> a report we generate later; it's recorded at the moment the order is created."

**Fallback:** if one phone's order doesn't appear, say *"and there's the
honest bit — let's see what happened"*, then show the same order **on the
till's Orders screen**. The order is there; only the kitchen view is behind.
That turns a miss into a point about the till being the source of truth.

---

## ACT 3 — "The kitchen answers back" *(2 min)*

**Point:** two-way, not a printout.

**Action:** on the kitchen screen, bump one ticket to **Preparing**, another to
**Ready**. Then on the till, open that order's **Kitchen** view.

> "The cook isn't just receiving. He's telling the floor where the food is. And
> the till is the only thing allowed to change that — the screen asks, the till
> decides and records. One source of truth, so two screens can never disagree
> about whether the food went out."

> ⚠️ **On the till you must leave the order and re-open its Kitchen view** to see
> the new status. Do not alt-tab and point at a stale screen. If it is still
> stale after re-opening, say *"the till refreshes that on a cycle"* and move
> on — it is on our list, and it is not worth a minute of their time.

---

## ACT 4 — "The bill" *(4 min)*

**Point:** this is a legal document, and the money is exact.

**Action:** on the till, take one captain order → **Confirm** → **Send to
Kitchen** → **Bill**. Split: part **cash**, part **UPI**. Print to PDF, open it.

> **Do not bill a bar item.** Food only.

> "Here's the part nobody demos because it's boring and it's the part that gets
> you fined. This is a GST invoice. Correct CGST and SGST split, HSN code on
> every line, your GSTIN, sequential invoice number that cannot repeat and
> cannot skip.
>
> They're paying half cash, half UPI — so here's the QR, for your UPI ID, for
> exactly this amount.
>
> And every figure on that bill is calculated once, in one place, in whole
> paise. Not rupees-and-decimals — whole paise. Floating-point money is how a
> chain loses lakhs it can never trace. The screen and the paper cannot
> disagree, because only one of them does the arithmetic."

**Fallback:** if the PDF doesn't open, show the bill on screen — the numbers are
the point, not the file.

---

## ACT 5 — "Pull the plug" *(4 min) — THE ONE THEY REMEMBER*

**Point:** the thing every competitor fails.

**Action:**

1. **Ask a panel member to turn off your laptop's internet.** Let them do it —
   hand them the machine, or have them watch you click. *It must not look like
   a rehearsed trick.*

   > Turn off **mobile data / Ethernet**, **NOT the hotspot.** If the hotspot
   > dies, every device disconnects and the demo ends. Practise this exact click
   > on Tuesday.

2. Take a **full order on a phone** → kitchen screen → **bill it** → **pay it**.

> "We are now completely offline. No internet at all. And I'm taking an order on
> a phone, it's reaching the kitchen, and I'm printing a legal GST invoice with
> a UPI QR on it.
>
> Ask any other restaurant system to do that. Most of them are a website — no
> internet, no billing, and you're writing on a notepad and typing it in at
> midnight. Your restaurant does not stop because your broadband did."

3. **Turn the internet back on.** Wait. Open the back office on the second
   laptop: `http://<LAN-IP>:5175/` → Orders.

> "Nothing to press. It noticed, and it sent everything that happened while it
> was dark — in order, exactly once. No double entries, no lost table. The
> owner sitting at home just sees the orders arrive."

**Fallback:** if replay is slow, keep talking and refresh once. It arrives
within a minute. Do not stare at the screen in silence.

---

## ACT 6 — "It knows what you used" *(2 min)*

**Point:** it is an operations system, not a cash register.

**Action:** till → **Stock**. Point at an ingredient that moved.

> "Nobody typed this. You sold the chicken skewers; the chicken, the oil, the
> sauce came off stock, at the recipe quantities, right down through the sauces
> you batch yourself. Tomorrow morning your manager doesn't count to find out
> what you used — he counts to find out what went missing. That is the
> difference between stock-taking and theft-detection."

---

## ACT 7 — Close *(2 min)*

> "Everything you just saw ran on this laptop, three of your own phones and a
> local network. No internet for half of it. No app installed on anything.
>
> The hard part of restaurant software isn't the screens — it's that the
> restaurant cannot stop. Not when the internet drops, not when a device dies,
> not at the till on a Saturday night. That is what we've built and that is what
> took the time.
>
> What we'd want next is one of your outlets, for a month, on real service."

---

# PART 4 — HANDLING THE HARD QUESTIONS

| They ask | You say |
|---|---|
| "What if the laptop dies?" | "Orders replay to the cloud continuously, so the day's trade is off the machine. The till itself is a cheap Windows box you can swap and re-point." *(Honest: the replay widening is in progress — don't invite that follow-up.)* |
| "Can two waiters order for the same table?" | "Yes — the second one adds to the open order rather than starting a second bill. You saw three tables; the same table twice appends." **Practise this Tuesday.** |
| "What about 20 tables and 10 waiters?" | "Same shape. The till is the hub and it's doing very little work per order." Don't invent a number you haven't measured. |
| "Does it work on iPhone?" | "It's a web page — anything with a browser." *(One panel iPhone proves it.)* |
| "Who can see what?" | "Every device has its own identity and its own permissions. A waiter phone can order; it cannot bill, discount or void." |
| "How long to set up a new outlet?" | "One configuration file — name, GSTIN, address, invoice series, UPI ID. Then the menu. No code." |
| "What does it cost to run?" | "One ordinary Windows PC per outlet. Staff phones are the terminals. There's no per-device licence and no per-terminal hardware." |
| "Can it integrate with Zomato/Swiggy?" | "The framework is built and an order from a platform lands on the till like any other. Going live needs their sandbox, which is a commercial conversation, not an engineering one." |
| "Is our data safe?" | "The outlet's database is encrypted on disk, and staff credentials never travel in readable form." |

---

# PART 5 — IF IT BREAKS: 60-SECOND TRIAGE

**Rule: one attempt, then fall back and keep talking.** Never debug in silence.

| Symptom | First thing to check | Fallback line |
|---|---|---|
| Phone: "device token was rejected" | Was it enrolled under a minute ago? **Wait 60s, pair again.** | Hand them the `demo-up` token instead |
| Phone won't load the page at all | Is it on **your hotspot**, not the restaurant WiFi? | Use your own phone; carry on with two |
| Kitchen screen stuck "Connecting to the till…" | The LAN address is wrong, or the firewall rule is missing. **Those are the only two causes.** | Use the KDS window on your own laptop |
| Ticket didn't appear | Did you go **Confirm → Send to Kitchen** on the Orders screen? The cart's **Send** does not reach the kitchen. | Show the order on the Orders screen |
| Till screen shows stale kitchen status | Leave the order, re-open its Kitchen view | "It refreshes on a cycle" — move on |
| Everything disconnected at once | **The laptop slept, or the hotspot dropped.** | Hotspot back on; everyone rejoins |
| Bill won't print | Show it on screen | "No printer in the room" |

**The one thing never to do on stage:** open a terminal. The moment you do,
they stop seeing a product and start seeing a project.

---

# PART 6 — TUESDAY CHECKLIST

Tick every line. An unticked line is a Wednesday failure.

- [ ] `pnpm --dir apps\pos build`, then `cargo build --release` (in that order)
- [ ] `pnpm --dir apps\kds build` and `pnpm --dir apps\admin build`
- [ ] Firewall rule created (9310, 9320, 5174, 5175 · Private **and** Public), and checked
- [ ] **Hotspot on.** Every rehearsal on the hotspot, never home WiFi
- [ ] Hotspot address noted; KDS connects **from the second laptop**
- [ ] `add-waiter.ps1` run twice; **both phones paired after a 60s wait**
- [ ] Three phones send at once → three tickets on the kitchen screen
- [ ] Two waiters, **same table** → appends, does not open a second bill
- [ ] Bump on kitchen screen → till shows it after leaving and re-opening
- [ ] Split cash+UPI bill → PDF opens → QR visible
- [ ] **Offline act rehearsed**: internet off, hotspot **still up**, order → bill
- [ ] Internet back → order visible in back office
- [ ] Stock screen shows a moved ingredient
- [ ] Sleep and screen-off set to Never; Do Not Disturb on
- [ ] Full run twice, timed, **no terminal opened during either**
- [ ] Phone chargers, laptop charger, sticky note with the LAN address

---

# PART 7 — WHAT I CHANGED TO MAKE THIS POSSIBLE

Two things were blocking a multi-device demo outright.

**1. The kitchen screen and the back office could only be opened on the machine
serving them.** Their dev servers bound to `localhost` only, so no second
laptop and no phone could ever reach them — the LAN-first design was
undemonstrable. Both now bind all interfaces (`apps/kds/vite.config.ts`,
`apps/admin/vite.config.ts`), for the production preview too. **Consequence to
know:** these serve to anything that can reach the machine, so they belong on
your hotspot or an outlet network, never on public WiFi.

**2. There was no way to add a second waiter phone.** `demo-up.ps1` enrols
exactly one. `scripts/add-waiter.ps1` enrols one more per run, prints its
captain URL and its own token, and states the 60-second cache delay in its own
output so it cannot surprise you on the day.

**Not changed, deliberately:** the cart's **Send** button still says "Send" and
still does not send to the kitchen — that is Confirm → Send to Kitchen on the
Orders screen. Relabelling it the day before a demo would put a promise on the
screen that the next screen does not keep. **Learn the two-step; do not trust
the button.**
