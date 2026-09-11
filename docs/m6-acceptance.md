# M6 acceptance evidence

**The record, not the chat.** A milestone does not close until its acceptance
evidence is committed to the repository: M5's criteria 1, 3, 4 and 6 were all
observed on real screens and then reported as unobserved by the next session,
which was holding the commit made *because* of the run that observed them. Every
row below names what was observed, how the precondition was established, who
observed it, and on what date — or says plainly that nobody has observed it yet.

**"C1".."C8" here mean M6's criteria** (`docs/m6-planning.md`), never M5's. M5's
seven are closed in `docs/m5-acceptance.md`; its criterion 7 (weighted average
cost) is a different thing entirely from **M6 C7**.

**Two classes of evidence, never merged.** *Executed* means a command was run and
its output read. *Observed* means a person watched the shipping binaries do it.
**An acceptance criterion is closed only by Observed**: a test harness is not an
acceptance run (`docs/retro.md`, 2026-08-11), and the falsifier must have been
watched failing first (§66).

---

## Phase C evidence quality — what generating the fake from published artefacts actually bought

**The strongest argument this milestone has produced for generating a fake from
published artefacts rather than writing one, and it is not a hypothetical.**

The plan's rule (C-2a) was that a self-authored fake proves only that we agree
with ourselves. The concrete failure it caught was not a field name or a
format — it was **which side of the protocol we are on**.

A restaurant is the **seller**, the BPP. A buyer app sends `confirm`; we answer
`on_confirm`. The first cut of the ONDC adapter parsed `on_confirm` as an
inbound order, which is the **buyer's** view of the protocol — the whole
integration pointed backwards.

**It passed everything.** ONDC's request and callback envelopes both carry
`message.order`, so the payload parsed cleanly, the adapter produced a
well-formed order, and every assertion held. The code was right about the JSON
and wrong about the direction.

**A hand-written fake could not have caught it, by construction.** We would have
authored a fixture from the same misunderstanding, and it would have agreed with
the adapter perfectly — the test constructing its own subject, in the one place
where no external check exists until certification. What caught it was fetching
ONDC's own `confirm` example and finding that the message we thought we received
is the one we are supposed to send.

The mistake also left a trace in the data, which is worth keeping as the tell: a
buyer-originated `confirm` states the order as **`Created`**, where the seller's
`on_confirm` says **`Accepted`**. Two different words for two different
directions, and the fixture that used the wrong one asserted the wrong word.

**What this does and does not license.** It raises confidence in the SHAPE of
the adapter and in nothing else. C8 is still recorded `SHAPE ONLY — no
integration evidence`: both fakes are ours, no message has been exchanged with
an ONDC system, and the integration row travels to M6.1 as an explicitly unmet
criterion. The artefact-generated fake is the strongest available check on shape
and the weakest possible check on integration — both halves of that sentence
belong here.

---

## THE OBSERVATION SITTING — C1, C5, C6 and C8 in one run

**PARTLY RUN — see "SITTING RUN OF 2026-09-10" below for where it stands.** The
plan was written out before the run so the preconditions are established
deliberately rather than discovered mid-sitting, which is how the 2026-09-05 C7
attempt was lost. It is kept here unchanged as the plan; the run's state lives in
its own section.

**Order matters and is not arbitrary.** C1's document must arrive **while the
cloud is reachable**, because ADR-022's guarantee is precisely that a NEW
aggregator order cannot arrive while the uplink is down. Establish the offline
state after the document is on the till, never before.

### Step 0 — the stack, verified by identity

1. Backend in its own window: `.\scripts\dev-up.ps1 -SkipInfra -SkipSeed -NoKds -NoPos`
2. `Get-NetTCPConnection -LocalPort 8080 -State Listen | Select-Object OwningProcess`
   — **record the PID and confirm it is NEW.** The port answering proves
   nothing: an old process answers identically, and that has already cost this
   project a debugging detour.
3. Bootstrap by hand with the backend already listening (the `dev-up.ps1`
   ordering defect): `[3b/4]` must say **enrolling** or **rotating**, never
   SKIPPED. A till with no device token syncs nothing and the whole sitting
   proves nothing.
4. Till: `$env:HOLLER_SYNC_PUMP_INTERVAL_SECS = "10"; .\apps\pos\run-dev.ps1`

### C5 — supplier and pack size, FALSIFIER FIRST

**The falsifier runs before the fix, or its absence afterwards means nothing.**

5. **Receive goods BEFORE creating the supplier item.** Record a GRN at the till
   against an inventory item with no `supplier_item` row.
6. **Observe the `NO_SUPPLIER_ITEM` gap** on the gaps screen. Record the gap id
   and reason. *This is the falsifier: without it, step 9's silence is
   unfalsifiable.*
7. In the admin console → Suppliers, create the supplier and its pack size.
   **Choose the dimension explicitly** — the selector is empty by design (0.5.2:
   auto-filling it makes the mismatch check `x == x` and it can never fire).
8. Receive the same goods again at the till.
9. **Observe: converts exactly, and NO new `NO_SUPPLIER_ITEM` gap.** Record the
   entered quantity, the pack size applied and the base quantity, and check the
   arithmetic by hand.

### C6 — the receipt reads back (screen half)

10. Admin console → Goods receipts. Find the receipt from step 8.
11. **Record every field the screen shows**, not a summary of it:
    `entered_quantity_micro`, `pack_size_micro_applied`, `base_quantity_micro`,
    `quantity_dimension`, `line_total_paise`, and the nullable
    `purchase_order_id` / `supplier_id` / `purchase_order_line_id` **as nulls
    where they are null**.

**The edge-row comparison is step 21, at the very end.** It needs the till
CLOSED so the database seals, and closing it mid-sitting would mean reopening
for C1 — two clean shutdowns where one will do, and every extra shutdown is
another chance to leave the database unsealed (the 2026-09-05 finding).

### C8 — both adapters, SHAPE ONLY

13. Drive an order through the **Beckn adapter** against the artefact-generated
    fake, and one through the **sync-REST adapter** against its local fake.
    Record the receipt from each.
14. **Introduce a platform-specific branch in the core** — e.g.
    `if in.Platform == "ondc"` in `internal/aggregators/port.go` — and watch
    `node scripts/check-aggregator-boundary.mjs` go **RED**. Record the output.
    Remove it and watch it go green. *A boundary nobody has watched fail is not
    a boundary.*
15. Record C8 as **`SHAPE ONLY — no integration evidence`** in those words. Both
    fakes are ours; the contract shape is proven twice and the integration zero
    times.

### C1 — offline operation, and the negative half

**Both halves, and the order is fixed.**

16. **WITH THE CLOUD REACHABLE**, POST an order document to the local callback
    path so it lands in Postgres, and let the till's pull bring it down. Confirm
    the document is on the till — this is what "already received" means, and it
    cannot be established later.
17. **Make the cloud provably unreachable by the three-probe method**: stop the
    backend **by PID** (the one recorded at step 2), then run
    `scripts\check-cloud-unreachable.ps1` and require **all three probes to
    agree**. Watch that script print STOP with the cloud UP first, so its
    agreement afterwards means something.
18. **Bill, print and close the order at the till.** Record the invoice number
    and the print outcome.

    **THE PRINT IS EVIDENCED BY THE FILE-SINK TRANSPORT, NOT BY PAPER**, and
    the row must say so. No thermal printer exists in this environment; the
    ESC/POS-on-paper gate has been PARKED since 2026-08-20 and is an M3 exit
    gate, not an M6 one. The file sink proves the byte stream was produced and
    handed to a transport. It does not prove a device accepted it, and this row
    carries the same trigger as M3's: **"when a printer is sourced"**.
    Recording it as "printed" without that qualifier would quietly promote a
    parked gate to a passed one.
19. **The negative half:** with the cloud still unreachable, confirm **NO NEW
    aggregator order arrives**. That is not a bug to fix — it is ADR-022's
    published guarantee, and observing it is what makes the positive half
    meaningful rather than lucky.
20. Restart the backend (**new PID again**), confirm the till's outbox drains
    the order created in step 18.

### Step 21 — CLOSE THE TILL, THEN READ THE EDGE ROW (C6's second half)

**Last, and only once.**

21. Close the POS **from the launching terminal with `Ctrl+C`**, not by the
    window's X — closing the window leaves `holler-pos.exe` running with the
    database open and unsealed (observed 2026-09-05). Confirm with
    `Get-Process holler-pos` that nothing survives, and confirm the directory
    holds `edge.db.enc` alone: no `edge.db`, no `-wal`, no `edge.db.open-marker`.
22. Read the receipt row by the **sealed-copy method**: copy the sealed file,
    decrypt the copy, query it, destroy both, **original never opened**.
23. **Compare field by field against what step 11 recorded.** C6 is met when the
    two agree on every field including the nulls — not when they agree on the
    quantities a summary would show.

### What must be recorded, for every criterion

The observation, the artefact (screen, row, request log, PID), **who** observed
it and **when**. A verdict that exists only in a session transcript is erased by
a restart — M5 lost four criteria that way, and the session that lost them was
holding the commit made because of the run that observed them.

---

## SITTING RUN OF 2026-09-10 — IN PROGRESS, STOPPED AFTER STEP 6

**This section is the resume point.** It is written while the sitting is running,
not after it, so that a restart comes back to a known stage rather than
reconstructing one from git history. Steps refer to the numbered plan above.

**A prior attempt on 2026-09-08 reached the same screens and was abandoned**
when it found two defects, both fixed in `0b500d6`: the config pull refused
every incremental bundle that changed no user, and the aggregator routes were
never mounted in `main.go`. Nothing from that attempt is evidence.

### Preconditions established 2026-09-10, by Gaurav with Claude driving the stack

- Infra: `holler-postgres-1`, `holler-redis-1` healthy, `holler-nats-1` up,
  started with `docker compose up -d postgres redis nats`. **Not** `make dev` —
  the compose file's `backend` service fails to build and is not used here; the
  backend runs natively.
- Backend: **PID 12404, started 2026-09-09 20:46:10**, `/health` returning
  `{"status":"ok"}`. Verified by identity, not by the port answering. An earlier
  process (PID 37752) was killed because it had started before Postgres existed;
  port 8080 was confirmed free before the replacement was started.
- Edge database reseeded by `scripts\dev-bootstrap.ps1 -SkipInfra -WithBilling`,
  run by the operator because `apps\pos\.env.dev` carries the encryption key and
  is deny-ruled to the agent. `%APPDATA%\com.holler.pos\edge.db.enc`,
  1,384,476 bytes, written 2026-09-10 00:50, sealed — no `edge.db`, no `-wal`,
  no open-marker.
- Enrollment landed: `device_credential` row for device
  `01a05d10-cba7-7876-9958-65f9a6ca2fe7` at 2026-09-09 19:20:00Z.
- POS started by the operator with `HOLLER_SYNC_PUMP_INTERVAL_SECS=10`, signed
  in as `cashier@holler.test`, which carries `procurement.manage`.
- Admin dev server on :5175.

**One precondition differs from the plan and is recorded rather than corrected:
the cloud now holds 990 `menu_item` rows, not 2.** The cloud/edge menu seed drift
that C7's falsifier depended on is therefore gone. That is harmless — M6 C7 is
closed and its evidence is committed — but the absence must not be read as a fix.

### C5 — the falsifier, OBSERVED 2026-09-10 (steps 5 and 6)

**Item chosen: Paneer, `INV-PANEER`, dimension MASS.** It is the only seed
inventory item with no `supplier_item` row in either store — established by
querying the cloud for seed-range inventory items (`id::text LIKE
'0191e800-0000-7000-8000-%'`) with no matching `supplier_item`, which returned
exactly one row. Every other seed item already has one, and receiving against any
of them would have falsified nothing. This matters because the config bundle
carries `supplier` and `supplier_item` down to the till, so "the till has no
supplier item" is not a property of the seed alone.

Entered at the till: purchase unit `kg`, quantity `2`, declared dimension
**Weight (MASS)** chosen by the operator from the delivery note — the selector is
empty by design (contracts 0.5.2), and auto-filling it from the item would make
the check `x == x`. Price ₹400 per kg, no purchase order, no batch, no expiry.

**The echo, read off the screen before saving:**

> 2 kg → 2000g of Paneer
> 1 kg = 1000g
> Cost ₹0.40 per base unit · line total ₹800.00
> No agreed pack size for this supplier and unit

Checked by hand: 2 kg × 1000 = 2000 g base; ₹400/kg ÷ 1000 = 40 paise per gram;
2000 g × 40 paise = ₹800.00. Agrees.

**Saved as `GRN/20260910/0001`**, business date 2026-09-10, stock increased.
The receipt screen reported **two gaps**:

- `NO_PURCHASE_ORDER` — "Received with no purchase order — walk-in delivery,
  standing order or emergency purchase. The goods were received." Correct and not
  a defect: a GRN never blocks on a PO (ADR-019).
- `NO_SUPPLIER_ITEM` — "No supplier_item row for this item in unit \"kg\"; the
  rate was resolved from the unit label instead."

**This is C5's falsifier and it is now watched.** The absence of a
`NO_SUPPLIER_ITEM` gap at step 9 will therefore mean something.

### C5 — MET 2026-09-10 (steps 7, 8 and 9)

**Step 7.** Supplier `Deccan Dairy` / `DECCAN-DAIRY` created in the admin
console with its first pack size: Paneer, purchase unit `kg`, pack size `1000`,
dimension MASS chosen by the operator. The cloud row reads
`pack_size_micro = 1000000000` — 1000 g per kg, exact.

**A wrong turn worth recording, because it was mine and it looked like a
failure.** The first retry of step 8 left **Supplier reference blank**, as steps
5–6 had. `resolve_pack_rate` only consults `supplier_item` when a supplier is
given (`edge/database/src/procurement/convert.rs:194`), so with no supplier the
`NO_SUPPLIER_ITEM` gap is guaranteed **whatever the admin holds** — the receipt
recorded as `GRN/20260910/0002` and proves nothing either way. The field also
takes the supplier's **UUID**, not its code.

**Step 8, with `supplier_id = 4eed005a-8889-479f-b3a5-20fcb5d5fb2c`:** recorded
as `GRN/20260910/0003`. Echo before saving:

> 2 kg → 2000g of Paneer
> 1 kg = 1000g
> Cost ₹0.40 per base unit · line total ₹800.00

**Step 9 — the gap is GONE.** One gap remains, `NO_PURCHASE_ORDER`, which is
correct and permanent for a walk-in delivery. The `NO_SUPPLIER_ITEM` line, and
its "the rate was resolved from the unit label instead" prose, are absent. That
absence is the criterion, and it means something because the same receipt
produced the gap three receipts earlier.

**What this run does NOT discriminate, stated plainly.** Both the falsified and
the met receipts converted 2 kg to 2000 g, because the seeded unit-label
conversion for kg and the agreed pack size are numerically identical (1 kg =
1000 g). **The arithmetic is therefore not the evidence — the gap's presence and
absence is.** A future run wanting the arithmetic to discriminate too should use
a pack size the unit label cannot reproduce, e.g. a `SACK` of 50 kg.

**It also evidences `0b500d6` on the live path.** The supplier and its pack size
reached the till only by config pull. Before that fix every incremental bundle
that changed no user was refused forever, so this step could not have passed at
all — the config pull now has a production caller and it works.

**Defect found on the way, filed but not fixed here:** `apps/admin` mints ids
with `crypto.randomUUID()` (`SuppliersScreen.tsx:114` and `:139`), which is
**UUIDv4**, against §74's app-generated UUIDv7/ULID rule — the created supplier's
id is `4eed005a-8889-479f-…`, version nibble 4. Every supplier and supplier_item
created from the console carries one. Two smaller ones from the same screen: the
form collects **no GSTIN** (the row renders "no GSTIN"), and the pack-size table
shows the raw inventory item UUID rather than the item's name.

### C6 — the screen half, OBSERVED 2026-09-10 (steps 10 and 11)

**Subject: `GRN/20260910/0003`.** Read off the admin console's goods-receipt
screen by the operator. The header:

> Received 2026-09-09T20:19:02Z · business date 2026-09-10 · no purchase order ·
> supplier `4eed005a-8889-479f-b3a5-20fcb5d5fb2c`

The line, exactly as the screen renders it (quantities divided out of micro-units
for display):

| Item | Entered | Pack size | Base quantity | Dimension | Line total |
|---|---|---|---|---|---|
| `0191e800-0000-7000-8000-000000000001` | 2 | 1000 | 2000 | MASS | 800.00 |

**All three quantity fields travel, which is the point of the row** (ADR-024):
entered, pack size applied and base quantity are each shown, so "what did they
actually type?" is answerable from the screen alone.

**The nulls are served as nulls, not hidden or defaulted.** The header says "no
purchase order" while naming a supplier, and the underlying row confirms it:

```
line_number             1
inventory_item_id       0191e800-0000-7000-8000-000000000001
entered_purchase_unit   kg
entered_quantity_micro  2000000          (2 kg)
pack_size_micro_applied 1000000000       (1000 g per kg)
base_quantity_micro     2000000000       (2000 g)
quantity_dimension      MASS
unit_cost_paise         40               (per gram)
line_total_paise        80000            (₹800.00)
purchase_order_line_id  NULL
batch_code              NULL
expiry_date             NULL
goods_receipt_note.purchase_order_id  NULL
goods_receipt_note.supplier_id        4eed005a-8889-479f-b3a5-20fcb5d5fb2c
```

`line_total_paise = 80000` with `unit_cost_paise = 40` satisfies contracts
0.6.3's directional CHECK — a total never without its rate — and the rate
recomputes from the total exactly: 80000 × 10⁶ ÷ 2000000000 = 40.

**Note the business date, which is correct and looks wrong.** Received
2026-09-09T20:19:02**Z**, business date **2026-09-10**: that is IST past midnight,
bucketed by `outlet.day_start_time` rather than by the UTC calendar day. This is
the M3-era defect that `compute_business_date` fixed for the stock ledger; the GRN
path uses the corrected function.

**What remains for C6 is step 23**, the field-by-field comparison against the
edge's own row, which needs the till closed and the sealed-copy read. Until then
this half evidences the cloud replica only — and the screen must be read as a
replica, whose figures may legitimately differ from the outlet's own view
(ADR-024).

### C8 — SHAPE ONLY, no integration evidence. Observed 2026-09-10 (steps 13, 14, 15)

**Recorded in those words, as the plan requires: `SHAPE ONLY — no integration
evidence`.** Both fakes are ours, no message has been exchanged with any ONDC
system, and the integration row travels to M6.1 as an explicitly unmet criterion
(M6.1 C1). The artefact-generated fake is the strongest available check on shape
and the weakest possible check on integration.

**Step 13 — one order through each adapter, on the shipping backend** (PID 12404),
driven over HTTP with the till's own device bearer token, not through a test
harness.

The **Beckn/ONDC** adapter, against the unmodified artefact-generated fixture
`backend/internal/aggregators/adapters/beckn/fixtures/confirm.json` — a buyer-app
`confirm`, which is what a BPP receives:

```
POST /aggregators/ondc/callback  ->  HTTP 200
{"external_order_id":"O1","applied":true,"duplicate":false,"unmapped_lines":2}
```

The **syncrest** adapter, against the local synchronous REST fake — a flat
envelope with paise on the wire, structurally unlike Beckn's `context`/`message`
shape:

```
POST /aggregators/syncrest/callback  ->  HTTP 200
{"external_order_id":"SR-20260910-001","applied":true,"duplicate":false,"unmapped_lines":2}
```

**Two protocol shapes, one core, same receipt shape back.** `unmapped_lines: 2`
on both is correct and not a failure: no `external_item_id` mapping exists for
either fake's items, and an unmappable line is recorded rather than refused
(ADR-022 rule 4).

**Step 14 — the boundary check watched FAILING, then passing.** A
platform-specific branch was planted in the core, in `ToAggregatorOrder`:

```go
if in.Platform == "ondc" {
```

`node scripts/check-aggregator-boundary.mjs` then printed, exit 1:

```
check-aggregator-boundary: platform vocabulary outside its adapter

  backend\internalggregators\port.go:148  "ondc"
      if in.Platform == "ondc" {

1 violation(s) across 295 scanned file(s).
```

The branch was removed and the check printed `OK — 295 file(s) outside the adapter
directory carry no platform vocabulary`, exit 0, with `git status` clean on
`port.go`. **A boundary nobody has watched fail is not a boundary**; this one has
now been watched failing and recovering, in that order.

### C1 — what "CODE COMPLETE" hid, and the accept path built 2026-09-10

**This criterion was recorded CODE COMPLETE and could not be observed at all.**
Found while trying to run step 16: the cloud→edge down-path did everything it
claimed — `pull_and_apply_aggregator_orders` → `apply_aggregator_order` writes
`aggregator_order` and `aggregator_order_line` into edge SQLite on the A5 loop —
and then stopped. Enumerated rather than eyeballed:

- `repo::list_unaccepted_aggregator_orders` and `repo::aggregator_order_acceptance`:
  **zero non-test callers**, only `edge/database/tests/aggregator_mirror.rs`.
- `Repository.ResolveMenuItem` in the cloud: **zero callers**, because
  `main.go` passed `ResolveItem: nil`, so every inbound line was unmapped no
  matter what `aggregator_item_map` held.
- `apps/pos/src-tauri/src/commands/`: no aggregator command. POS routes were
  `/`, `/orders`, `/orders/$orderId/billing`, four inventory, three procurement,
  `/login`. **No aggregator surface existed.**

So a document landed on the till and nothing could turn it into an order, bill
it, print it or close it. **"Code complete" was read off the down-path, which is
the half that was written.** Same family as contracts 0.5.2 and 0.5.9 — a column
nothing reads, now a query nothing calls — and the lesson is the repository's
own: count the SINKS, not the surfaces. A screen can be missed; a write path
cannot.

**What was built, deliberately minimal** (one commit, 2026-09-10):

- `Db::list_unaccepted_aggregator_orders` / `Db::list_aggregator_order_lines`
  and `repo::list_aggregator_order_lines` — the read surface the POS crate needs,
  since it has no `rusqlite` and cannot SELECT for itself.
- `commands::aggregator` — list, and accept. **Accepting a document IS creating
  its local order**: no acceptance flag exists anywhere, because
  `aggregator_order` is a read-only mirror of a cloud-authoritative aggregate and
  an edge-written column on it is the split authority ADR-022 exists to avoid.
  The order carries `external_order_id` and replays up the ordinary outbox.
- `DraftOrderInput` gained `source`, `external_order_id` and
  `source_payload_json`; the till path passes `POS`/null/null exactly as before.
- `/aggregator-orders` screen and a **Platform Orders** button on the till
  header — because a screen nothing navigates to is the same defect one layer out.
- `main.go` now supplies the item resolver, scoped by a `Scope` the service puts
  on the context. **A missing scope resolves nothing rather than querying
  unscoped**: an unmapped line is a normal recorded outcome, a line mapped from
  another tenant's menu is a data leak.

**What it deliberately does NOT do:** no reject, no platform-side cancel
visibility — both filed in `docs/backlog.md` with the trigger *before any
platform sandbox access*, because their shape is argued from what a real platform
does with a refusal and no fake we authored can answer that. An **unmapped line
is skipped and COUNTED**, never silently dropped (`order_item.menu_item_id` is a
real NOT NULL FK), and a document whose every line is unmapped is refused by name
— `AGGREGATOR_ORDER_NO_MAPPED_LINES` — rather than producing a ₹0 bill.

**One contract limit recorded rather than worked around:** `order.source`'s CHECK
(contracts 0004) is `POS`/`QR`/`AGGREGATOR_ZOMATO`/`AGGREGATOR_SWIGGY`/`DIRECT`
and cannot name ONDC. The accept path writes `DIRECT` and keeps the platform's
identity in `source_payload_json`; widening the CHECK is a frozen-contract change
and is filed, not done here.

**Verified: builds and checks only, no observation.** `cargo build` on
`apps/pos/src-tauri`, `go build ./...`, `tsc --noEmit` clean, 230 POS vitest tests
executed through `scripts/assert-tests-ran.mjs`, 5 `aggregator_mirror` tests,
all three `check-seams` targets, and `check-aggregator-boundary` still OK across
295 files. **None of that is C1**, which needs the sitting.

### C1 — MET 2026-09-10, both halves, on the shipping binaries

**The accept path this criterion needs did not exist when the sitting began** —
see the section above. It was built the same day (`3589f63`), and this run is the
first time an aggregator order has been billed at a Holler till.

**Preconditions, established in the order ADR-022 requires.** Item mappings were
inserted for the two fixture SKUs (`aggregator_item_map` has no product surface;
the direct insert is recorded here rather than implied to be a screen), and
HSN/SAC was set on both cloud menu items through `PATCH /menu/items/{itemId}` —
**without it the invoice could not have issued at all** (0.4.5), and both cloud
rows carried NULL, which is a filed defect of the cloud seeder.

**Step 16, WITH THE CLOUD REACHABLE.** `POST /aggregators/syncrest/callback`
returned `{"external_order_id":"SR-C1-20260910-090521","applied":true,
"duplicate":false,"unmapped_lines":0}`. **`unmapped_lines: 0` is the first time
`Repository.ResolveMenuItem` has ever resolved anything** — it had no caller at
all until this run. The till's periodic pull brought the document down
(`periodic pulled 1 aggregator order document(s)`) and it appeared on **Platform
Orders** with both lines matched to real menu item ids.

**Step 17, the cloud made provably unreachable, falsifier watched FIRST.**
`scripts\check-cloud-unreachable.ps1` printed **STOP - cloud is REACHABLE** with
the backend up, naming pid 47548 on all three probes. The backend was then
stopped **by that pid**, and the same script printed all three probes agreeing:
nothing listening, TCP refused, HTTP no answer.

**Step 18, with the cloud down — bills, prints and closes.**

- Accepted `SR-C1-20260910-090521`, creating local order **#A1** (`order_type`
  AGGREGATOR, `source` DIRECT, `external_order_id` set).
- Invoice **DEV/000002** ISSUED: Masala Chai 2 × ₹40.00 = ₹80.00 taxable, Veg
  Thali 1 × ₹220.00, CGST ₹7.50, SGST ₹7.50, **Grand Total ₹315.00**. Tax
  computed at the edge and matching the printed bill exactly.
- **Printed to the file sink**: `20260910T035143.331453400-Dev-Bill-Printer.escpos`
  (721 bytes) plus its readable `.txt`, carrying `HSN/SAC 9963` on both lines.
  **THIS IS EVIDENCED BY THE FILE-SINK TRANSPORT, NOT BY PAPER.** It proves the
  byte stream was produced and handed to a transport; it proves nothing about a
  device accepting it. The ESC/POS-on-paper gate stays PARKED with its trigger
  *when a printer is sourced*.
- Cash shift `01a08974` opened; **CASH ₹315.00 CAPTURED**; Amount Due **₹0.00 —
  settled**.

**Step 19, the negative half.** With the cloud still stopped, Platform Orders
listed only the two pre-existing documents and **no new one**, while the log
repeated `periodic aggregator pull failed (http transport error contacting
cloud); keeping the documents this till already holds`. The accepted document had
left the list, which is acceptance derived from the local order's existence, not
a stored flag. **That absence is ADR-022's published guarantee, observed rather
than assumed**, and it is what makes the positive half meaningful rather than
lucky.

**Step 20, the cloud returns.** Backend restarted on a **NEW pid 49600**
(previous 47548), verified by identity rather than by the port answering. The
till's next drain published the order, and Postgres holds it linked by the only
link there is:

```
AGGREGATOR | DIRECT | SR-C1-20260910-090521 | 30000 | DRAFT
```

**Two things that row shows, which this record does not paper over:**

- **`display_number` is empty in the cloud, for this order and every other one.**
  The outbox event is built from the pre-insert DTO while `#A1` is minted
  transactionally by the insert, so the number never travels. Pre-existing, not
  introduced by the accept path, and filed.
- **The cloud says DRAFT at ₹300.00 while the till has it billed, printed and
  settled at ₹315.00.** That is **A7** exactly as carried: `invoice` and
  `payment` have no edge route, so the cloud cannot know. Consistent with the
  known gap rather than a new defect — and a reminder that A7 must close before
  any aggregate beyond `order` is expected to replay.

**A defect found and fixed mid-run, on money.** The billing screen showed a
settled bill as fully due, because the Tauri DTO hardcoded `allocations: []` on
every payment behind a stale comment claiming allocation was unimplemented at the
edge. The screen attributes payments to invoices through that list, so every
payment was invisible and only the edge refusing a second tender stood between a
settled bill and a customer paying twice. Fixed in `fc38006` and **verified on the
same running app**: the same order went from "₹315.00 due, no payments" to "₹0.00
— settled, CASH ₹315.00 CAPTURED" against a payment already in the database.

### C6 — MET 2026-09-10. Step 23, the edge row, field by field

**Read after the till was closed**, from `GRN/20260910/0003`, and compared
against what the admin console showed at step 11 and against the Postgres
replica. Every field agrees, including the nulls:

| field | edge row | admin screen / cloud replica |
|---|---|---|
| `entered_quantity_micro` | 2000000 | 2 kg / 2000000 |
| `pack_size_micro_applied` | 1000000000 | 1000 / 1000000000 |
| `base_quantity_micro` | 2000000000 | 2000 / 2000000000 |
| `quantity_dimension` | MASS | MASS |
| `unit_cost_paise` | 40 | ₹0.40 / 40 |
| `line_total_paise` | 80000 | ₹800.00 / 80000 |
| `entered_purchase_unit` | kg | kg |
| `purchase_order_id` | NULL | "no purchase order" / NULL |
| `purchase_order_line_id` | NULL | NULL |
| `batch_code` / `expiry_date` | NULL / NULL | NULL / NULL |
| `supplier_id` | 4eed005a-8889-479f-b3a5-20fcb5d5fb2c | same |
| `received_at` / `business_date` | 2026-09-09T20:19:02.387Z / 2026-09-10 | same |

**The nulls agree AS NULLS**, which is the half a summary comparison would have
skipped (contracts 0.5.9's lesson: a fidelity test proves fidelity only for the
fields its fixture populates). The row's only `grn_gap` is `NO_PURCHASE_ORDER` —
**no `NO_SUPPLIER_ITEM`** — which corroborates C5 in storage rather than only on
a screen.

**How it was read, and what was NOT protected this time.** The M5 method is:
copy the sealed file, decrypt the copy, query it, destroy both, original never
opened. **That method could not be followed, because the till never sealed.**
See the A6(b) observation below: `Ctrl+C` in the terminal did not fire
`RunEvent::Exit`, so `edge.db`, `edge.db-wal` and `edge.db.open-marker` were all
still on disk in plaintext and `edge.db.enc` was stamped with the time the app
OPENED. The read was therefore done in place, `mode=ro`, against the plaintext
the failed shutdown had already left — which creates **no new** unencrypted
artefact and never touches the sealed file, but it is a weaker posture than M5's
and is recorded as such rather than described as a sealed-copy read.

### A6(b) — observed again 2026-09-10, this time with a clean Ctrl+C

The shutdown seal did not run. After `Ctrl+C` in the launching terminal and
`Get-Process holler-pos` returning nothing, the data directory still held:

```
edge.db              1404928  10-09-2026 10:27:27
edge.db-shm            32768  10-09-2026 10:27:27
edge.db-wal           469712  10-09-2026 11:56:05
edge.db.enc          1404956  10-09-2026 10:27:27
edge.db.open-marker        0  10-09-2026 10:27:27
```

`edge.db.enc` carries the timestamp of the moment the app **opened** (10:27:27,
when crash recovery resealed the previous session), while the WAL is from
11:56:05 — so **the sealed file does not contain tonight's billing at all** and
the plaintext beside it does.

**This matters beyond tidiness for two reasons.** The plaintext carries the
cached Argon2id `password_hash` and `pin_hash` rows that exist so a cashier can
log in offline, and it will sit there until that database is next opened. And a
reader who does the careful thing — decrypt the sealed copy — gets an OLDER state
than the plaintext beside it, with nothing announcing the difference.

The backlog entry for A6 says the window-close case does this; **this run shows a
deliberate `Ctrl+C` from the launching terminal does it too**, which is the
documented correct way to stop the app. The entry is updated accordingly.

### Where the run stands

| Step | State |
|---|---|
| 0 (stack, by identity) | Done — PIDs and file state above |
| 5–6 (C5 falsifier) | **Observed**, above |
| 7 (create supplier + pack size in admin) | **Done** — `Deccan Dairy`, id `4eed005a-8889-479f-b3a5-20fcb5d5fb2c` |
| 8–9 (receive again, no gap) | **Observed — C5 MET** on `GRN/20260910/0003`, above |
| 10–11 (C6 screen half) | **Observed** on `GRN/20260910/0003`, above; step 23 still owed |
| 13–15 (C8, both adapters + boundary check RED) | **Observed — C8 met, SHAPE ONLY**, above |
| 16–20 (C1, both halves) | **Observed — C1 MET**, above: falsifier watched first, cloud stopped by pid, order billed/printed/settled offline, replayed on a new pid |
| Step 16's precondition, DONE | The two documents already posted tonight (`ondc`/`O1`, `syncrest`/`SR-20260910-001`) arrived BEFORE the resolver was wired, so every line is unmapped and accepting either is correctly refused with `AGGREGATOR_ORDER_NO_MAPPED_LINES`. C1 needs `aggregator_item_map` rows for the item ids a fixture carries, and a document posted AFTER the new backend is up |
| 21–23 (close the till, edge-row read, C6 field-by-field) | **Observed — C6 MET**, above. The till did NOT seal on Ctrl+C (A6(b) again), so the read was in-place read-only rather than a sealed copy |

### Step 7's field values, worked out on 2026-09-10 and not to be re-derived

The admin "Add a supplier" form takes a raw inventory item id and a pack size in
base units, and both were typed wrong on the first attempt. Correct values:

- Code `DECCAN-DAIRY`, Name `Deccan Dairy`.
- **Inventory item id `0191e800-0000-7000-8000-000000000001`** (Paneer).
- **Purchase unit `kg`** — must match the till entry in step 5 exactly, or the
  next receipt will not find the pack size and step 9 proves nothing.
- **Pack size `1000`** — base units per purchase unit, and Paneer's base unit is
  the gram. `0.5` says a kilo weighs half a gram and would convert 2 kg into 1 g.
- Dimension **MASS**, chosen from the supplier's own statement. The selector is
  empty by design (contracts 0.5.2); auto-filling it from the item makes the
  comparison `x == x` and the guard can never fire.

**To resume after a restart:** verify the stack by identity again — the backend
PID above is the one to expect, and a different PID means the observations below
step 0 were made against a process that no longer exists. Do not re-run steps 5
and 6: the falsifier is watched, `GRN/20260910/0001` exists, and receiving Paneer
a second time before step 7 would muddy step 9's silence.

---

## Phase B — CLOSED 2026-09-08 WITH THREE SURFACES BUILT AND TWO CARRIED, NOT ALL FIVE

**Say it in those words.** Phase B was scoped as five admin surfaces. **Three
were built: menu and pricing, suppliers and pack sizes, the goods-receipt
list.** **Two were deferred: purchase orders, and staff and permissions**, each
in `docs/backlog.md` with the trigger *before the first pilot*. Staff and
permissions is pilot work rather than milestone work: today the only way to
create a user is `devseed`, so a real outlet would have one cashier and no way
to add, remove or lock out staff.

| Surface | State |
|---|---|
| Menu and pricing | **BUILT** — observed rendering with live data, 2026-09-08 |
| Suppliers and pack sizes | **BUILT** — observed, 2026-09-08 |
| Goods-receipt list | **BUILT** — observed, 2026-09-08 |
| Purchase orders | **DEFERRED** — backlog, before first pilot |
| Staff and permissions | **DEFERRED** — backlog, before first pilot |

### The observation, 2026-09-08, by the operator

All three tabs rendered in a real browser at `http://localhost:5175`, signed in
as `owner@holler.test`, against the running backend and a live Postgres.
Screenshots taken of each.

- **Menu and pricing** — Masala Chai at **45.00** and Veg Thali at 220.00, with
  categories, the availability column, and the "edits change the cloud, not the
  tills" note. The 45.00 is itself evidence: the item seeds at 4000 paise and
  reads 4500 because `PATCH /menu/items/{itemId}` was exercised against this
  database and the change persisted.
- **Suppliers and pack sizes** — Pune Grain Traders (SUP-GRAIN), one pack size
  of 50000 base units against purchase unit SACK, dimension MASS shown as
  stored, and the create form with an **empty** dimension selector.
- **Goods receipts** — four receipts (GRN/20260902/0003 down to
  GRN/20260901/0003), each with **Entered / Pack size / Base quantity** as three
  separate columns, `4 x 25000 = 100000` consistent on every line, line totals
  formatted as money, and `no purchase order · no supplier recorded` where those
  links are null.

**`hsn_sac` reads "not set — cannot be billed" on both menu items, and that is
CORRECT.** `backend/cmd/devseed/main.go` inserts `menu_item` without an
`hsn_sac` column at all, so the cloud's two rows genuinely carry NULL. Filed as
a seed gap. It is worth recording precisely because it looks like the wire
defect it is not: before the fix a missing KEY made Zod reject the whole
response, and now a real NULL renders as an absence.

### Five defects found between "build green" and "screens working"

Every one invisible to `tsc`, `pnpm build`, and both test suites — which is
CLAUDE.md's build-green-is-not-dev-works rule demonstrated five times in one
sitting.

| # | Defect | Fix |
|---|---|---|
| 1 | **No CORS anywhere in the API.** Every browser that had ever called it was same-origin or not a browser: the POS is a Tauri window, the KDS talks to the edge over the LAN. `apps/admin` is the first cross-origin browser client this API has ever had | `eb8fb38` |
| 2 | **The client hand-wrote the login principal** and declared an `email` field it has never carried. Sign-in authenticated, then failed at the parse step | `153bbb4` |
| 3 | **Stale Vite prebundle** — `@holler_contracts.js` was 6.5 hours older than the source, so new exports resolved to nothing and the page was blank. Same defect as the POS white screen of 2026-08-20 | `b7bfefb` |
| 4 | **`itemWire` carried 7 fields where `MenuItemSchema` declares 10** — `tax_profile_id`, `hsn_sac` and `schema_version` were in the database, the domain type and the SELECT, and absent from the struct that goes on the wire. Shipping since 0.4.2 | `3371b91` |
| 5 | **`SupplierWithItems` was hand-written as `{supplier, items}` in a bare array.** The wire is flat, inside a `{suppliers: […]}` envelope | `3371b91` |

**Two of the five were mine (2 and 5), and both have the same cause: I wrote a
shape from memory instead of reading the contract.** Both now come from
`packages/contracts`. Defect 4 is the same failure in the other direction and
had been shipping for months — **the admin console is the first strict client
this API has ever had, and it found a months-old wire defect within minutes of
first contact.**

---

## Phase A — CLOSED 2026-09-07 WITH THREE OF SEVEN GAPS CARRIED, NOT ALL SEVEN LANDED

**Say it in those words.** Phase A was scoped as seven sync gaps (A1–A7).
**Five landed: A1, A1b, A2, A3, A5.** **Three were deferred: A4, A6, A7**, each
filed in `docs/backlog.md` with the trigger *before the first pilot*. A report
that says "Phase A complete" without naming the three is wrong, and the largest
of them is not cosmetic — A7 means **78 outbox rows on the live edge database
have no route and can never be sent** (55 `kot`, 22 `stock_count`, 1 `invoice`,
measured 2026-09-07).

| ID | Gap | State |
|---|---|---|
| A1 | Cloud returns 500 for a client-data failure | **LANDED** — one SQLSTATE classifier, `99875cc`, `ab6c201` |
| A1b | Two more ingest paths report client-data failures as 500/404 | **LANDED** — `856616b`, `3f7abaa` |
| A2 | Head-of-line blocking strands the whole outbox | **LANDED** — `07d7968`, `59c2ea3` |
| A3 | Retry budget never spends; nothing surfaces a blocked row | **LANDED** — `c95dc24`, `078d4e5`, `c8147ef` |
| A5 | No periodic sync pump | **LANDED** — `4d12363` |
| A4 | `Offline` conflates four states | **DEFERRED** — reporting half only; backlog, before first pilot |
| A6 | Shutdown drain silent; window close does not exit the process | **DEFERRED** — backlog, before first pilot |
| A7 | ~120 rows pending with no edge route | **DEFERRED** — 78 rows measured; backlog, before first pilot |

**The sequencing invariant held.** *At no commit boundary may an order become
droppable with no operator trace.* A1 landed alone and held the row; A2 retained
it without making it visible, and its commit message said so; A3 added the
budget and the surfacing; A5 made the surfacing reachable without a restart
loop. The 2026-09-07 C7 run is the end-to-end evidence for that chain.

**What Phase A was for, and whether it is met.** The stated reason to do sync
before aggregators was that aggregator orders ride the same outbox, and a wedged
outbox would bury the same defect twice. That is met for the `order` stream: a
permanently-refused row blocks itself, is charged, is surfaced, and its
neighbours drain. It is **not** met for `kot`, `invoice`, `payment`, `cash_shift`
or `stock_count`, which A7 leaves unroutable. **Contracts may proceed to 0.7.0
on the strength of the order stream; A7 must be closed before any aggregate
beyond `order` is expected to replay.**

---

## M6 BOUNDARY LIST — WORK THAT MUST LAND BEFORE M6 CLOSES

Not the backlog. The backlog holds what M6 ships without; this list holds what M6
cannot close without. An item arrives here only by decision, with the reason
recorded.

### 1. Contracts 0.8.1 — `order.source` must be able to name the platform — **LANDED 2026-09-11 (ADR-026)**

**STATUS: LANDED**, with one deliberate deviation from the item as written below
and one part explicitly deferred. Read this block before the original, which is
kept for its reasoning.

**The deviation: ONE GENERIC `AGGREGATOR` MEMBER, NOT A PLATFORM-NAMED ONE.**
The item said the column must "name the platform". It cannot, and should not.
`aggregator_order.platform` is free `TEXT` because contracts 0032 decided a new
platform must not require a migration, and the aggregator boundary check exists
to keep platform names out of the core — so a per-platform member would reverse a
recorded decision and re-create the same lie one platform later. The platform is
already stored twice, in `aggregator_order.platform` and
`order.source_payload_json`. Escalated before writing anything and ruled on by
Gaurav; the premise was a review-partner error, recorded as such in ADR-026.

**The consumer list is done:** sqlite 0035 (a table REBUILD, since SQLite cannot
`ALTER` a CHECK), postgres 0035, the Go constants, the Zod enum, the OpenAPI
enum, and `scripts/check-order-source-drift.mjs` proving all five agree — watched
RED with a member removed from one store before being trusted.

**The edge writer deliberately still writes `DIRECT`.** Switching it is its own
change, and until then "declared but never emitted" is the correct state for both
new members, pinned by exact assertion rather than left to memory: the drift
check fails the build if anything under `edge/` or `apps/pos/src-tauri/src`
emits `AGGREGATOR` or `TABLE_TAB`, and the writer that starts emitting one
removes it from that list in the same commit.

**0035 is applied in POSTGRES ONLY so far.** The edge takes it at the next clean
bootstrap: applying it to the live edge database would rebuild a file for which
no trustworthy backup can currently be taken (the `.enc` is only as current as
the last successful seal, and the sealing defect is open). Nothing writes the new
members, so a lagging edge schema changes no behaviour. Filed with the runner fix
in `docs/backlog.md`.

**Two boundary-check holes were found and closed while landing this**, both by
planting a value and watching it pass: `packages/contracts` was outside the
check's search roots, and its word-boundary pattern could not match
`AGGREGATOR_ONDC` because `_` is a word character. The check that exists to keep
platform names out of the core could not see the schema, and could not have
matched the most likely form. Both fixed, both re-planted afterwards to confirm
the fix catches what the plant proved it missed.

---

#### The original item, kept for its reasoning

**Decided 2026-09-10 by Gaurav, pulled FORWARD out of `docs/backlog.md`**, where
it had been filed with the trigger *before the first pilot*. The accept path built
today writes `source = 'DIRECT'` for an order that came from ONDC, because
contracts 0004's CHECK is the closed set
`POS`/`QR`/`AGGREGATOR_ZOMATO`/`AGGREGATOR_SWIGGY`/`DIRECT` and none of its
members is true of an ONDC order.

**Why it cannot wait, in the words of the decision:** an aggregator order
recorded as a walk-in is wrong in every report that groups revenue by channel,
and carrying the platform in `source_payload_json` instead is exactly the
JSON-blob-for-typed-data the contract rubric forbids. A stored value that lies is
not a display defect.

**The shape is additive** — a widened CHECK, a version bump to 0.8.1 and an ADR
note — so it proceeds without escalation under the 2026-09-07 standing rule, but
**it has a consumer list and is not landed until the list is done**: every reader
of `order.source` in both stores, the Go struct, the Zod schema, the OpenAPI
shape, and the edge writer that currently hardcodes `DIRECT`
(`commands::aggregator::ACCEPTED_ORDER_SOURCE`). The 0.5.2 and 0.5.9 lessson
applies unchanged: a column nothing reads is a column that does not exist, and an
enum member nothing writes is worse — it reads as supported.

**One more value travels with it: `TABLE_TAB`.** ADR-025 (PROPOSED, 2026-09-10)
records table-side customer ordering, whose orders need a source of their own for
the same reason — a tab-entered order recorded as `POS` is wrong in every report
that groups by channel. **One CHECK widening, two values, one ADR note, one
consumer list**, because doing it twice costs more than doing it once. Everything
else in ADR-025 is M8 work and none of it is M6 scope.

**It does NOT block the sitting**, and that was decided explicitly: **C1 does not
test `order.source`**. The criterion is that an already-received aggregator order
bills, prints and closes with the cloud unreachable, and it does that identically
whatever the source column says. So the sitting runs on the current binaries with
`DIRECT`, and the widening lands before the milestone closes — in that order,
deliberately, rather than holding an observation for a schema change.

---

### Not M6: table-side ordering (ADR-025)

**Recorded here so the architecture section is not mistaken for work in flight.**
`docs/architecture/SYSTEM_ARCHITECTURE.md` gained a "Table-side ordering
(customer tab)" section on 2026-09-10 and `docs/adr/ADR-025-table-ordering-device.md`
was written PROPOSED. **No code was written, and no M6 criterion touches any of
it.** Proposed landing is **M8**, trigger *after the first pilot runs on
`STAFF_ONLY`*; the eight follow-ups are in `docs/backlog.md`. The single
exception is the `order.source` widening above, which is pulled forward only
because a CHECK widening was already pending for 0.8.1.

## Status summary

| # | Criterion | State |
|---|---|---|
| C1 | Aggregator order bills and closes with the cloud unreachable | **MET — observed 2026-09-10**, both halves, after the accept path it needs was found MISSING and built the same day. Print evidenced by the file sink, not paper (that gate stays parked) |
| C2 | Stock-out snoozes on ONDC staging | **PARKED** behind platform sandbox access |
| C3 | A permanently-rejected row blocks itself and not its neighbours | **MET — both halves observed 2026-09-10/11** on the shipping binaries. Falsifier watched on a RECONSTRUCTED pre-fix binary (the A2 line removed by hand, C8 precedent): neighbour stranded. Restored binary (`df5e030`, `git diff` empty): neighbour landed while the refusing row stayed blocked. Neighbour counts recorded both times; reconstruction limit and precision caveat stated in the section below |
| C4 | An offline order reaches the cloud without the operator closing the app | **MET — observed 2026-09-10 (evening)** on the shipping binaries by the strict run: order created with the cloud stopped by pid, cloud restarted, row landed inside one pump interval with the POS pid unchanged and the window never touched. An earlier run the same evening was REJECTED because a startup drain could equally explain it |
| C5 | Supplier and pack size created in admin convert on the next receipt | **MET — observed 2026-09-10.** Falsifier watched first (`NO_SUPPLIER_ITEM` on `GRN/20260910/0001`), then absent on `GRN/20260910/0003` after the supplier item was created in the console |
| C6 | A goods receipt is readable back in-product | **MET — observed 2026-09-10.** Screen half at step 11, edge row at step 23; every field agrees including the nulls, and the row carries `NO_PURCHASE_ORDER` only |
| C7 | A client-data failure is reported as 4xx with a reason the edge records | **CLOSED — observed 2026-09-07** on the shipping binaries, both halves of the falsifier watched |
| C8 | An aggregator order flows through both adapters | **MET 2026-09-10 — `SHAPE ONLY — no integration evidence`.** Both adapters applied an order over HTTP on the shipping backend; the boundary check was watched RED on a planted `if in.Platform == "ondc"` and green after removal. Integration travels to M6.1 C1, unmet |

---

## M6 C7 — a client-data failure is reported as 4xx with a reason the edge records

**State: CLOSED. Observed end to end on the shipping binaries, 2026-09-07, by
the operator.** Both halves of the falsifier were watched: the pre-fix 500 on
2026-09-03, and the post-fix 422-stored-and-surfaced on 2026-09-07. Nothing
below is evidenced by a test harness.

### The closing observation, 2026-09-07

**Preconditions, each verified rather than assumed.** Backend restarted after
the previous instance was killed: `api.exe` **PID 60872**, created 15:48:56,
`/health` 200 — a NEW pid, not the port answering (the old PID 8800 was killed
and port 8080 confirmed free first). Cloud seeded with its two-item menu; the
edge seeded with 39 items across 8 categories, so the drift the criterion needs
was intact. The till's sync credential was enrolled with the backend already
listening — `[3b/4] rotating ... sync ENABLED`, device
`01a05d10-cba7-7876-9958-65f9a6ca2fe7`. **That step had silently failed on the
two previous attempts** and is the reason they produced nothing; see the
`dev-up.ps1` ordering defect in `docs/backlog.md`. Pump interval set to 10s via
`HOLLER_SYNC_PUMP_INTERVAL_SECS`.

**What the operator saw.** A purple banner at the top of the till, listing seven
rows, each reading `order <aggregate_id> · 5 attempts · missing_reference (HTTP
422)`, headed "7 records will not reach the cloud" and closing "Nothing is lost
locally — these need someone to look at them."

**What the database held at that moment** (read from `sync_outbox_block` while
the application was open, on a copy):

| aggregate_id | attempts | last_status | last_code | blocked_at (UTC) |
|---|---|---|---|---|
| `01a04210-8e03-7540-a480-d0fde09d14b3` | 5 | 422 | `missing_reference` | 11:21:45.021 |
| `01a04219-1241-71c2-b689-1ea22414f8d1` | 5 | 422 | `missing_reference` | 11:21:45.109 |
| `01a04266-710b-7730-bc2f-910a7dc68931` | 5 | 422 | `missing_reference` | 11:21:45.208 |
| `01a042dc-fab8-7f92-9e29-f04cb5346292` | 5 | 422 | `missing_reference` | 11:21:45.316 |
| `01a06ee5-67f8-76d2-a4b7-caf41e1fc1d1` | 5 | 422 | `missing_reference` | 11:21:56.288 |
| `01a04210-…` (2nd outbox row) | 5 | 422 | `missing_reference` | 11:22:16.787 |
| `01a04210-…` (3rd outbox row) | 5 | 422 | `missing_reference` | 11:23:07.431 |

Four of those five orders are the ones rejected on **2026-09-05** and they
survived a machine restart and a full reseed to be surfaced here.

**The "records" half, tested harder than planned.** Closing the POS window did
NOT terminate the process (see the finding below), so the relaunch exercised
`crypto::recover_crash_leftovers` rather than a clean reopen: SQLite replayed
the WAL, the merged state was resealed superseding a sealed file 79 minutes
stale, and the plaintext was wiped. `edge.db.enc` went 1335324 bytes @16:13 →
1343516 @17:32 and `edge.db-wal` 626272 → 0. **After the relaunch the banner
showed the same seven rows, same ids, same attempt counts, same code.** The
reason is durable across an unclean exit and a WAL replay, which is a stronger
observation than the clean restart originally specified.

### Findings from the observation — none blocking C7, all real

1. **The banner prints `aggregate_id`, so one order appears three times.** The
   three `01a04210` entries are three distinct outbox rows of one four-item
   order (`01a04210-a428-…`, `01a04210-a6f6-…`, `01a04211-d7f2-…`, all
   `ItemAdded`). Once a row exhausts its budget and is abandoned, the drain
   moves to the next row of the same aggregate, which spends its own five
   attempts and blocks ~30s later. Two consequences: the count climbs toward
   roughly twenty entries for what is five orders, and an operator reads that
   as twenty lost orders; and fully surfacing one order costs five attempts
   PER EVENT, not five in total.
2. **The banner covers the top bar.** `position: fixed; top: 0` with
   content-dependent height — at seven rows it hides the search box and the
   DINE_IN / TAKEAWAY / DELIVERY / Select table / Orders / Stock row entirely.
   The CSS comment claims this region is "a region nothing else occupies"; it
   is the top bar's region. This is the third fixed overlay colliding, which
   is what that comment set out to avoid.
3. **Closing the window does not exit the POS under `tauri dev`.**
   `holler-pos.exe` PID 78528 remained alive after the window was closed, with
   the pump still ticking (WAL written two minutes later) and the database
   still open and unsealed. `RunEvent::Exit` never fired, so the shutdown drain
   never ran. Sits beside A6.

### C3 evidence collected in passing, NOT sufficient to close it

During the run, order rows published 74 → 84 while five aggregates were
blocked; 24 order rows remained pending across 13 distinct aggregates. So
neighbours drained while blocked rows did not, which is C3's observation — but
C3's falsifier requires the same fixture on the **pre-fix** binary with
neighbour counts recorded both times, and that has only been done in tests.
**C3 stays open.**

### The falsifying condition, watched first

> Replay an FK-violating row on the **pre-fix** binary → 500, budget uncharged;
> after → 4xx, reason stored, row surfaced.

**The pre-fix half is Executed and recorded.** On 2026-09-03, against Docker
Postgres, a replayed `order_item` referencing a `menu_item` the cloud does not
hold produced, from the real router and the real repository:

```
2026/09/03 10:05:30 ERROR httpx: unhandled error error="ordering: appending item:
ERROR: insert or update on table \"order_item\" violates foreign key constraint
\"order_item_menu_item_id_fkey\" (SQLSTATE 23503)"
    status = 500 (internal_error), want a 4xx
    code = "internal_error", want "missing_reference"
```

### What is Executed

| Half of the criterion | Evidence | Commit |
|---|---|---|
| **4xx on the wire** | `TestIngest_AppendItem_MissingMenuItemIsClientErrorNotServerError` — red at 500 `internal_error`, green at 422 `missing_reference`. Body carries no SQL, no SQLSTATE, no constraint name | `99875cc` |
| **The same fault on two more ingest routes** | KOT for an unknown order was **404** (worse than 500: the edge treats 404 as transient, so it retried forever and took a global stop with it) → 422. Payment for an unknown order 500 → 422, with a companion test proving a good tender still succeeds | `856616b`, `3f7abaa` |
| **Budget charged** | `a_permanently_refused_row_spends_its_budget_then_becomes_visible` — five attempts, then `blocked_at` set. Falsified by disabling the give-up branch: `the budget is spent: left: 0, right: 1` | `078d4e5` |
| **Transient failures never charged** | `a_transient_failure_counts_forever_and_blocks_never` — counted and surfaced, never blocked. Falsified by spending the budget on transient failures | `078d4e5` |
| **Reason stored** | `sync_outbox_block.last_code` carries the machine-readable code (`missing_reference`), durable across a restart. Asserted field-by-field, not merely non-empty | `c95dc24`, `078d4e5` |
| **Row surfaced** | `list_blocked_outbox_rows` / `list_persistently_failing_outbox_rows`, their TS clients, and `SyncBlockedBanner` on `PosScreen` and `OrderListScreen` | `c8147ef` |
| **Neighbours not stranded (C3)** | `a_refused_row_blocks_its_own_aggregate_and_not_its_neighbours` — falsified twice: report fields with no skip logic, then the classifier call replaced with `if false`, both giving `left: [] right: ["outbox-2"]` | `07d7968` |

Suites executed through `scripts/assert-tests-ran.mjs`, so a run that executed
nothing is a failure rather than a pass: backend `go test ./...` — 15 packages
with tests; `cargo test -p holler-edge-sync` — 62 tests; `pnpm test` (POS) — 230
tests. All green.

### What is NOT observed, and why that matters here

**The banner has never been rendered.** It is typechecked (`tsc --noEmit`), built
(`pnpm build`) and its data path is unit-tested — and this repository has twice
recorded that **build-green is not dev-works for a Tauri frontend**: the KDS
detached-global crash was browser-only, and the POS white screen was
dev-server-only. Neither was visible to any green suite. A banner that renders
blank, or renders behind another fixed overlay, would satisfy every check above
and fail the criterion.

**The end-to-end path has never run in one process.** Each half is proven against
its own harness: the cloud returns 422 in a Go test, the edge records and blocks
in a Rust test against a `tiny_http` stand-in. **No single run has taken a real
order from the till, failed it against the real backend, and shown the operator
the result** — and `docs/backlog.md` still carries "`edge/sync` has no host", so
the worker is only reachable from a test process at all.

### The 2026-09-05 attempt — the post-fix half PARTLY observed, and why it fell short

An operator run was made on 2026-09-05 and **did not close the criterion**. It is
recorded here rather than discarded, because what it produced is evidence and
because the reason it fell short changed the criterion itself.

**What the run produced, read back from `sync_outbox_block` on 2026-09-07** (the
edge database, queried directly; four rows, all `aggregate_type = order`):

| aggregate_id | attempts | last_status | last_code | first → last attempt (UTC) |
|---|---|---|---|---|
| `01a04210-8e03-7540-a480-d0fde09d14b3` | 2 | 422 | `missing_reference` | 00:05:00.949 → 00:05:02.886 |
| `01a04219-1241-71c2-b689-1ea22414f8d1` | 2 | 422 | `missing_reference` | 00:05:01.283 → 00:05:02.971 |
| `01a04266-710b-7730-bc2f-910a7dc68931` | 2 | 422 | `missing_reference` | 00:05:01.732 → 00:05:03.070 |
| `01a042dd`* → `01a042dc-fab8-7f92-9e29-f04cb5346292` | 2 | 422 | `missing_reference` | 00:05:02.217 → 00:05:03.169 |

`last_error` on all four: `cloud rejected the envelope with status 422`. Times are
UTC; the operator observed them as 05:35 IST. The items ordered were Malai Tikka,
Mixed Veg Curry, Palak Paneer, Paneer Butter Masala, Egg Bhurji, Fish Curry and
Chana Masala — **none of them among the two the cloud seeds**, which is what the
criterion's precondition requires.

**So the wire half and the storage half ARE observed on the shipping binaries:**
a client-data failure was reported as 422 with `missing_reference`, the reason was
stored durably, and it survived both the process dying and a machine restart.

**The surfacing half was not, and the banner was correctly absent.** Every row
stands at `attempts = 2` with `blocked_at` NULL, and the two queries behind
`SyncBlockedBanner` select on exactly those columns:
`list_blocked_outbox_rows` needs `blocked_at IS NOT NULL`;
`list_persistently_failing_outbox_rows` needs `attempts >= OUTBOX_ATTENTION_ATTEMPTS`.
2 < 3 and 2 < 5, so both returned empty and the component returned `null`. The
run stopped **one attempt short of the amber condition and three short of the
purple one**. Nothing here is a defect in the banner.

**Why it stopped at two attempts, and the correction it forces.** Before M6 A5,
`drain_outbox` had exactly two callers — startup and `RunEvent::Exit`. Attempts
could therefore only accrue when a human started or stopped the application, so
"watch five pumps" with the window open could not move the counter at all. **The
observation is only reachable with the periodic pump present**, which supersedes
the earlier startup/shutdown-only note in this section's closing steps.

### The falsifier, corrected

The criterion's falsifier as originally written — *"after → 4xx, reason stored,
row surfaced"* — reads as one event and is three, separated by a threshold a
single order and a few pumps cannot cross. Stated exactly, with the constants it
depends on (`edge/sync/src/worker.rs`):

- **4xx on the wire** and **reason stored** happen on the FIRST rejection.
- **Row surfaced, still retrying (amber)** requires `attempts >= OUTBOX_ATTENTION_ATTEMPTS`, which is **3**.
- **Row surfaced, given up on (purple)** requires `attempts >= MAX_OUTBOX_REPLAY_ATTEMPTS`, which is **5**, at which point `blocked_at` is set.

**A single order with a few pumps cannot satisfy this criterion**, and any future
run that reports it satisfied without naming an attempt count of at least 3 has
not observed the surfacing half. This correction was made on 2026-09-07 after the
2026-09-05 run failed for precisely this reason.

### What closing it requires

0. **A5 (the periodic pump) present in the running binary.** Without it the
   attempt counter only moves when the application starts or stops, and the
   thresholds above are reachable only by a restart loop no operator would ever
   perform — a condition the environment cannot naturally produce, which M5's
   retro already names as no test at all.
1. Backend up in its own window via `scripts/dev-up.ps1`, **verified by a NEW
   pid** — not by the port answering (`docs/retro.md`; an old process answers
   identically).
2. A till order whose `menu_item_id` the cloud does not hold — the seeded 2-row
   cloud menu against the edge's 43 makes this the default, not a contrivance.
   **The menu seed drift must stay untouched until then**: seeding the cloud
   makes the 500 disappear and ships both defects looking like a fix.
3. Watch the drain: the order is refused 422, its neighbours still publish, and
   the row lands in `sync_outbox_block`. **Then keep watching**: the budget
   spends one attempt per pump, and the row is not visible to anyone until the
   third.
4. **Read the banner off the screen** and record what it says, with the
   `aggregate_id`, the code it displays, and **the attempt count at the moment it
   appeared**.
5. Restart the POS and confirm the banner still says it — that is the half
   "a reason the edge **records**" actually asserts.

\* The fourth row's `outbox_id` is `01a042dd-125b-7423-87e4-ac38b9edd069`; its
`aggregate_id` is the value in the table. The two differ by one character at the
prefix and are easy to transpose — noted so a later reader does not read it as an
inconsistency.

---

## M6 C3 — a permanently-rejected row blocks itself and not its neighbours

**State: MET — both halves observed 2026-09-10/11 on the shipping binaries.**
The mechanism, its falsifications and its commits are in the C7 table above.

The falsifier this criterion names — *the same fixture on the pre-fix binary
strands the neighbours, neighbour counts recorded both times* — was Executed
only as a harness result (`left: [] right: ["outbox-2"]`) until 2026-09-11.
Nothing in the working tree builds a pre-fix binary, so one was RECONSTRUCTED by
removing the A2 line by hand. The full run, its reconstruction limit and its
precision caveat are in the final section of this file.

---

## SITTING RUN OF 2026-09-10 (EVENING) — C4 MET BY THE STRICT RUN, C3's POSITIVE HALF OBSERVED

Driven by Claude, with Gaurav operating the till. This run exists because an
earlier attempt the same evening was rejected as insufficient, and that rejection
is the most important thing in this section.

### The run that was NOT accepted, and why

At 18:50:54 the POS was started with the C4 order from the previous session
(`01a08b39-d92b-7dd1-8df9-77bacce251bc`) still pending, and the row appeared in
Postgres at 18:51:11 — seventeen seconds later, with the window open and the
operator closing nothing. That satisfies the words of C4's falsifier and was
still **not** recorded as met.

The reason: the startup drain has existed since long before A5, so a row landing
seventeen seconds after process start does not distinguish the periodic pump from
the drain that runs at open. The pre-fix binary would have landed it too. C4's
falsifier as written (`taskkill` so `RunEvent::Exit` never fires) isolates the
*shutdown* drain and says nothing about the startup one. **A criterion that a
pre-fix binary also passes is not evidence for the fix.** The observation was
discarded and the strict run below was constructed to exclude the startup drain
structurally, by leaving the POS process running across the whole sequence.

### C4 — MET 2026-09-10, by the strict run

Executed:

| Time (local) | Action |
|---|---|
| 19:0x | Backend stopped **by pid 3104**; port 8080 confirmed free; process confirmed gone from the process table |
| 19:0x | `scripts/check-cloud-unreachable.ps1` agreed on all three probes: nothing listening, TCP refused, HTTP no answer |
| 19:03 (13:33:20.733Z) | Operator rang one order on the till with the cloud down — `01a08b85-dbdd-7670-ae18-42b674510ad5`, DINE_IN, 1 item, ₹300.00 |
| 19:08:10 | Backend restarted; bound 8080 at **19:08:12** on **new pid 64312** |

Read-verified against Postgres, not inferred from any screen or log:

| Check | Before restart | After restart |
|---|---|---|
| `"order"` count | 250 | 251 |
| Target row | **ABSENT** | present by **19:08:29**, as `DRAFT` / `30000` paise |
| POS process | pid 28796, started 18:50:54 | pid 28796, started 18:50:54 — **unchanged** |

The cloud came back at 19:08:12 and the row was in Postgres by 19:08:29, inside
one or two ticks of the 10-second pump interval this run used
(`HOLLER_SYNC_PUMP_INTERVAL_SECS=10`; the production default is 60). No process
started between those two timestamps, so no startup drain could have run: the
POS had been up for eighteen minutes and stayed up afterwards, same pid, window
never touched. **An order placed offline reached the cloud with the application
still running and the operator doing nothing.** C4 is met.

Two honesty notes that travel with this record:

- C4's falsifier was written assuming a normal exit fires `RunEvent::Exit`. An
  earlier session established that **neither `Ctrl+C` nor a window close fires
  it on this build**, so the abnormal-exit distinction the falsifier reaches for
  does not exist here. The criterion still stands on its own terms — the pump
  landing a row with no exit event involved is the point — but the record must
  not imply the falsifier isolated an abnormal path.
- The 10-second interval is not the shipped cadence. It was set so a tick could
  be watched inside one sitting; at the 60-second default the same observation
  takes up to a minute longer and is otherwise identical.

The order's total in the cloud is `30000` paise, matching ₹300.00 on the till
exactly, so the create event carries the right money. That matters for the
separate finding below: the divergence is not at create.

### C3 — the positive half, OBSERVED 2026-09-10 (evening)

From the same run, off the till's own sync banner rather than a harness. The
banner read **"8 records will not reach the cloud"**, with the details pane
naming, among them:

- `order 01a04210-8e03-7540-a480-d0fde09d14b3` · 5 attempts · `missing_reference (HTTP 422)`
- `order 01a04219-1241-71c2-b689-1ea22414f8d1` · 5 attempts · `missing_reference (HTTP 422)`

While all eight stayed blocked at their attempt ceiling, the till accepted a new
order and published it: `01a08b85-dbdd-7670-ae18-42b674510ad5` landed in
Postgres at 19:08:29, taking the count from 250 to 251. **A permanently-rejected
row blocked itself and did not block its neighbours**, observed on the shipping
binaries with the operator watching the same banner.

An observation made while reading this, and NOT part of C3's claim: both blocked
ids are present in Postgres as orders. The blocked outbox rows are therefore
later events on those orders, not their create events — which is a lead on the
divergence finding below, not evidence for C3.

**C3 is still NOT closed.** Its falsifier requires the same fixture on the
pre-fix binary with neighbour counts recorded both times, and that comparison
exists only as a harness result. The positive half being observed does not
substitute for it, and this section must not be read as closing the criterion.

### Three findings carried out of this run

**The cloud's copy of an order stops tracking the till after create —
reproduced twice, cause unknown, A7 ruled out for the second instance.** Order
`01a08b38-91a6-78d0-b805-123e9ff75a10` reads two items and ₹385.00 on the till;
in Postgres it is `DRAFT` at `5500` paise with a single `order_item` line whose
`line_total_paise` is `22000` — a total that agrees with neither the till nor its
own line. The earlier instance was attributed to A7 (invoice and payment have no
edge route), but `order` **does** have a route, so A7 does not explain this one.
The one-query read taken here: the cloud has 990 `menu_item` rows and the line's
`menu_item_id` (`0191a000-0000-7000-8000-000000000012`, Veg Thali) exists, so a
missing menu item is not the reason. Combined with the C3 note above — both
blocked rows are later events on orders whose creates landed — the shape is
consistent with item and status events failing `missing_reference` while creates
succeed, but that is a hypothesis and nothing here tested it. **Trigger: before
the first pilot.**

**Do not use this order's cloud copy as C6 evidence or any other comparison
against the till.** C4 is the row landing, not its contents, and C4 is unaffected.

**The 10-second pump makes the till sluggish while the cloud is down.** Observed
again this run, and this time with the cloud confirmed down by all three probes:
the operator reported the application slow while ringing the C4 order. Every tick
takes the database lock and walks the pending backlog against a dead uplink, and
the UI waits behind it. ADR-013's promise is that an outlet is unaffected by a
dead uplink. The 10-second interval used here is more aggressive than the shipped
60-second default and makes it worse, so the shipped severity is unmeasured.
**Trigger: before the first pilot.**

**The Orders screen renders raw UTC.** The Created column shows
`2026-09-10T13:33:20.733Z` for an order rung at 19:03 local, which reads as a
clock fault to an operator. CLAUDE.md's rule is UTC storage with local rendering.
Cosmetic, no data implication, filed with the other two.

---

*Last updated 2026-09-10, during M6 Phase C.*

---

## M6 C3 — MET 2026-09-10/11, BOTH HALVES OBSERVED ON THE SHIPPING BINARIES

Driven by Claude, with Gaurav operating the till. This closes the criterion that
had stood at "code complete, awaiting observation" since Phase A, and whose
positive half was observed earlier the same evening without its falsifier.

### The pre-fix binary, and why it is a reconstruction rather than a checkout

C3's falsifier requires the same fixture on the **pre-fix** binary. Nothing in
the working tree builds one, so the A2 fix was removed by hand: one line in
`edge/sync/src/worker.rs`, `if blocked.contains(&aggregate_key)` replaced by
`if !blocked.is_empty()`, reconstructing pre-A2 GLOBAL head-of-line blocking —
once any aggregate is refused in a tick, every row behind it is held regardless
of which aggregate it belongs to. The C8 precedent (a planted
`if in.Platform == "ondc"` watched RED, then removed) was followed deliberately
over a worktree checkout, which this repository has a data-loss incident against.

**The reconstruction is PARTIAL, and the limit is stated here rather than
discovered later.** `blocked` is a `HashSet` local to `pump_outbox`
(`worker.rs:341`), seeded only by refusals in that same call, and
`repo::outbox_row_is_blocked` short-circuits at `worker.rs:349` BEFORE the
plant is reached. So the plant strands neighbours only while the offending row
is still being attempted — about 50 seconds at a 10-second tick, five attempts.
Real pre-A2 had no per-row budget and would have wedged the neighbour
permanently. The reconstruction therefore reproduces the SHAPE of the defect for
a bounded window, not its duration.

### The fixture, and the correction to what makes it work

**The refusal path is the VARIANT foreign key, not the menu one.** This was
assumed backwards when the run was planned — "the cloud menu seed is a token, 2
rows against the edge's 43" — and the assumption was falsified mid-run by an
order whose menu item the cloud DOES hold being refused anyway. The cloud holds
exactly one `menu_item_variant` row for the seeded outlet
(`backend/cmd/devseed/main.go:267`, Large on Masala Chai) against a variant per
item on the edge, and `order_item` carries three foreign keys. Reproduced in a
rolled-back transaction:

```
ERROR:  insert or update on table "order_item" violates foreign key constraint "order_item_variant_id_fkey"
```

So the fixture is: **Palak Paneer refuses** (neither its menu item nor its
variant exists cloud-side), **plain Masala Chai succeeds** (both exist, proven
empirically — two Chai lines carrying variant `...0013` landed during this run).
The full finding is in `docs/backlog.md`; it is also the cause of the DRAFT-copy
divergence recorded earlier the same evening.

One behaviour to know when reading the ids below: **ringing an item while a
draft is open on that table APPENDS to the open draft** rather than creating a
new order. Both Palak lines therefore landed on existing orders, which is why
the banner grows a second row against an id already in it.

### Pre-fix half — THE NEIGHBOUR IS STRANDED

Binary: planted `worker.rs`, POS pid 25836. Cloud up, pid 64312.

| | Value |
|---|---|
| X (refusing row) | Palak Paneer line appended to `01a08bc4-ef94-77f1-9349-a983e447eb4b` |
| X observed | banner, 5 attempts, `missing_reference (HTTP 422)` |
| Y (neighbour) | `01a08e33-a7b5-7fd2-a81b-bd9534a4b846`, plain Masala Chai, created 02:02:25.077Z |
| Banner | **11 → 12** |
| Postgres `"order"` count | 253 at the read after Y was rung — **Y ABSENT** |
| Y in banner | **no** |
| Later | Y landed, count 254, only after X exhausted its five attempts |

**Y was never attempted, and that is provable without the encrypted outbox.**
Plain Masala Chai with variant `...0013` is the one line shape that succeeds
here, and it succeeds on its first attempt. A row that succeeds on first attempt
cannot remain unsent across roughly twelve 10-second ticks unless it was never
sent. It drained only once X moved into `sync_outbox_block`, after which the
short-circuit at `worker.rs:349` runs before the plant and the plant fires on
nothing — the bounded window described above, observed from both sides.

### Post-fix half — THE NEIGHBOUR IS NOT STRANDED

Binary: `edge/sync/src/worker.rs` restored, `git diff` EMPTY, zero plant markers,
built from commit **`df5e030`**. POS pid 26140, started 07:36:37. Cloud up, pid
64312.

| | Value |
|---|---|
| X' (refusing row) | Palak Paneer line appended to `01a08e33-a7b5-7fd2-a81b-bd9534a4b846` |
| X' observed | banner, 5 attempts, `missing_reference (HTTP 422)` |
| Y' (neighbour) | `01a08e3b-eecf-7d61-ad23-1dce200b32a4`, plain Masala Chai, created 02:11:27.567Z |
| Banner | **12 → 13** |
| Postgres `"order"` count | **254 → 255, Y' PRESENT** while X' is blocked |
| Y' in banner | **no** |

### Neighbour counts, both times — the comparison the criterion asks for

| Binary | Refusing row surfaced | Neighbour reached the cloud while it was blocked |
|---|---|---|
| Pre-fix (planted) | yes, 5 attempts | **NO** |
| Post-fix (`df5e030`) | yes, 5 attempts | **YES** |

**A permanently-rejected row blocks itself and not its neighbours.** C3 is MET.

### PRECISION CAVEAT — stated because the evidence is coarser than it reads

Both halves are **bounded by reads, not measured per tick.** Y's stranding is
established by one read showing it absent and a later read showing it present,
about two minutes apart; Y' was first polled roughly 88 seconds after it was
rung, so its landing is bounded at <=88s rather than pinned to a tick. **The
verdict does not depend on this**: the pre-fix claim is "not sent across many
ticks", which coarse reads establish, and the post-fix claim is "present while
the refusing row is blocked", which does not need a timestamp at all.

### Supplementary, NOT the criterion's evidence

To remove the looseness on the post-fix side at no cost, a 2-second poll was
started BEFORE a single plain Masala Chai was rung on the same binary: order
`01a08e41-74c0-7991-abc5-3f4e8e557608`, created 07:47:29.536 local
(02:17:29.536Z), observed in Postgres at **07:47:36 — about 6.5 seconds, inside
one 10-second pump interval**, with 13 rows blocked in the banner at that moment.

This is recorded as supplementary because it has no refusing row of its own: it
measures how fast a good row lands on the fixed binary, not neighbour behaviour.
A deliberate choice was made NOT to re-plant for a matching tick-level
measurement of the pre-fix side — it would have cost another build cycle and
added another permanently-blocked row to the live edge database to confirm what
the absence already shows.

### What this run cost the live database, recorded so it is not mistaken for a defect

Two further permanently-blocked outbox rows (the two Palak lines), taking the
banner from 11 to 13. Both are `missing_reference` on the variant foreign key
and are the fixture, not a fault. Nothing was seeded to make them go away, per
the standing rule that the drift is the stimulus.

---

*Last updated 2026-09-11, during M6 Phase C.*
