# The client walkthrough — three phones, one laptop, one restaurant

**This is the doc you speak from.** It is the story, the exact actions, and —
the part that actually wins the room — *why each thing is hard*, in language a
restaurant owner understands.

It does **not** replace the two files you already have:

| File | What it is | When you read it |
|---|---|---|
| `docs/demo-wednesday.md` | The operational plan — build, firewall, triage, hard questions | The night before, and if something breaks |
| `docs/demo-script.md` | The day-of checklist for bringing the stack up | While setting up |
| **`docs/demo-client-walkthrough.md`** (this file) | **The performance — what you show, what you say, what it proves** | **In the room** |

> **One deviation from `docs/demo-wednesday.md`, and you must know it.** That
> plan puts the kitchen screen on a **second laptop**. This one puts it on a
> **phone**. Read [§1.2](#12-the-kitchen-screen-on-a-phone--what-is-verified-and-what-is-not)
> before you commit to it: the layout is built to collapse to one column and
> the buttons are full-width touch targets, but **the kitchen screen has never
> been run on a phone**, and I am telling you that rather than letting you find
> out in the room.

---

# 1. The cast

Four devices. Each one stands for a real thing in a real restaurant, and you
should introduce them that way — not as "my laptop and three phones".

| Device | Plays | Address it opens | What the client should understand |
|---|---|---|---|
| **Your laptop** | **The till** — the cash counter | The Holler window (already open) | The brain. Everything is stored here. It is the only device that takes money. |
| **Phone 1** | **Waiter — "Rahul"** | `http://<TILL-IP>:9320/` | A waiter's own phone. No app installed. |
| **Phone 2** | **Waiter — "Priya"** | `http://<TILL-IP>:9320/` | A second waiter, working a different table at the same time. |
| **Phone 3** | **The kitchen screen** | `http://<TILL-IP>:5174/` | Mounted by the pass in a real kitchen. Here, a phone. |
| Your laptop, browser tab | **The back office** | `http://localhost:5175` | What the owner sees from home. |

**`<TILL-IP>` is your laptop's address on your own hotspot.** Get it with:

```powershell
.\scripts\lan-ip.ps1 -Urls
```

Write it on a sticky note. Every phone types the same host, different port.

## 1.1 The one thing that must be true

**Every device is on YOUR laptop's Mobile Hotspot. Never the restaurant's
WiFi.** Their network will have client isolation, a captive portal or a guest
VLAN that silently blocks device-to-device traffic — and the failure looks
exactly like your software being broken. This is the single most likely way to
lose the demo, and it has nothing to do with the product.

## 1.2 The kitchen screen on a phone — what is verified and what is not

Be honest with yourself about this before you stand up.

**What I verified in the code:**

- The ticket grid is `repeat(auto-fill, minmax(280px, 1fr))` — on a phone
  viewport it collapses to **one column of tickets**, stacked. Nothing is cut
  off and nothing needs sideways scrolling.
- The action button on each ticket is **full width** at the large control
  height — a genuine touch target, not a mouse target.
- A cook advances a ticket with four taps over its life: **Accept → Start
  preparing → Mark ready → Mark served**.
- The kitchen screen needs **no pairing**. Its credential is built into the
  page. Open the URL and it is the kitchen screen.

**What nobody has observed:** the kitchen screen running on an actual phone.
Not once. It has always been a laptop or the till itself. Test it during setup,
not in the room.

**If it looks wrong on the phone:** put the kitchen screen on your laptop in a
second window and hand a phone to a panel member as a *third waiter* instead.
The demo does not weaken — it arguably strengthens, because three waiters
ordering at once is the harder thing.

---

# 2. Setup, in order

Assumes `demo-build.ps1` has run and the firewall rule exists — see
`docs/demo-wednesday.md` T-1 and T-2. Both must be done before this.

**1. Hotspot on.** Then note the address:

```powershell
.\scripts\lan-ip.ps1 -Urls
```

**2. Start the stack** (your own terminal, not one I control):

```powershell
.\scripts\demo-up.ps1 -DbKeyHex <key> -Fresh -Release
```

`-Fresh` resets cloud and edge to the demo seed: the full menu, recipes,
opening stock, one supplier, one received delivery. You want this. A demo on
yesterday's data has yesterday's mistakes in it.

**3. While that is coming up, enrol the second waiter phone:**

```powershell
.\scripts\add-waiter.ps1 -Name "Priya's phone"
```

> ### THE SIXTY SECONDS — the one thing that will look broken and is not
>
> The till refreshes its list of allowed devices **every 60 seconds**. A phone
> paired immediately after enrolment is told *"That device token was
> rejected"* — **the same message a wrong token gives**. It is not broken. It
> is early.
>
> **Enrol both phones while the stack is booting.** Then pairing just works.
> Do not debug this in the first thirty seconds.

**4. Pair the two waiter phones.** Each opens `http://<TILL-IP>:9320/` and
pastes its **own** token once. The phone remembers it.

**5. Open the kitchen screen** on phone 3: `http://<TILL-IP>:5174/`. Wait for
the indicator to say **connected**. If it sits on "Connecting to the till…"
for more than ten seconds, the address is wrong or the firewall rule is
missing — those are the only two causes.

**6. Open the back office** on your laptop: `http://localhost:5175`, sign in as
the owner.

**7. Laptop settings:** sleep and screen-off to **Never**, Do Not Disturb
**on**. A notification banner across the till during the bill is a bad look.

---

# 3. THE DEMO

Eight acts, about 25 minutes. Each act below gives you **DO**, **THEY SEE**,
**SAY**, and **WHY THIS IS HARD** — the last one is the commentary that turns a
screen recording into a reason to buy.

---

## ACT 0 — The claim *(1 min)*

**DO:** Nothing yet. Devices on the table, screens on.

**SAY:**

> "Before I show you anything — one sentence about what this is.
>
> Most restaurant software is a website. If the internet drops, you stop
> billing. Holler is the other way round: everything runs on the till, in this
> room, on this machine. The internet is a convenience, not a requirement.
>
> I'm going to show you an order taken on a waiter's phone, cooked in the
> kitchen, billed with a legal GST invoice, and deducted from stock — and then
> I'm going to unplug the internet and do it again."

**WHY THIS IS HARD:** You have just promised the hardest thing in the room and
set up Act 6. Do not soften it.

---

## ACT 1 — Two waiters, two tables, one kitchen *(5 min) — THE CENTREPIECE*

This is the act the client's mental model is built around: *device at table →
hub → kitchen*. Everything else is supporting material.

**DO:**
1. Hand **Phone 1 to one panel member**, **Phone 2 to another**. Let them hold
   the devices. This matters more than anything you say.
2. Phone 1: tap **Table 4**. Phone 2: tap **Table 7**.
3. Each picks two or three dishes. **Food only — see the warning below.**
4. Both tap **Send** at roughly the same time.

**THEY SEE:** Two tickets appear on the kitchen phone, near-instantly, each
naming its own table.

**SAY:**

> "Neither of them installed anything. That's a web page served by the till
> itself — the phone joined the restaurant's network and that was the whole
> setup. In a real outlet a new waiter is handed a phone and is taking orders
> in about ten seconds.
>
> Notice they worked at the same time, on different tables, and nothing
> collided."

> ### ⚠️ DO NOT ORDER A BAR ITEM
> Alcohol is seeded at zero tax because state VAT on liquor is not yet
> expressible in the system. An alcohol line on the bill in Act 4 is a wrong
> bill in front of a client. **Order food.** If asked about the bar menu: *"GST
> is done; state VAT on liquor is the next tax module."*

**WHY THIS IS HARD — say some of this, not all:**

- **No app store.** Nothing is installed, so nothing needs updating on twelve
  phones. Deploy once to the till, every device is current.
- **It works with the internet down.** The phone talks to the till over the
  local network. It never needed the internet, so losing it changes nothing —
  which is what Act 6 proves.
- **The order knows who took it.** Each phone has its own credential, and the
  order records the *device that actually created it* — not the till, and not
  whatever the phone claims to be. That distinction is exactly why "which
  waiter took this order" is answerable later rather than guessable.
- **Two waiters, two tables, simultaneously** is the boring-sounding thing that
  breaks naive systems. Each table holds its own open order; neither can
  scribble on the other's.

---

## ACT 2 — The kitchen answers back *(3 min)*

**DO:**
1. Take the kitchen phone. Tap **Accept** on one ticket, then **Start
   preparing**.
2. Show the till: the order's state has moved.
3. Tap **Mark ready**.

**THEY SEE:** Ticket state changes on the kitchen phone; the till reflects it.

**SAY:**

> "The kitchen isn't a printer here — it's a participant. The cook accepting a
> ticket is recorded, and the front of house can see it. When a table asks
> 'how long?', the answer is on the screen instead of in someone's head."

**WHY THIS IS HARD:**

- **The kitchen screen is never the authority.** Tapping a button *requests* a
  change; the till decides and confirms. If a ticket cannot legally move, the
  screen shows what the till confirmed, not what the cook tapped. That's how
  two screens never drift apart.
- **It's a live connection, not polling.** The ticket appears when it's sent,
  not up to N seconds later.
- **Four honest states** — accepted, preparing, ready, served — not a binary
  done/not-done, because a kitchen is not binary.

*Optional flourish, only if the room is technical:* open the kitchen screen
with `?perf=1` and it shows its own wire-to-render time in milliseconds. Do not
promise a number you have not measured on the hotspot first.

---

## ACT 3 — A second round on a live table *(2 min)*

**DO:** Phone 1, same table (Table 4): add one more dish. **Send** again.

**THEY SEE:** A **new ticket** for the new dish only. The table still has **one**
bill.

**SAY:**

> "Second round on the same table. The kitchen gets a ticket for the new dish
> only — they don't re-cook the first round. And the table still has one bill,
> not two."

**WHY THIS IS HARD:** "Append to an order the kitchen already has" is where
most systems either duplicate the whole order or open a second bill. The table
is asked what its open order is and the round is appended to it; only the
lines that have no ticket yet are sent to the kitchen.

---

## ACT 4 — The bill *(5 min)*

**DO:**
1. On the **till**, open Table 4 and go to billing.
2. Show the invoice on screen. Point at: **invoice number**, **restaurant name
   and GSTIN**, the **CGST/SGST split**, **HSN codes**, the **UPI QR**.
3. Take payment as **split — part cash, part UPI**.
4. The **PDF opens by itself**.

**THEY SEE:** A GST invoice that looks like a real bill, paid two ways, opening
as a document.

**SAY:**

> "That's not a receipt-shaped picture — it's a compliant GST tax invoice.
> Sequential numbering, your GSTIN, tax split per line, HSN code on every item,
> place of supply. Your accountant can use this.
>
> Split payment, because tables actually pay that way — some cash, the rest
> UPI, one bill.
>
> And the QR is for this exact amount, not a generic one."

**WHY THIS IS HARD — this is your strongest technical section:**

- **Money is never a decimal.** Every amount is a whole number of paise, end to
  end. Floating-point money is how a chain loses lakhs invisibly over a year.
  This is a structural choice, not a detail.
- **Tax is computed once, in one place.** The screen and the printed bill
  cannot disagree, because neither of them does the arithmetic — both display
  what the till calculated. Per-line at full precision, summed, rounded once.
- **The invoice cannot be edited.** Once issued, it's frozen. A mistake is
  corrected by a cancellation, never by quietly changing a number. That is what
  makes the number sequence trustworthy.
- **A bill cannot be issued without an HSN code** on every line, because a GST
  invoice without it isn't a compliant document. The system refuses rather than
  producing something that looks fine and isn't.
- **The invoice number is minted on the till and never leaves it.** No cloud
  service hands out numbers, so no outage can produce a gap or a duplicate.

**On the printer, if asked:** *"There's no thermal printer in the room. That
PDF is rendered from exactly the same data the printer gets."*

---

## ACT 5 — It knows what you used *(3 min)*

**DO:** On the till, open the stock screen. Show the ingredients that moved
because of the sale you just billed. Point out any low-stock warning.

**SAY:**

> "Nobody typed this. You sold those dishes, so those ingredients came down —
> by recipe, at the right quantities. If a base is made in batches and used in
> three dishes, selling any of them draws down the batch correctly.
>
> This is the difference between knowing what you sold and knowing what you
> used. The gap between those two numbers is theft, waste and over-portioning —
> and right now most owners find out at month end, if at all."

**WHY THIS IS HARD:**

- **Recipes nest.** A dish uses a sauce; the sauce has its own recipe and its
  own batch size. Selling the dish draws down through both levels at exact
  ratios — and if the sauce is rescaled from 300ml to 3 litres, every dish that
  uses it stays correct.
- **Stock never blocks a sale.** Negative stock is allowed on purpose. A
  counting error must never stop you selling food — it's a signal to
  investigate, not a wall.
- **A missing recipe never fails a bill.** If an item has no recipe, the sale
  completes and the gap is *recorded* — "items sold with no recipe" is a report
  you can act on, not a silent hole.

---

## ACT 6 — Pull the plug *(4 min) — THE ONE THEY REMEMBER*

**DO:**
1. Say what you are about to do, then do it: **turn off your laptop's mobile
   data / internet — but leave the hotspot running.** Practise this exact click
   beforehand; it is easy to kill the hotspot by accident and take every phone
   down with it.
2. A **banner** appears: the till says it cannot reach the cloud.
3. **Take another order on a phone. Send it. Cook it. Bill it.** Everything
   works.
4. Turn the internet back on.
5. The banner clears. Open the back office — **the order is there.**

**SAY:**

> "Internet's gone. Watch what still works.
>
> …everything. Orders, kitchen, the bill, stock. Because none of it was ever
> asking permission from a server somewhere else.
>
> Now watch it come back." *(reconnect)* "Nobody typed anything twice. Nothing
> was lost. It caught up by itself."

**WHY THIS IS HARD — and this is the real engineering:**

- **Sending is the easy half.** The hard half is knowing exactly what has and
  has not arrived, and never sending the same thing twice. Every order carries
  its own identity and replays in order.
- **One stuck record must not block the rest.** If one item can never be
  accepted, it's set aside and flagged to a human — the queue behind it keeps
  moving. Silence is the failure mode we designed against.
- **The outlet is the source of truth.** The cloud is a copy. That is why the
  restaurant never stops.

**Be honest if pressed:** today it's **orders** that flow up to the back
office. Kitchen tickets, invoices and stock counts stay on the till. *"The
outlet is the source of truth by design; we're widening what replays upward,
one stream at a time."*

---

## ACT 7 — The owner's view *(2 min)*

**DO:** Back office on the laptop. Show **Orders** — including the one taken
while the internet was down. Show the **menu**, the **supplier**, and the
**received delivery**.

**SAY:**

> "This is you, at home, on a Sunday. Today's orders, your menu and prices,
> your suppliers, and what was actually received into the kitchen."

**WHY THIS IS HARD:**

- **A price changed here reaches the till by itself.** The till pulls its
  configuration on a schedule — and if it can't, it keeps selling on the last
  configuration it had. It never stops to ask.
- **What you see is a copy, and we label it as one.** The outlet's own record
  is authoritative. We show both rather than pretending one number exists when
  two honestly do.

**Do not promise:** stock editing or variance reports in the back office. Not
built. *"Stock lives at the outlet today; the owner's stock view is next."*

---

## ACT 8 — The close: onboarding is one file *(2 min)*

**DO:** Optionally show `seed/outlet.toml` — one small text file.

**SAY:**

> "Last thing, and it's the one that matters commercially.
>
> Everything you've seen branded as this restaurant — the name on the bill, the
> GSTIN, the address, the invoice prefix, the UPI ID, the logo — is one file.
> Onboarding a restaurant is writing that file. It is never changing code.
>
> That's the difference between a product and a project. One restaurant or
> fifty, it's the same build."

**WHY THIS IS HARD:** Most "customisable" systems customise by branching the
code, and every branch is a maintenance cost forever. There's also a guard that
refuses to start if the identity file and the menu don't belong together — so
one restaurant's name can never end up on another's GST invoice.

---

# 4. The hard questions

Short answers. Do not over-explain; a long answer sounds like a weak one.

| They ask | You say |
|---|---|
| "What if the till dies?" | "The data is on the till, encrypted, and it syncs to the cloud. Hardware replacement is a restore — that's a process we set up with you at install, not something I'd hand-wave now." |
| "Can the waiter take payment?" | "Deliberately not. Money goes through one screen with one person accountable. Phone-side payment is a separate decision with its own controls." |
| "Can it handle our bar?" | "GST is done. State VAT on liquor is the next tax module — that's why I ordered food today rather than show you something I'd have to caveat." |
| "How many devices?" | "It's your local network, not a per-seat licence. The limit is practical, not architectural." |
| "Is my data safe?" | "The database on the till is encrypted at rest, and staff credentials are never sent anywhere in readable form." |
| "What about our existing menu?" | "Your menu is already in here — that's what you've been looking at all session." |
| "Who else uses this?" | Do not invent a customer. "You'd be the first outlet running it in production, which is why the pilot terms matter and why I'm being precise with you about what's built and what isn't." |
| "Can we get it next week?" | "What you saw is real. What's between here and your floor is a pilot — printer, your tax profile, your staff trained. Weeks, not months." |

---

# 5. If it breaks — 60-second triage

**Rule: never open a terminal in front of the client.** If it needs a terminal,
it needs a break.

| Symptom | Almost certainly | Do this |
|---|---|---|
| Phone shows "token rejected" | Paired within 60s of enrolling | Wait a minute, paste again. Say nothing. |
| Phone loads nothing at all | Wrong network, or firewall | Check the phone is on **your** hotspot. |
| Kitchen screen stuck "Connecting…" | Firewall rule missing, or wrong address | Move on; use the till's own kitchen view and carry on talking. |
| Order sent, no ticket | Kitchen screen lost its connection | Reload the page on the kitchen phone. |
| A screen is frozen | — | Reload. Don't investigate; keep talking. |
| Anything worse | — | **"That's the demo gremlin — let me show you the next bit."** Move to the next act. Never debug in the room. |

**The recovery line, memorised:** *"I'll come back to that."* Then don't.

---

# 6. What NOT to do

1. **Do not bill an alcohol item.** Zero tax; wrong bill.
2. **Do not use the restaurant's WiFi.** Yours, always.
3. **Do not open a terminal**, or a code editor, or a log file.
4. **Do not promise a date** for anything not in this walkthrough.
5. **Do not invent a reference customer.**
6. **Do not apologise for what isn't built.** State it in one sentence and move
   on. Confidence about the boundary reads as competence; hedging reads as
   hiding something.
7. **Do not zoom into the GSTIN.** It's a correctly-shaped placeholder, not
   their real number. If they notice: *"Placeholder — yours goes in that one
   file at install."*

---

# 7. The 45-second version

If you get five minutes instead of thirty, this is the whole product:

1. **Phone → kitchen.** Order on a waiter's phone, ticket on the kitchen
   screen. No app installed.
2. **Internet off. Do it again.** Everything still works.
3. **Internet on. It catches up by itself.**
4. **The bill is a real GST invoice**, and the stock moved by itself.

Those four beats, in that order. Everything else in this file is supporting
detail for a longer meeting.
