# The client demonstration: three phones, one laptop, one restaurant

This is the document you speak from. It tells you what to show, what to say,
and why each thing you are showing is genuinely difficult to build. That last
part matters. Anyone can show a screen. Explaining why the screen was hard to
build is what turns a demonstration into a reason to buy.

This file does not replace the two you already have.

| File | What it contains | When to read it |
|---|---|---|
| `docs/demo-wednesday.md` | The operating plan: building, the firewall, what to do when things break | The night before, and during any trouble |
| `docs/demo-script.md` | The checklist for starting everything up on the day | While you are setting up |
| `docs/demo-client-walkthrough.md` (this file) | The demonstration itself: what you show and what you say | In the room |

**One important difference from `docs/demo-wednesday.md`.** That plan puts the
kitchen screen on a second laptop. This one puts it on a telephone. Please read
section 1.2 before you decide to do it that way. The screen is built to work on
a small display, and I have checked that in the code, but nobody has ever
actually run it on a telephone. I would rather tell you now than let you find
out in front of the client.

---

# 1. What you need, and what each thing represents

You have four devices. Each one stands for something real in a working
restaurant, and you should introduce them that way, rather than as "my laptop
and three phones".

| Device | What it represents | What it opens | What the client should understand |
|---|---|---|---|
| Your laptop | The till at the cash counter | The Holler window, already open | This is the brain. Everything is stored here, and it is the only device that handles money. |
| Telephone 1 | A waiter, say Rahul | `http://<TILL-IP>:9320/` | An ordinary phone. Nothing was installed on it. |
| Telephone 2 | A second waiter, say Priya | `http://<TILL-IP>:9320/` | Another waiter, working a different table at the same moment. |
| Telephone 3 | The kitchen screen | `http://<TILL-IP>:5174/` | In a real kitchen this is mounted by the pass. Today it is a phone. |
| Your laptop, in a browser | The back office | `http://localhost:5175` | What the owner looks at from home. |

`<TILL-IP>` is your laptop's address on your own hotspot. You can find it by
running:

```powershell
.\scripts\lan-ip.ps1 -Urls
```

Write that address on a piece of paper. Every phone types the same address,
with a different number after the colon.

## 1.1 The one thing that must be true

Every device must be connected to your laptop's own mobile hotspot, and never
to the restaurant's wireless network. Their network will almost certainly stop
devices from talking to one another, either through client isolation, or a
login page, or a separate guest network. When that happens the demonstration
simply stops working, and it looks exactly as though your software is broken.
This is the most likely way to lose the room, and it has nothing whatever to do
with the product.

## 1.2 The kitchen screen on a telephone: what I checked and what I did not

Please be honest with yourself about this before you stand up.

Here is what I confirmed by reading the code:

* The tickets are laid out so that on a narrow screen they become a single
  column, one under the other. Nothing is cut off and nothing has to be
  scrolled sideways.
* The button on each ticket runs the full width of the ticket and is sized for
  a finger rather than a mouse.
* A cook moves a ticket along with four taps over its life: Accept, then Start
  preparing, then Mark ready, then Mark served.
* The kitchen screen does not need to be paired with anything. Its credentials
  are built into the page. You open the address and it is the kitchen screen.

Here is what nobody has ever done: run that screen on a real telephone. Not
once. It has always been a laptop, or the till itself. Please try it while you
are setting up, rather than in front of the client.

If it looks wrong on the phone, put the kitchen screen on your laptop in a
second window and hand the spare phone to somebody on the panel as a third
waiter. The demonstration does not suffer for it. It may even improve, because
three waiters ordering at the same time is the harder thing to do.

---

# 2. Setting up, in order

This assumes you have already built the software and created the firewall rule.
Both of those are in `docs/demo-wednesday.md`, sections T-1 and T-2, and both
must be done first.

**First,** switch the hotspot on, then find your address:

```powershell
.\scripts\lan-ip.ps1 -Urls
```

**Second,** start everything, from a terminal you opened yourself:

```powershell
.\scripts\demo-up.ps1 -DbKeyHex <key> -Fresh -Release
```

The `-Fresh` part resets both the till and the cloud back to the prepared
demonstration data: the full menu, the recipes, opening stock, one supplier and
one delivery already received. You want this. A demonstration run on
yesterday's data still has yesterday's mistakes in it.

**Third,** while that is starting, enrol the second waiter's phone:

```powershell
.\scripts\add-waiter.ps1 -Name "Priya's phone"
```

> **Please read this next part carefully, because it will look like a fault and
> it is not one.**
>
> The till refreshes its list of permitted devices once every sixty seconds. If
> you pair a phone immediately after enrolling it, the phone is told that the
> device code was rejected. That is the same message it would give for a code
> typed in wrongly, so it reads like a real failure. It is not. The phone is
> simply early.
>
> Enrol both phones while everything is still starting up and you will never
> see this. If you do see it, wait a minute and try once more. Please do not
> start investigating it in the first thirty seconds.

**Fourth,** pair the two waiters' phones. Each one opens
`http://<TILL-IP>:9320/` and pastes in its own code, once. The phone remembers
it from then on.

**Fifth,** open the kitchen screen on the third phone at
`http://<TILL-IP>:5174/` and wait until it says it is connected. If it still
says it is connecting after about ten seconds, then either the address is wrong
or the firewall rule is missing. Those are the only two causes.

**Sixth,** open the back office on your laptop at `http://localhost:5175` and
sign in as the owner.

**Seventh,** set your laptop so that it never sleeps and never turns its screen
off, and switch on Do Not Disturb. A message sliding across the till while you
are showing somebody their bill looks careless.

---

# 3. The demonstration

There are eight parts and they take about twenty five minutes altogether. Each
one tells you what to do, what the client will see, roughly what to say, and
why the thing you have just shown is difficult to build. That last section is
the one worth reading twice.

---

## Part one: the opening claim (about a minute)

**What you do:** nothing yet. The devices are on the table with their screens
on.

**What to say:**

> "Before I show you anything, let me tell you in one sentence what this is.
>
> Most restaurant software is really a website. If the internet goes down, you
> stop billing. This works the other way round. Everything runs on the till, in
> this room, on this machine. The internet is a convenience, not a requirement.
>
> I am going to show you an order taken on a waiter's phone, cooked in the
> kitchen, billed with a proper GST invoice, and taken out of stock. Then I am
> going to disconnect the internet and do the whole thing again."

**Why this matters:** you have just promised them the hardest thing you have,
and you have set up part seven. Do not water it down.

---

## Part two: two waiters, two tables, one kitchen (about five minutes)

This is the most important part of the demonstration. The picture in the
client's head is a device at the table, talking to a machine at the counter,
talking to the kitchen. Everything else you show is supporting material.

**What you do:**

1. Hand telephone 1 to somebody on the panel and telephone 2 to somebody else.
   Let them hold the devices themselves. This does more for you than anything
   you can say.
2. On the first phone, tap table four. On the second, tap table seven.
3. Each person chooses two or three dishes. Food only, and please read the
   warning below.
4. Both tap Send at roughly the same moment.

**What they see:** two tickets appear on the kitchen phone almost at once, each
one naming its own table.

**What to say:**

> "Neither of you installed anything. That is a web page being served by the
> till itself. The phone joined the restaurant's network, and that was the whole
> of the setup. In a real restaurant you hand a new waiter a phone and he is
> taking orders about ten seconds later.
>
> And notice that you both worked at the same time, on different tables, and
> nothing went wrong."

> **Please do not order anything from the bar.**
>
> The drinks have been set up with no tax on them, because state tax on alcohol
> is not something the system can express yet. If a drink ends up on the bill in
> part five, you will be showing the client an incorrect bill. Order food.
>
> If somebody asks about it, the honest answer is short: "GST is finished. State
> tax on liquor is the next piece of tax work."

**Why this is difficult to build.** Say some of this, not all of it, and judge
the room.

* Nothing is installed, so nothing ever has to be updated on twelve different
  phones. You update the till and every device is current.
* It keeps working when the internet is down. The phone is talking to the till
  over the local network. It never needed the internet in the first place,
  which is what part seven proves.
* The order knows who took it. Each phone has its own credentials, and the
  order records the device that actually created it, rather than the till, and
  rather than whatever the phone claims to be. That is precisely why you can
  answer "which waiter took this order" afterwards instead of guessing.
* Two waiters working two tables at the same time sounds dull, and it is the
  thing that breaks simpler systems. Each table holds its own open order, and
  neither waiter can write over the other's work.

---

## Part three: the kitchen answers back (about three minutes)

**What you do:**

1. Pick up the kitchen phone. Tap Accept on one of the tickets, then tap Start
   preparing.
2. Show the client the till. The order has moved along.
3. Tap Mark ready.

**What they see:** the ticket changes on the kitchen phone, and the till knows
about it.

**What to say:**

> "The kitchen here is not a printer. It takes part. When the cook accepts a
> ticket that is recorded, and the front of house can see it. So when a table
> asks how long their food will be, the answer is on a screen instead of in
> somebody's head."

**Why this is difficult to build.**

* The kitchen screen is never in charge. Tapping a button asks the till to make
  the change. The till decides, and the screen then shows what the till
  confirmed, rather than what the cook tapped. That is how two screens are kept
  from drifting apart and telling two different stories.
* The connection stays open, so the ticket appears the moment it is sent,
  instead of whenever the screen next thinks to ask.
* There are four honest stages rather than a simple done or not done, because a
  kitchen does not work in two states.

If the room is a technical one, you can open the kitchen screen with `?perf=1`
added to the address and it will show you how many milliseconds it took for a
ticket to arrive and appear. Please do not quote a number you have not measured
on the hotspot yourself first.

---

## Part four: adding to a table that is already eating (about two minutes)

**What you do:** on the first phone, go back to table four and add one more
dish. Send it again.

**What they see:** a new ticket in the kitchen for the new dish only. The table
still has one bill.

**What to say:**

> "That is a second round on the same table. The kitchen gets a ticket for the
> new dish only, so they do not cook the first round again. And the table still
> has one bill rather than two."

**Why this is difficult to build.** Adding to an order the kitchen already has
is where most systems either send the whole order again or quietly open a
second bill. Here the table is asked what its open order is, the new round is
added to that order, and only the lines that have not been sent to the kitchen
yet are sent.

---

## Part five: the bill (about five minutes)

**What you do:**

1. On the till, open table four and go to billing.
2. Show them the bill on screen. Point at the invoice number, the restaurant's
   name and GST number, the way the tax is split into its two halves, the HSN
   codes against each item, and the payment code.
3. Take the payment as a split: part cash, the rest by UPI.
4. The bill opens by itself as a document.

**What they see:** something that looks like a real bill, paid in two ways,
opening as a proper document.

**What to say:**

> "That is not a picture shaped like a receipt. It is a proper GST tax invoice.
> Numbered in sequence, your GST number on it, tax split correctly on every
> line, an HSN code against each item, and the place of supply. Your accountant
> can work from this.
>
> And it was paid two ways, because that is how tables actually pay. Some cash,
> the rest by phone, one bill.
>
> The payment code is for this exact amount, not a general one."

**Why this is difficult to build.** This is your strongest section technically,
so take your time over it.

* Money is never stored as a decimal number. Every amount is a whole number of
  paise from beginning to end. Storing money as a decimal is how a chain of
  restaurants loses a great deal of it invisibly over a year. This was a
  deliberate decision rather than a detail.
* The tax is worked out once, in one place. The screen and the printed bill
  cannot disagree with one another, because neither of them does the
  arithmetic. Both simply show what the till worked out.
* Once a bill has been issued it cannot be edited. A mistake is corrected by
  cancelling it and issuing another, never by quietly changing a number. That is
  what makes the numbering worth trusting.
* A bill cannot be issued at all if any item is missing its HSN code, because a
  GST invoice without one is not a valid document. The system refuses, rather
  than producing something that looks correct and is not.
* The invoice number is produced on the till and never leaves it. No service
  somewhere else hands out numbers, so no outage can create a gap in the
  sequence, or the same number twice.

If somebody asks about the printer, the answer is simply that there is no
thermal printer in the room, and that the document they are looking at is
produced from exactly the same information the printer would be given.

---

## Part six: it knows what you used (about three minutes)

**What you do:** on the till, open the stock screen. Show the ingredients that
came down because of the sale you have just billed. Point out any item that is
now running low.

**What to say:**

> "Nobody typed any of this in. You sold those dishes, so those ingredients
> came down, by recipe, in the right quantities. If a sauce is made in batches
> and used in three different dishes, selling any of them takes the right amount
> out of the batch.
>
> This is the difference between knowing what you sold and knowing what you
> used. The gap between those two figures is theft, waste and over generous
> portions, and most owners only find out at the end of the month, if they find
> out at all."

**Why this is difficult to build.**

* Recipes sit inside other recipes. A dish uses a sauce, and the sauce has its
  own recipe and its own batch size. Selling the dish works its way down through
  both levels at the right proportions. If the sauce is later made in three
  litre batches instead of three hundred millilitre ones, every dish that uses
  it stays correct.
* Running out of stock never stops a sale. Stock is allowed to go below zero on
  purpose. A counting error must never stop you selling food. It is something to
  look into, not a locked door.
* A missing recipe never stops a bill either. If an item has no recipe the sale
  still goes through and the gap is written down, so that "items sold with no
  recipe" is a report somebody can act on rather than a silent hole.

---

## Part seven: disconnect the internet (about four minutes)

This is the part they will still be talking about afterwards.

**What you do:**

1. Tell them what you are about to do, then do it. Turn off your laptop's
   internet connection, but leave the hotspot running. Please practise this
   exact step beforehand. It is very easy to switch off the hotspot by accident
   and take all three phones down with it.
2. A message appears on the till saying it cannot reach the cloud.
3. Take another order on a phone. Send it. Cook it. Bill it. All of it works.
4. Turn the internet back on.
5. The message clears by itself. Open the back office and the order is there.

**What to say:**

> "The internet has gone. Watch what still works.
>
> All of it. Orders, the kitchen, the bill, the stock. None of it was ever
> asking permission from a machine somewhere else.
>
> Now watch it catch up." (reconnect) "Nobody typed anything twice. Nothing was
> lost. It sorted itself out."

**Why this is difficult to build, and this is the real engineering.**

* Sending the information is the easy half. The hard half is knowing exactly
  what has arrived and what has not, and never sending the same thing twice.
  Every order carries its own identity and goes up in order.
* One stuck record must not hold up everything behind it. If one item can never
  be accepted, it is set aside and somebody is told about it, and the queue
  behind it keeps moving. A system that goes quiet is the failure we designed
  against.
* The restaurant holds the true record and the cloud holds a copy. That is the
  reason the restaurant never has to stop.

If they press you, be straightforward. At the moment it is orders that travel
up to the back office. Kitchen tickets, bills and stock counts stay on the
till. The honest sentence is: "The restaurant is the authority by design, and
we are widening what travels up one piece at a time."

---

## Part eight: the owner's view (about two minutes)

**What you do:** open the back office on your laptop. Show the orders,
including the one taken while the internet was off. Show the menu, the
supplier, and the delivery that was received.

**What to say:**

> "This is you at home on a Sunday. Today's orders, your menu and your prices,
> your suppliers, and what actually came into the kitchen."

**Why this is difficult to build.**

* If you change a price here it reaches the till on its own. The till asks for
  its settings on a schedule, and if it cannot reach anything it carries on
  selling using the last settings it had. It never stops to ask permission.
* What you are looking at is a copy, and we label it as one. The restaurant's
  own record is the authority. Where the two can honestly differ we show both,
  rather than pretending there is only one number.

Please do not promise stock editing or waste reports in the back office. They
are not built. If asked: "Stock lives at the restaurant today. The owner's view
of it is the next piece of work."

---

## The closing point: setting up a restaurant is one file (about two minutes)

**What you do:** if it helps, show them `seed/outlet.toml`. It is one small
text file.

**What to say:**

> "One last thing, and commercially it is the one that matters.
>
> Everything you have seen today with this restaurant's name on it, the name on
> the bill, the GST number, the address, the invoice prefix, the payment
> address, the logo, all of that is one file. Setting up a new restaurant means
> writing that file. It never means changing the software.
>
> That is the difference between a product and a one off project. One restaurant
> or fifty, it is the same software."

**Why this is difficult to build.** Most systems that claim to be adaptable
adapt by making a separate copy of the software for each customer, and every
copy has to be looked after for ever afterwards. There is also a safeguard here
which refuses to start if the restaurant's details and its menu do not belong
together, so one restaurant's name can never end up on another's tax invoice.

---

# 4. Questions they are likely to ask

Keep the answers short. A long answer sounds like a weak one.

| Question | Answer |
|---|---|
| What happens if the till breaks? | "The information is on the till, encrypted, and it copies up to the cloud. Replacing the machine is a restore, and that is something we would set up with you properly at installation, rather than my waving a hand at it now." |
| Can the waiter take the payment? | "Deliberately not. Money goes through one screen with one person answerable for it. Taking payment on the phone is a separate decision with its own controls." |
| Can it cope with our bar? | "GST is finished. State tax on liquor is the next piece of tax work, which is why I ordered food today rather than show you something I would have to apologise for." |
| How many devices can we have? | "It is your own network rather than a licence for each person, so the limit is practical rather than built in." |
| Is our information safe? | "The database on the till is encrypted, and staff passwords are never sent anywhere in a readable form." |
| What about our existing menu? | "Your menu is already in here. It is what you have been looking at all the way through." |
| Who else is using it? | Do not invent a customer. "You would be the first restaurant running it properly, which is why the pilot terms matter, and why I am being careful to tell you exactly what is built and what is not." |
| Can we have it next week? | "What you have seen is real. What stands between here and your floor is a pilot: the printer, your tax details, your staff trained on it. That is weeks rather than months." |

---

# 5. If something goes wrong

The rule is simple. Never open a terminal window in front of the client. If
something needs a terminal, it needs to wait until they have gone.

| What you see | Almost certainly | What to do |
|---|---|---|
| The phone says the code was rejected | It was paired within a minute of being enrolled | Wait a minute and paste it again. Say nothing about it. |
| The phone will not load anything at all | It is on the wrong network, or the firewall rule is missing | Check that the phone is on your hotspot. |
| The kitchen screen will not connect | The firewall rule is missing, or the address is wrong | Leave it. Use the till's own kitchen view and keep talking. |
| An order was sent but no ticket appeared | The kitchen screen lost its connection | Reload the page on the kitchen phone. |
| A screen has frozen | Anything | Reload it. Do not investigate. Keep talking. |
| Something worse | Anything | "That is the demonstration gremlin, let me show you the next part." Move on. Never debug in the room. |

The sentence to have ready is "I will come back to that." Then do not.

---

# 6. Things not to do

1. Do not put a drink on the bill. There is no tax on it and the bill will be
   wrong.
2. Do not use the restaurant's wireless network. Use your own hotspot, always.
3. Do not open a terminal window, a code editor, or a log file.
4. Do not promise a date for anything that is not in this document.
5. Do not invent a customer who is already using it.
6. Do not apologise at length for the things that are not built. Say what is
   missing in one sentence and move on. Being clear about the edges sounds
   competent. Hedging sounds as though you are hiding something.
7. Do not zoom in on the GST number. It is a correctly formed placeholder
   rather than their real one. If they notice: "That is a placeholder. Yours
   goes into that one file when we install it."

---

# 7. If you only get five minutes

If the meeting collapses to five minutes, this is the whole product in four
steps:

1. An order goes from a waiter's phone to the kitchen screen, with nothing
   installed on the phone.
2. Turn the internet off and do it again. It all still works.
3. Turn the internet back on. It catches up on its own.
4. The bill is a proper GST invoice, and the stock came down by itself.

Those four, in that order. Everything else in this document is detail for a
longer meeting.

---

# 8. Awkward questions, and honest answers

Somebody in that room will ask about something we have not finished. That is
not a problem in itself. The only thing that loses you the room is being caught
overstating what exists, so every answer below is written to be true.

**How to answer any of these.** Say what the system does today in one sentence,
say plainly what it does not do, and then say where it sits in the plan. Do not
apologise more than once, and do not invent a date. "That is not built yet, and
here is what happens instead" is a perfectly respectable answer from a product
that is being piloted. Waffle is not.

**One habit worth having.** If you genuinely do not know, say "I do not know,
and I will find out for you." Nobody has ever lost a deal for saying that.
Several people have lost one by guessing and being wrong in front of their own
engineer.

## 8.1 Cancelling and correcting things

This is the most likely area to come up, because it is what actually happens in
a restaurant during every service. Please read this section properly. The
honest position here is weaker than you might assume.

| They ask | What is actually true today | What to say |
|---|---|---|
| "A table changes its mind. How do I cancel the whole order?" | There is no way to cancel a whole order from any screen. The status exists in the system but nothing sets it. | "Today you would either not send it to the kitchen, or handle it as a correction at the till before billing. A proper cancellation, with a reason recorded against it, is pilot work, and it needs to be, because it is the first thing a manager will ask for." |
| "A guest wants one dish taken off before it goes to the kitchen." | A line's quantity can be changed, but never down to zero, and there is no remove or void action. | "You can change quantities before it goes to the kitchen. Taking a line off entirely is one of the corrections we finish during the pilot." |
| "The dish is already with the kitchen and they want it taken off." | The engine can do this, and it sends the kitchen a cancellation ticket. It is not on any screen. | "The kitchen side of that is built, and it tells the kitchen properly rather than leaving them to notice. The button for it is not on the screen yet." Please do not attempt this in the demonstration. |
| "The bill is wrong and it is already printed." | Nothing can cancel an issued bill. The design allows it. It has not been built. | "By design a bill is never edited. It is cancelled and a fresh one issued, so your numbering stays trustworthy for your accountant. The cancellation itself is pilot work." This is a strong answer. The refusal to edit a bill is the valuable part. |
| "Can I refund a customer?" | Refunds are not implemented. | "Not yet. Money taken is recorded permanently, and a refund is recorded as its own entry rather than by rubbing out the original, which is the right shape for it. Doing that from the screen is pilot work." |
| "Can I move a table, or merge two tables?" | Not built. | "Not today. It is on the list, and it is a small piece of work rather than a structural one, because a table's session and its order are already separate things." |
| "Can I split a bill between guests?" | Built and working. | "Yes." Show it if the moment is right. |
| "Can I give a discount?" | Built, on the billing screen, including discounts that require a reason. | "Yes, and where your policy demands a reason the system insists on one." |

## 8.2 Payments

| They ask | What is actually true today | What to say |
|---|---|---|
| "Does it take card payments?" | Cash, UPI, card and several other methods can all be recorded. None of them are processed. There is no payment gateway and no card machine connected. | "Today the system records how a bill was settled, and the money itself moves through your existing machine or UPI app. Connecting a gateway, so the card terminal is driven directly, is later work, and the system is already built to receive it." |
| "So the UPI code on the bill is not connected to anything?" | Correct. It is a standard UPI code for the right amount and payee. The customer's own app moves the money. | "It is an ordinary UPI code for that exact amount, so any app will pay it. What it does not do yet is tell the till by itself that it has been paid, so the cashier confirms it. Automatic confirmation is gateway work." |
| "What if the card machine says yes and the till says no?" | There is no link between them, so this can happen today. | "Today those are two separate records and your cashier reconciles them, which is what they already do. Removing that step is exactly what connecting the gateway is for." |
| "Can we run house accounts, or monthly credit for regulars?" | The payment method exists in the design, but there is no screen and no ledger behind it. | "It is in the design, but I would be overselling it to call it built." |
| "Is there a cash drawer and an end of day count?" | Opening and closing a shift, and paying money in and out, are built. | "Yes. Shifts open and close, and cash movements are recorded against them." |

## 8.3 Stock and purchasing

| They ask | What is actually true today | What to say |
|---|---|---|
| "Can I correct a stock figure when the count is wrong?" | Stock counts and wastage entries are built, on the till, with reasons. | "Yes, from the till. You can count, and you can record waste against a reason, which is the number you actually want at the end of the month." |
| "Can the owner see and fix stock from home?" | No. The back office shows orders, the menu, suppliers and received deliveries. There is no stock screen there. | "Stock lives at the restaurant today. The owner's view of it is the next piece of work, and it is the one I would put first after the pilot." |
| "Does it tell me what I should have used against what I actually used?" | The parts exist. The report does not. | "The system knows what you used, because it took it out by recipe, and it knows what you counted. Putting those two side by side as a variance report is reporting work, which is the next milestone." |
| "Can I raise a purchase order and receive against it?" | Both built. Receiving works even with no purchase order at all, deliberately. | "Yes. And importantly a delivery is never blocked because the paperwork is missing. Goods standing at your back door get received, and the mismatch is recorded for the buyer to sort out afterwards. Refusing a delivery is worse than recording an oddity." |
| "Batch numbers and expiry dates?" | Captured at receipt. Nothing acts on them yet. | "They are captured when goods are received, because that is the only moment they can be captured. Nothing acts on them yet, and expiry alerting is later work." |
| "Does it handle a central kitchen, or stock moving between branches?" | Not built. A later milestone on purpose. | "Not yet. That is multi branch work, and it comes after reporting." |

## 8.4 Reporting and the back office

| They ask | What is actually true today | What to say |
|---|---|---|
| "Where are my sales reports?" | The back office lists orders. There are no summaries, no charts and no daily totals. | "Today you can see every order. Proper reporting, your day summaries and your item performance, is the next milestone, and it is deliberately after the things that have to be correct first. Reports built on wrong numbers are worse than no reports." |
| "Can I see which dishes make me money?" | Not built. | "Menu profitability needs recipe costing, which exists, joined to sales, which also exists. It is reporting work rather than new plumbing." |
| "Can I see it on my phone?" | The back office is a web page and will open on a phone, but it has not been designed or tested for one. | "It is a web page, so it opens. I would not claim it is designed for a phone yet." |
| "Do the kitchen tickets and the bills reach the cloud?" | Only orders travel up today. | "Orders do. Kitchen tickets, bills and stock counts stay at the restaurant for now. The restaurant is the authority by design, and we are widening what travels up one piece at a time." |

## 8.5 Delivery apps and other systems

| They ask | What is actually true today | What to say |
|---|---|---|
| "Does it connect to Swiggy and Zomato?" | The framework for receiving an outside order is built and has been tested, but only against our own test harness. There is no live connection to any platform. | "The plumbing for outside orders is built, and an order from a delivery platform arrives at the till like any other order. What we have not done is the commercial onboarding with a particular platform, which is their paperwork and their approval rather than engineering." |
| "What about ONDC?" | The same position. Built to the shape, never connected. | The same answer. Please do not demonstrate it. |
| "Can it talk to our accounting software?" | No integration exists. | "Not today. The bills are correct and can be exported, which is the hard part. A direct connection is a known piece of work rather than a research project." |
| "Can we use our existing printers?" | The output is standard thermal printer output, but no physical printer has ever been connected to it. | "It produces exactly what a thermal printer expects. I have not connected one, and I am not going to pretend otherwise. Proving it on your hardware is the first thing in the pilot." |

## 8.6 Staff, security, and the questions their IT person asks

| They ask | What is actually true today | What to say |
|---|---|---|
| "Can I control what staff are allowed to do?" | Roles and permissions are built and enforced. | "Yes. Staff have roles, and the system checks them." |
| "Can a waiter give himself a discount?" | Permissions govern who may apply a discount, and some discounts require a reason. | "Your policy decides that, and where a reason is required the system will not let the discount through without one." |
| "Where is the data actually kept?" | Encrypted on the till, with a copy in the cloud. | "On the till, encrypted, with a copy in the cloud. The restaurant's copy is the authoritative one." |
| "What if somebody steals the till?" | The database is encrypted at rest. | "The database is encrypted, so the machine on its own is of no use to anybody." |
| "Two waiters on the same table at the same time?" | Handled. The table reports its open order and the new round is added to it. | "They add to the same order rather than opening two." |
| "What if a waiter's phone loses signal halfway through sending an order?" | If a send fails and the waiter sends again, the same round can be recorded twice as duplicate lines. It will not create a duplicate order or a second bill. | "It will not open a second bill. It can, in a bad moment, put the same round on twice, and the cashier sees that before billing. Making it impossible means each send carrying its own reference, and that is pilot work." Please do not volunteer this one. |

## 8.7 Three answers worth rehearsing out loud

**"What is actually not finished?"**

> "Three things worth your attention. Corrections, meaning cancelling an order
> or a bill from the screen. The owner's view of stock from home. And proper
> reporting. Everything underneath those is built and working, which is why they
> are weeks of work rather than months."

**"Why should we trust this over an established product?"**

> "You should not trust it because I say so. Look at what it does when the
> internet dies, because that is the thing established products handle worst,
> and it is the thing that costs you money on a Saturday night. Everything else
> is catching up on features. That part is a design decision made at the
> beginning, and it cannot be added later."

**"What happens in the pilot?"**

> "We install it in one outlet with your real menu and your real tax details,
> connect your printer, train your staff, and run it alongside what you do now
> until you are bored of it working. The corrections work I mentioned lands
> during that period, because your floor is where we find out which of them you
> actually need first."
