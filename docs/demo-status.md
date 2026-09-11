# Demo build — status, evidence and what is gated

**Written 2026-09-11.** The brief is `docs/demo-kickoff.md`. This file is the
record of what was built, **how it was verified**, and what still depends on a
human. The chat is not the record — a verdict that exists only in a session
transcript is erased by a restart, and what replaces it is a reconstruction from
git history stated with the confidence of a read.

Read `CLAUDE.md`'s `## Current milestone:` block for scope and EXCLUDES.

---

## Work items

| # | Item | State | Evidence |
|---|---|---|---|
| **0** | `apps/captain` | **BUILT, NOT PASSED** — the cut-off condition needs a human | below |
| **0b** | UPI QR, screen + receipt | **DONE** | below |
| **1** | Seed parity | **BUILT; row-for-row comparison UNRESOLVED** | below |
| **2** | Demo seed content + reset command | **BUILT; never run with `-Force`** | below |
| **3** | Rendered receipt | **DONE** | below |
| **4** | Sync banner legibility | **DONE, observed in Chromium** | below |
| **5** | Offline tick | **NOT STARTED** — conditional on observing sluggishness with the cloud down |  |
| **6** | Presentability | **DONE for POS and admin; KDS unreachable** | below |
| **7** | `docs/demo-script.md` | **NOT STARTED** — Tuesday |  |
| **8** | Three timed rehearsals | **NOT STARTED** — Tuesday, needs item 2's reset to run first |  |

---

## What is gated on a person, not on code

Everything here is blocked on something no agent in this project can do.

1. **`apps/pos/.env.dev` carries the edge encryption key and is deny-ruled to
   agents.** So **the operator runs `scripts/demo-reset.ps1 -Force`**, and that
   first run is also the first end-to-end execution of the whole sequence — see
   item 2 below for exactly which steps have never executed.
2. **A WAITER device must be enrolled against the cloud** and its token pasted
   into a phone. Item 0's cut-off cannot be attempted before this.
3. **The POS Tauri window cannot be launched from a tool with redirected
   stdio** — it never appears. Nothing that requires the POS shell has been
   observed running by anyone.
4. **The POS icon is a 16×16 placeholder** from the original scaffold, 1086
   bytes. Needs real artwork; no builder can produce it.

## Decisions outstanding with the operator

- **Demo steps 4 and 5 name admin screens that do not exist.** `apps/admin` has
  three tabs — Menu and pricing, Suppliers, Goods receipts (`App.tsx`). There is
  **no orders screen and no stock variance screen**, and no cloud read route to
  build them on: `/orders` is POST-only ingest, and `/inventory/ledger-entries`,
  `/inventory/items`, `/inventory/counts` and `/inventory/recipes` are all
  POST-only. Only `/orders/{id}` has a GET and there is no list route. An admin
  orders list therefore needs **new OpenAPI paths — a contract change**, against
  a brief that needs none. Options put to the operator: **A** cut both from the
  story; **B** orders only (new `GET /orders` + Go handler + admin tab, ~1.5–2
  builder-days, contracts 0.9.0); **C** orders and stock (~3–4 days).
- **`inventory_item_name` is missing from `GoodsReceiptLineReadSchema` and
  `SupplierItemSchema`.** Both carry `inventory_item_id` only, unlike
  `stock_ledger_entry`/`stock_count_line`, which already denormalise the name
  for exactly this reason. Consequence: **the admin GRN screen cannot name a
  single ingredient on a receipt**, and demo step 5 shows that receipt. The
  presentability pass withheld the UUID rather than fabricate a name, which was
  correct. Folds naturally into option B if that is taken.

---

## Item 1 and 2 — seed parity

**Design: one emitter, one committed generated artefact, two readers.**
`edge/database/src/bin/devseed.rs --emit-json` produces `seed/demo-outlet.json`;
both seeders consume that file. Hand-transcribing the catalogue into JSON was
considered and rejected — the transcription step is exactly where two
descriptions drift, which is the defect being fixed, one layer out. Full
reasoning in `seed/README.md`.

**What it replaced:** two seeders describing one outlet by hand-mirrored
constants, each with a comment saying it MUST match the other, diverged by a
factor of twenty (edge ~39 menu items, cloud two). That divergence is the direct
cause of the thirteen permanently-blocked rows in the live edge outbox, each
`missing_reference (HTTP 422)` on `order_item_variant_id_fkey`.

**Content:** 43 menu items, 10 categories, 24 recipes, 93 `recipe_ingredient`
rows, 32 inventory items with opening stock, one supplier with pack sizes, and
`GRN/20260809/0001` with 7 lines.

### A defect the gate caught, and why three green signals missed it

The emitter produced `tax_rule.id` as `format!("{id}-{component}")` —
`0191e600-...-000001-CGST`, **not a UUID**. `tax_rule.id` is `UUID PRIMARY KEY`
in Postgres, which rejected it on the first insert with SQLSTATE `22P02`.

It survived three green signals:

- **SQLite has no UUID type**, so every edge test stored it happily.
- **The emitter is byte-stable**, so `check-seed-drift.mjs` regenerated the same
  broken bytes and stayed green.
- **The cloud's test fixture hand-authored a valid UUID in that exact field**, so
  the Go tests passed against a fixture that diverged from the real emitter on
  the one field that mattered.

It was only visible from the store that enforces the type. Fixed by
`tax_rule_id(seq)` in its own disjoint range (`6467bc8`), then **proved against
Postgres**: the Go seeder run against the real committed file, exit code captured
unpiped, `EXIT=0`, six real UUID rows confirmed by direct query.

The same sweep found the identical pattern in `seed_billing()`, left alone with a
reason: it is gated behind `HOLLER_SEED_BILLING`, edge-only, and never reaches
the shared JSON or Postgres.

### Verified

- Edge: 324 tests, `cargo test` native Windows; clippy and fmt clean.
- Cloud: 6 tests, `go test` native, against live Postgres.
- Re-emission byte-stability: emitted twice, `diff` identical.
- `check-seed-drift.mjs` falsified by hand-editing `outlet.name` and watching it
  report STALE.

### UNRESOLVED

**Row-for-row comparison of the two stores has never happened.** The live
Postgres carries 991 `menu_item`, 123 `tax_profile` and 345 `recipe` rows from
unrelated prior work, so there is no known state to compare against. Settled by:
run `scripts/demo-reset.ps1 -Force`, then diff `menu_item`,
`menu_item_variant`, `recipe`, `recipe_ingredient`, `inventory_item` and
`supplier_item` between Postgres and the edge SQLite by id. **A matching count
with mismatched contents is exactly what the old hand-mirrored constants
achieved**, so compare contents, not counts.

### The goods receipt is seeded into both stores directly

`edge/sync/src/route.rs` maps only `order` and `table_session` (carried gap A7),
so a GRN seeded at the edge can never reach the cloud, and demo step 5 shows it
in the admin console. It is therefore written to both stores from the same shared
description. **That is an accommodation for A7, not a fix for it**, and the
cloud's copy is still a replica that every surface must label as one (contracts
0.7.0).

### `.gitattributes`

`check-seed-drift.mjs` compares raw strings and the repository has
`core.autocrlf=true` with no `.gitattributes` at all, so a fresh clone would have
written the file CRLF against an LF emitter and reported STALE on every clean
checkout. Pinned at `1551bee`. A guard that cries wolf gets switched off, and
switching this one off restores the condition it exists to prevent.

---

## Item 2 — the reset command

`scripts/demo-reset.ps1` plus `scripts/demo-assert/`, a dev-only Rust binary that
opens the sealed edge database, asserts four invariants by name with actual row
counts, and reseals on the way out either way.

**The assertion tool was falsified**: a blocked row was planted and `run()`
watched reporting FAIL rather than passing silently.

**Two assertions in the original spec named things that cannot be queried**, both
found while implementing them and corrected in `seed/README.md` at `02a51fd`:

- `local_outbox` **carries no blocked flag**; the rows live in
  `sync_outbox_block` with `blocked_at` set (contracts 0.6.4). Written literally,
  that assertion would have passed by querying a column that does not exist.
- **"The POS sync banner is empty" cannot be answered by a database.** The check
  queries what the banner reads and is required to say in its output that it is a
  **proxy**, not an observation of `SyncBlockedBanner.tsx`.

`stock_deduction_gap` is **not** `grn_gap`. The demo seed legitimately produces
one `NO_PURCHASE_ORDER` row in `grn_gap`, because a GRN never blocks on a PO
(ADR-019); asserting zero there makes a correct seed look broken.

**Never exercised**: the real `-Force` run — deliberately, since it drops the
shared dev Postgres schema and deletes the shared edge database while other work
had live state in both. Specifically unexecuted through the script: the
`DROP SCHEMA ... CASCADE`, the `Remove-Item` on `edge.db.enc`/`edge.db`, both
devseed invocations as driven by the script, and the backend kill.

Guards: `-Force` or `-WhatIf` required; the full destruction banner with resolved
absolute paths prints **even when the run refuses**, so a caller who forgot
`-Force` still sees what refusing avoided. **No backup step of any kind** — the
edge database is never copied anywhere unencrypted (ADR-011), and a convenience
backup is how that rule gets broken.

---

## Item 0 — `apps/captain`

**Approved reduced scope**: pair, tables, menu+cart, send. No bill screen, no
payment, no paid modifiers. `order.source` stays `POS`. **No contract change was
needed** — `WAITER` already exists in `device.kind`.

The JSON API was pinned in `docs/captain-api.md` **before either half was
built**, so the frontend and the listener could be built in parallel from one
document.

**Verified as a seam, not just as two halves.** All five endpoints were compared
field-for-field between the Rust wire structs and the frontend's Zod schemas:
**no mismatches**, including ones that currently happen not to matter. The auth
reimplementation was checked rule by rule against `CachedCredentialVerifier` and
preserves every one, fail-closed on an unparseable expiry included. **No path
exists for a client-supplied `device_id`** — confirmed by reading both sides.

**The hub gap.** The listener's first test suite built its state with
`AppState::new`, which sets `hub: None`, making `notify_kot` a no-op. So 5/5
green proved the KOT row landed in SQLite and **nothing about the hub** — while
the cut-off condition is *reaches the KDS **and the hub***, and the hub is what
the KDS renders from. Closed at `d805218`: the test now subscribes to a real
`Hub` and asserts a `KotUpserted` frame arrives with the matching `kot.id`,
`order_id` and `station`. **Falsified** by pointing the subscription at a decoy
`Hub` never wired into the running state and watching `recv_timeout` fire.

### Known, filed, not fixed

**A captain-originated KOT's `created_by_device_id` is the till, not the
waiter.** `send_order_to_kitchen_impl` stamps it from `state.device_id`.
`order.device_id` **is** correctly attributed to the waiter — these are two
different columns and only the KOT one is wrong. It is invisible today: **the KDS
renders no ticket origin at all, for any ticket.** Also, attribution is captured
only at order-create time, so appended lines record nothing about who added them
in either direction.

### Still standing between here and the cut-off

A WAITER device enrolled; a real phone on the hotspot; three runs from a clean
reset with the POS process staying up across all three. **No browser has ever
talked to the real listener** — the frontend's own tests run against mocks, and
the seam is confirmed by static comparison plus a hand-rolled HTTP client sending
the frontend's exact shapes.

---

## Item 3 and 0b — the receipt

The file sink emits **raw ESC/POS bytes**, plus a byte-derived `.txt` companion
that already existed. `render_invoice_html` adds a third representation: an
**independently coded** HTML renderer, not derived from the byte stream.

**An equivalence test binds them line for line**, and it was falsified by
changing the HTML renderer's GSTIN label to "GST No" and watching it catch the
divergence — which is what demonstrates the two are genuinely independent.

The HTML opens on print, **invoices only**: a browser tab per kitchen ticket
would be worse than none.

**This proves the render, not the device.** It does **not** close the parked
ESC/POS-on-paper gate (ADR-013) — no physical printer exists in this
environment.

### The UPI QR

On the invoice screen and on the receipt. The screen's QR was verified by
rendering it in real Chromium and **decoding the pixels back with `jsqr`**; the
receipt's by reconstructing the SVG's dark modules and decoding with `rqrr`.
Both round-trip to the expected link.

`qrcode` is pinned at `=0.14.1`, default features off, **zero transitive
dependencies**; the decoder is dev-only and not in the shipped binary.

**A divergence the shared vector could not catch.** The screen passed
`inv.invoice_number` as the transaction note and the receipt passed
`ctx.order_display_number` — two QRs for one bill carrying different references.
The pinned vector fixes the encoding given four inputs and says nothing about
which field supplies them: **a vector test proves agreement only for the
arguments both sides are handed.** Aligned on the invoice number at `29df694`,
falsified by pointing one side at the other field and watching
`tn=%23A184` versus `tn=FY26%2FPNQ%2F001423`. Slash encoding inside a query
parameter confirmed by the same real round-trip.

**Two environment variables must hold the same value.** Vite requires the
`VITE_` prefix to expose a variable to the client bundle, so the screen reads
`VITE_HOLLER_DEMO_UPI_VPA`/`_PAYEE_NAME` and the native side reads the
unprefixed pair. **Nothing detects a mismatch.** If the VPA is unset, **no QR
renders at all** — a QR aimed at an empty payee opens a real payment app on a
customer's phone pointed at nobody.

**This is not a payment integration.** Nothing reconciles a scan against a
received payment; the cashier still records the tender.

---

## Items 4 and 6 — banner and presentability

UUID leaks found and fixed on the order list (raw order UUID, truncated KOT
UUID), billing (cash-shift UUID, `reverses_payment_id`), the admin header
(outlet UUID), and the aggregator screen — which showed the raw `menu_item_id`
on every **matched** line and the item name only when unmatched, backwards from
what a human needs. IST rendering added; storage stays UTC.

`bundle.icon` was `[]`, so **the installed executable has been shipping with no
icon at all**. Wired to the existing `.ico`, which is itself a placeholder.

The `SyncBlockedBanner` rewrite preserves the distinction its header draws
between rows **given up on** (`blocked_at` set) and rows **still trying**
(`blocked_at` null, attempts high). Telling an operator "given up" when the truth
is "still retrying" is a different lie in the same family as halting sync
silently.

### Observed in a real browser, and it found three bugs

Chromium via the Playwright already installed under `apps/kds`, driven against
each app's real dev server. **Admin at 1440x900** (backend confirmed by PID
58148 through a health fetch, not by the port answering) and **POS at 1440x900**
with Tauri IPC injected as contract-shaped fixtures, since no Tauri shell can be
launched here.

**The banner deliverable was inverted, and only a browser could see it.**
`.pos-screen` is a CSS grid with every cell claimed by a named area; the three
banners carried no `grid-area`, so auto-placement collapsed the sync banner into
the **140px category sidebar** — the exact opposite of *full width, legible from
a metre*. It was a regression introduced by the presentability pass's own CSS,
and 234 passing tests, a clean `tsc` and a clean `vite build` all saw nothing.
Fixed with a dedicated `.pos-banners { grid-area: banners }` row.

Two pre-existing bugs found in the same run:

- **`.sync-blocked-list { display: flex }` beat the `hidden` attribute**, so
  "Hide details" relabelled the button and left every row visible. This is the
  `[hidden]{display:none}` specificity trap, live.
- `OrderListScreen.tsx` mapped rows with `<>` shorthand fragments, which cannot
  carry a `key` — a real React console warning.

Plus a smaller one: the IST helper rendered lowercase `am`/`pm`, which reads as a
typo on a bill.

After the fixes, screenshots confirm the populated banner full width with both a
**will not reach the cloud** row (HTTP 422, `missing_reference`) and a **still
retrying** row (HTTP 503), each reading `Order #A184 - Butter Chicken` and never
a UUID; the collapse toggle genuinely hiding rows; and the banner element count
at **0 when both queries are empty** — fully absent rather than an empty box.

One false alarm was ruled out by the agent itself: a raw UUID on a category tab
traced to its own fixture using `display_order` where the contract says
`sort_order`, which failed Zod validation and emptied the category map. Not a
product defect.

**The KDS could not be reached**, and was not worked around.
`VITE_KDS_DEVICE_TOKEN` lives in `apps/kds/.env.dev`, which is deny-ruled to
agents exactly as the POS's is, and no edge LAN server is running here anyway.

---

## A finding about the milestone that just closed

**`adr020_outbox_drain` has been red since 2026-09-08, and M6 was tagged
`m6-complete` on 2026-09-11 with it red.** `bdc40d3` added the aggregator pull to
`AppState::drain_outbox` during M6 Phase C; that test's fake cloud excludes only
`/sync/config` from its ingest counter, so it counts a legitimate call as a
replay. Both commits are ancestors of this work's base, confirmed by
`git merge-base --is-ancestor` — nothing in the demo build caused it.

The close therefore either did not run the suite or did not read what it
returned. Filed in `docs/backlog.md` at `e883820`, **with an instruction not to
fix it by widening the exclusion**: the test's subject is that the drain
publishes before the seal and nothing after, and whether the aggregator pull
belongs inside `drain_outbox` at all is the question to answer first.

---

## The devseed idempotence fix is in a commit that does not mention it

`edge/database/src/bin/devseed.rs`'s GRN and opening-stock seeding was not
idempotent: a second run against the same data directory died on
`UNIQUE constraint failed: goods_receipt_note.id` and **aborted before the
bootstrap's device-enrolment steps**, leaving the edge seeded and the KDS and
captain devices un-enrolled — the worst of the three outcomes, because it looks
like it half-worked. The operator re-running the bootstrap on demo morning is
the normal case, not the exceptional one.

The enumeration found a second instance of the same shape: `write_opening_stock`
would have failed on the very next run, invisible only because the GRN aborted
the process first. Both now check for their fixed-id row and skip, which never
attempts a second write against `stock_ledger_entry`'s append-only trigger and
so neither weakens nor routes around it. Three consecutive runs produce
identical counts — `goods_receipt_note: 1`, `grn_line: 7`, `grn_gap: 1` (the
expected `NO_PURCHASE_ORDER`, neither multiplied nor eliminated),
`stock_ledger_entry: 39`, `stock_count_line: 32`.

**That fix lives in `e9ec3dc`, whose message is
`style(admin): adopt @holler/ui tokens and add the header logo`.** Two builders
were running in parallel and the styling track staged a file it did not own,
sweeping the devseed change into its own commit — the exact incident CLAUDE.md's
commit rules describe, this time caused by concurrency rather than by
`git add -A`. The commit is pushed, history rewriting is denied, and rewriting
shared history to correct a message would trade a real risk for a cosmetic gain.
So the pointer is recorded here instead: **`git log` on `devseed.rs` will show an
admin CSS commit message; the change is real, tested and correct.**

The reusable lesson is narrower than "be careful": **staging by path is not
enough when tracks overlap in time.** A builder that stages only its own paths
can still capture another builder's work if that work is sitting staged in the
same index. Ownership boundaries in a brief prevent two agents editing one file;
they do not prevent one agent committing another's.

## Two commits are missing their `Claude-Session:` trailer

`e9e7337` and `29df694`. Both are pushed. The builder flagged them rather than
amending, which was correct — rewriting shared history to add a trailer trades a
real risk for a cosmetic gain. Recorded here instead.
