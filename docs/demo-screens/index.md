# Demo screens — presentability check (T14 follow-up, T19 controls/header pass)

## T23 update (2026-09-11) — Inter now ships; every screen in this set re-taken or confirmed against it

`packages/ui` now bundles Inter as self-hosted variable woff2 (`fonts.css` +
`fonts/`), imported by `tokens.css`. Every screenshot in this directory was
taken (or re-taken) against the real face, not Segoe UI — nine files below
are **new captures in this pass** (five `captain-*`, two `admin-*`, two
`pos-*`); the rest were captured in earlier passes but are unaffected because
`--font-sans` already put Inter first in the stack, so the only change
visible on them is that the fallback no longer fires. See T23's own report
for the byte cost, the variable-vs-static comparison, the ₹-glyph
(`fontTools` cmap) confirmation, and separate dev/build/captain-listener
verification.

**Re-taken in this pass**, driven by real `pnpm dev` servers with either
`window.__TAURI_INTERNALS__.invoke` (POS) or `page.route` (`apps/captain`,
`apps/admin`) mocked with contract-shaped fixtures, never a `page.goto`
reload after login/pairing, each screen's own unique content asserted before
capture (throwaway scripts, session scratch directory, not part of this
repository):

| File | Screen | Notes |
|---|---|---|
| `captain-pair.png` | Pair | Token pasted, not yet submitted |
| `captain-tables.png` | Tables | Free vs. occupied, colour + text tag |
| `captain-menu-cart.png` | Menu + cart | 1 item / ₹220.00 in the cart bar |
| `captain-modifier-sheet.png` | Free-modifier sheet | "Chicken Biryani"'s required "Spice Level" group |
| `captain-sent.png` | Sent confirmation | Order A211, two KOTs (GRILL, MAIN), both QUEUED |
| `admin-sign-in.png` | Admin — Sign in | Email/password filled, not yet submitted |
| `admin-menu.png` | Admin — Menu and pricing | Chicken Biryani/Paneer Tikka/Masala Chai, ₹450.00 shown |
| `pos-order-list.png` | POS — Order List | One CONFIRMED (A184) + one DRAFT (A185) order |
| `pos-billing.png` | POS — Billing | Order A184 billed, invoice FY26/PNQ/001423, ₹440.00, one CASH payment captured |

Two schema corrections made while building the fixtures, recorded because
each would otherwise silently produce a wrong or stuck screen:
`CanonicalOrder.status` is `SENT_TO_KITCHEN`, not `SENT` (an invalid-enum
zod error, not a network failure, stalled the captain send flow); and
`MenuItem` requires `config_version` (missing on the first attempt — the
admin menu screen's `isError` branch caught it, not a silent drop).

### Full current set, MD5, 21 files pairwise distinct (`md5sum docs/demo-screens/*.png`)

```
65ef593004e37f919c9a7b62171d6764  admin-goods-receipts.png
b6a7d9b680706825351bb51b7ca2f355  admin-menu.png
39cfe9fc071ab29c5d70cb995833e57e  admin-sign-in.png
d21c25d317e82c94d8ebbb368a5e0669  admin-suppliers.png
a1811fe7a34d09cb0814b715c3e40077  captain-menu-cart.png
e3d1668d3ffd6757c708ba3fab7c2f2e  captain-modifier-sheet.png
bc586445684cf7d100ef01868fc918fa  captain-pair.png
e74d9b334b08f2f8b37b54ea57147cfa  captain-sent.png
1538445032fc1ee572363e76f07d0792  captain-tables.png
898c22bafbb4ebf8493362c8d8c2f332  kds-ticket.png
f4cf0bdc20616b063f8270ff81fd5b97  pos-aggregator-orders.png
eb4db7380d18c65121dbc080e4da92b5  pos-billing.png
7acadd1008c32911ba0bb771846b827e  pos-billing-upi-qr.png
6e3528a35bd33115d797d72d165d94de  pos-crash-screen.png
887b0c7e3e1224f744da1bf38f3d4f03  pos-current-stock.png
a0b782731c989abddf2905651cd371c2  pos-grn-gaps.png
1fb0db03af5166cfb7a41b8d3364c0c6  pos-order-list.png
1c85a41d8a44bc23c94f071c7561425f  pos-order-list-empty.png
d2c0985bacbad90897a2d8803a9ce3db  pos-order-list-kots.png
db1c969ea99617e31c16bd260d68b162  pos-purchase-return.png
1cbd58a26aca8c5292ad7308cb417461  pos-receiving.png
```

21 files, 21 distinct hashes (checked with `sort | uniq -d`, empty output).
Every file listed under "Re-taken in this pass" has a hash superseding its
prior entry further down this document; the earlier entries are left as
history, not deleted.

**Not re-taken in this pass** (unaffected because Inter was already first in
`--font-sans`; a build/browser check for the face itself, not per-screen,
covers them — see T23's report): `admin-goods-receipts.png`,
`admin-suppliers.png`, `kds-ticket.png`, `pos-aggregator-orders.png`,
`pos-billing-upi-qr.png`, `pos-crash-screen.png`, `pos-current-stock.png`,
`pos-grn-gaps.png`, `pos-order-list-empty.png`, `pos-order-list-kots.png`,
`pos-purchase-return.png`, `pos-receiving.png`.

## T22 update (2026-09-11) — `admin-menu.png` re-captured, `pos-aggregator-orders.png` captured (first ever)

Closes the presentability set. **No application code changed in this pass** —
`apps/admin/` and `apps/pos/` are read-only to this track; both are exactly as
T20/T21 left them.

### `admin-menu.png` — re-captured

Stale since `3ae0ba4` (admin money-formatting pass): the committed image
predated `formatPaiseAsRupees`'s ₹ prefix and `.money` tabular figures on
`MenuScreen`. Re-captured against the running `apps/admin` dev server
(`pnpm dev`, port 5175), signed in, and — since `MenuScreen` starts on the
"Menu and pricing" tab by default, no further in-app navigation was needed —
asserted **both** money renderings before capture: the display cell for
"Chicken Biryani" reads `₹450.00`, and after clicking that row's "Edit"
button the price `<input>`'s value is `"450.00"` (plain decimal, no ₹
prefix). **Both confirmed present and correct — this is the designed
behaviour, not a bug**: the input is deliberately unprefixed because it
round-trips through `parseRupeesToPaise` on save, and a ₹ in that string
would break the parse. The captured image shows the Chicken Biryani row
mid-edit (its `Save`/`Cancel` buttons and plain-decimal input visible) beside
Paneer Tikka and Masala Chai still in display mode (`₹320.00`, `₹40.00`),
so both states are visible in the one frame.

Mocked via `page.route` against contract-shaped fixtures (`POST /auth/login`,
`GET /menu/items`, `GET /menu/categories`, plus empty-list stubs for
`/procurement/suppliers` and `/procurement/goods-receipts` so those tabs stay
inert); no `page.goto` after sign-in. Throwaway script
(`shoot-admin-menu.mjs`, session scratch directory), deleted after the
capture.

### `pos-aggregator-orders.png` — captured for the first time

`AggregatorOrdersScreen` (demo step 6, the optional ONDC step) was
token-converted and `.money`-applied in an earlier pass but never
screenshotted. Captured against the running `apps/pos` dev server (`pnpm
dev`, port 5173) with `window.__TAURI_INTERNALS__.invoke` mocked via
`addInitScript` (no real Tauri shell here): `login`, `list_menu_items`,
`list_menu_categories`, `list_menu_item_variants` (empty), `list_tables`
(empty), `get_active_draft_order` (null), and
`list_unaccepted_aggregator_orders` returning one ONDC document with two
lines. Navigated by clicking "Platform Orders" on `PosScreen` after login —
never a `page.goto` reload (the in-memory `useAuthStore` would drop).

**The fixture deliberately carries one matched line and one unmatched
line**, per the task note that an earlier pass fixed a defect where a
matched line showed the raw `menu_item_id` and only an unmatched line showed
a name — backwards from what the fixed code does. Confirmed on screen before
capture: the matched line ("Chicken Biryani (Full)") resolves in the
"Matched to this menu" column to the item's **name**, `"Chicken Biryani"`
(never the UUID); the unmatched line ("Weekend Thali Combo") shows
`"not matched"` in that column and its own platform-supplied name in the
item column. Both are visible together in the captured frame, so the
screenshot exercises the fixed path rather than only the unmatched one.

Throwaway script (`shoot-pos-aggregator.mjs`, session scratch directory),
deleted after the capture.

### Full current set, MD5, 21 files pairwise distinct

```
0f4758073f2139056caa9eccb1907398  captain-tables.png
1c85a41d8a44bc23c94f071c7561425f  pos-order-list-empty.png
1cbd58a26aca8c5292ad7308cb417461  pos-receiving.png
412a572f9d01d7ebb3080ce0c47299f0  captain-pair.png
50ae7ecf0fdf025e7157d37efd768419  captain-modifier-sheet.png
55e95f8bc0eff9f9f3c0a8d878437237  admin-sign-in.png
65ef593004e37f919c9a7b62171d6764  admin-goods-receipts.png
6e3528a35bd33115d797d72d165d94de  pos-crash-screen.png
7acadd1008c32911ba0bb771846b827e  pos-billing-upi-qr.png
887b0c7e3e1224f744da1bf38f3d4f03  pos-current-stock.png
898c22bafbb4ebf8493362c8d8c2f332  kds-ticket.png
92f4fd83078de61f1294d0205e92a560  pos-billing.png
9e975e907ab7084a7248714e98594f8e  captain-menu-cart.png
a0b782731c989abddf2905651cd371c2  pos-grn-gaps.png
a15138dc8bce8025d57f0d1f81ef6e8e  captain-sent.png
b9ee734280214518ab808bbf11fa1782  admin-menu.png
ca2946ee5e1d61fa4322571e789dc47f  pos-order-list.png
d21c25d317e82c94d8ebbb368a5e0669  admin-suppliers.png
d2c0985bacbad90897a2d8803a9ce3db  pos-order-list-kots.png
db1c969ea99617e31c16bd260d68b162  pos-purchase-return.png
f4cf0bdc20616b063f8270ff81fd5b97  pos-aggregator-orders.png
```

21 files, 21 distinct hashes. `admin-menu.png`'s hash changed from
`a0c246eff50ee9772cc18649cc7685e2` (T20/T21, stale) to
`b9ee734280214518ab808bbf11fa1782` (this pass). `pos-aggregator-orders.png`
is new.

### Screen table addition

| File | Screen | Demo step | What it shows |
|---|---|---|---|
| `admin-menu.png` | Admin — Menu and pricing | Step 5 | ₹-prefixed display prices on two rows, the third row mid-edit showing the plain-decimal (unprefixed) price input — both money renderings intentional, side by side |
| `pos-aggregator-orders.png` | POS — Delivery-Platform Orders | Step 6 (optional, ONDC) | One ONDC document with a matched line (resolves to the menu item's name) and an unmatched line (shows "not matched"), exercising the fixed matched/unmatched display path |

## T20 update (2026-09-11) — billing screen fixes, re-taken `pos-billing.png`

T19's `pos-billing.png` was captured without `VITE_HOLLER_DEMO_UPI_VPA` set,
so the customer-facing UPI QR — the single most important thing on that
screen for the demo — was absent. `UpiPaymentQr` (`components/UpiPaymentQr.tsx`,
`domain/upi.ts`) renders **nothing at all** when no demo payee VPA is
configured — deliberately: a QR aimed at an empty payee would open a real
payment app on a customer's phone pointed at nobody. **The QR only ever
appears when `VITE_HOLLER_DEMO_UPI_VPA` (and optionally
`VITE_HOLLER_DEMO_UPI_PAYEE_NAME`) is set at build/dev-server time** — set
them in `apps/pos/.env.local` (untracked) or in the environment before
`pnpm dev`/`pnpm build`. The re-take below used `demo@upi` /
`Holler Demo Kitchen`, an obviously-fake payee.

Two further `BillingScreen.tsx`/`index.css` fixes are in this same
screenshot:

- The "Bill" card used to render as a bare `<h2>Bill</h2>` with an empty
  body once an invoice existed (everything it would show is already
  rendered per-invoice below in `.invoice-detail-panel`) — the card is now
  dropped once `hasInvoices` is true, and shows a real one-line empty state
  ("No unbilled order — send an order to the kitchen first.") when there is
  nothing yet to bill, instead of a bordered box with nothing in it.
- "Open Shift", "+ Add Tender" and "Record Payment(s)" were stretching to
  the full card width — their flex-column parent containers defaulted
  `align-items` to `stretch`, pulling every `inline-flex` `.btn` along with
  them. Fixed with `align-items: flex-start` on those containers;
  "Record Payment(s)" opts into full width explicitly via `.btn--block`,
  since it is the screen's one genuinely terminal action.

`pos-billing.png` was re-captured with a throwaway Playwright script
(scratch directory, not part of this repository) driving the real
`pnpm dev` server with `VITE_HOLLER_DEMO_UPI_VPA=demo@upi` and
`VITE_HOLLER_DEMO_UPI_PAYEE_NAME="Holler Demo Kitchen"` set,
`window.__TAURI_INTERNALS__.invoke` mocked with contract-shaped fixtures
(login → orders list → Bill → invoice detail), navigated entirely by
clicking in-app (never a `page.goto` reload after login), and asserted on
the invoice number and "Scan to pay via UPI" text being on screen before
capture. No other file in this set changed in this pass.

`pos-billing-upi-qr.png` (the standalone QR crop from the T14 pass) is
unchanged and kept for its own MD5 below.

**`pos-billing.png` MD5, this pass:** `92f4fd83078de61f1294d0205e92a560`
(supersedes the T19-pass hash `f69af7491d769204905582bb38c62088` recorded
below). Confirmed distinct from every other file in this directory.

## T20 part 2 (2026-09-11) — `apps/captain` and `apps/admin` screenshots (first ever)

Neither app had a screenshot in this directory before this pass, even though
both were converted to the shared tokens by earlier tracks and observed in a
browser at the time — that track simply did not commit the images. Both apps
are **read-only** in this track; nothing in `apps/captain/` or `apps/admin/`
changed.

Both sets used the same discipline as the T14/T19/T20-part-1 POS captures:
a throwaway Playwright script per app (scratch directory, not part of this
repository), driving each app's real `pnpm dev` server, `/api/*` mocked with
`page.route` against contract-shaped fixtures (never a stub of a hand-rolled
shape), navigated end to end by clicking in-app — sign-in/pair once, then
every further screen reached by a real click, never a `page.goto` reload —
and each screen's own unique heading/content asserted on screen before
capture.

### `apps/captain` — demo step 1a, the waiter's phone, captured at **390px width**

| File | Screen | Notes |
|---|---|---|
| `captain-pair.png` | Pair | Device token pasted, not yet submitted |
| `captain-tables.png` | Tables | Free vs. occupied shown by colour **and** the text tag ("Free"/"Occupied"), never colour alone |
| `captain-menu-cart.png` | Menu + cart | Cart bar pinned at the bottom with 1 item / ₹220.00, an unavailable item shown greyed with a "Not available" tag |
| `captain-modifier-sheet.png` | Free-modifier sheet open | "Chicken Biryani"'s required "Spice Level" group, backdrop dimming the menu behind it |
| `captain-sent.png` | Sent confirmation | Order A211 with its two KOTs (GRILL, MAIN), both QUEUED |

Flow driven: pair → tables → select a free table → add a plain item (cart bar
appears) → tap an item with a free modifier group (sheet opens) → resolve it
→ Send → sent screen. `POST /api/orders` and `POST /api/orders/{id}/send`
were both mocked; no real order was created anywhere.

### `apps/admin` — demo step 5, captured at **1440×900** (desktop)

| File | Screen | Notes |
|---|---|---|
| `admin-sign-in.png` | Sign in | Email/password filled, not yet submitted |
| `admin-menu.png` | Menu and pricing | Three items, one with `hsn_sac: null` correctly shown as "not set — cannot be billed", the replica-note banner visible |
| `admin-suppliers.png` | Suppliers | "Add a supplier" form plus one existing supplier (Fresh Vegetable Traders) with its one pack-size row — **re-captured in T21**, see below |
| `admin-goods-receipts.png` | Goods receipts | **The screen the demo story actually shows** — `GRN/20260902/0007`, all three quantity columns (entered/pack size/base), the replica-note banner |

Flow driven: sign in → Menu and pricing → click "Suppliers" → click "Goods
receipts". `POST /auth/login`, `GET /menu/items`, `GET /menu/categories`,
`GET /procurement/suppliers` and `GET /procurement/goods-receipts` were all
mocked.

**Two things previously observed in `apps/admin` — both fixed in T21, see the
T21 section below. Left here, marked resolved, rather than deleted, so the
history of what was wrong and when it was fixed stays readable:**

1. ~~**`admin-suppliers.png` shows a raw UUID**~~ **RESOLVED IN T21.**
   `SuppliersScreen.tsx` rendered `it.inventory_item_id` directly.
   `GoodsReceiptsScreen.tsx` on the same app already handled the identical
   gap correctly (no `inventory_item_name` on the wire shape, so it shows
   "ingredient on file" instead of the id); `SuppliersScreen.tsx` now does
   the same, for the same reason (the contract gap — `SupplierItemSchema`
   has no denormalised name — is still open and is not this app's to close).
2. ~~**Money on `admin-menu.png` and `admin-suppliers.png` has no ₹
   symbol**~~ **RESOLVED IN T21.** `apps/admin/src/lib/money.ts` now exports
   `formatPaiseAsRupees` (₹-prefixed, matching
   `apps/pos/src/domain/money.ts` byte for byte) for display, and
   `formatPaiseAsPlainDecimal` (unprefixed) for seeding the editable price
   input on `MenuScreen` so it still round-trips through
   `parseRupeesToPaise` on save.

### Full-set hash check, this pass

All 20 files in this directory, MD5, confirmed pairwise distinct
(`md5sum docs/demo-screens/*.png`):

```
0f4758073f2139056caa9eccb1907398  captain-tables.png
1c85a41d8a44bc23c94f071c7561425f  pos-order-list-empty.png
1cbd58a26aca8c5292ad7308cb417461  pos-receiving.png
412a572f9d01d7ebb3080ce0c47299f0  captain-pair.png
50ae7ecf0fdf025e7157d37efd768419  captain-modifier-sheet.png
55e95f8bc0eff9f9f3c0a8d878437237  admin-sign-in.png
65ef593004e37f919c9a7b62171d6764  admin-goods-receipts.png
6e3528a35bd33115d797d72d165d94de  pos-crash-screen.png
7acadd1008c32911ba0bb771846b827e  pos-billing-upi-qr.png
887b0c7e3e1224f744da1bf38f3d4f03  pos-current-stock.png
898c22bafbb4ebf8493362c8d8c2f332  kds-ticket.png
92f4fd83078de61f1294d0205e92a560  pos-billing.png
9e975e907ab7084a7248714e98594f8e  captain-menu-cart.png
a0b782731c989abddf2905651cd371c2  pos-grn-gaps.png
a0c246eff50ee9772cc18649cc7685e2  admin-menu.png
a15138dc8bce8025d57f0d1f81ef6e8e  captain-sent.png
ca2946ee5e1d61fa4322571e789dc47f  pos-order-list.png
d2c0985bacbad90897a2d8803a9ce3db  pos-order-list-kots.png
db1c969ea99617e31c16bd260d68b162  pos-purchase-return.png
f76061c6163d9891e15cd46fd025afdf  admin-suppliers.png
```

20 files, 20 distinct hashes.

## T21 update (2026-09-11)

Two display defects on `admin-suppliers.png`, both found by reviewing the
screenshot directly (see the "resolved" notes above): a raw
`inventory_item_id` UUID in the pack-size table's "Item" column, and money
rendered with no ₹ symbol (`"1800.00"` instead of `"₹1800.00"`).

**Fix, `apps/admin/src/`:**
- `components/SuppliersScreen.tsx` — the Item cell now shows `"ingredient on
  file"`, following `GoodsReceiptsScreen.tsx`'s existing pattern exactly (no
  new fetch: this app has no inventory-items query anywhere to resolve the id
  against, and the contract gap — `SupplierItemSchema` has no
  `inventory_item_name` — is left with the operator, not worked around here).
- `lib/money.ts` — `formatPaise` split into `formatPaiseAsRupees` (₹-prefixed
  display, matching `apps/pos/src/domain/money.ts`'s function of the same
  name field-for-field: integer `Math.trunc`/`%` div-mod by 100, no float
  arithmetic) and `formatPaiseAsPlainDecimal` (unprefixed, for seeding
  `MenuScreen`'s editable price input, which round-trips through
  `parseRupeesToPaise` on save — a ₹ prefix in that string would break the
  parse). `.money` (tabular figures) added to every money cell touched:
  `SuppliersScreen`'s last-price column, `GoodsReceiptsScreen`'s line-total
  column, `MenuScreen`'s price column.
- `lib/money.test.ts` (new) — exact-case tests for `formatPaiseAsRupees`
  (whole rupee, sub-rupee remainder, large total, zero, negative) and one
  round-trip test for `formatPaiseAsPlainDecimal` through
  `parseRupeesToPaise`. Watched RED first: `formatPaiseAsRupees is not a
  function` / `formatPaiseAsPlainDecimal is not a function` against the
  pre-fix `money.ts`, then green after the split above. 6 new tests; 12 total
  in the app, all passing, ~770ms (`pnpm test` via
  `scripts/assert-tests-ran.mjs`).

`admin-suppliers.png` re-captured: signed in, clicked "Suppliers" (never
`page.goto` after login), asserted `"Fresh Vegetable Traders"`,
`"ingredient on file"` and `"₹1800.00"` were on screen and that the raw
`inventory_item_id` was NOT present, before capturing. `/auth/login`,
`/menu/items`, `/menu/categories`, `/procurement/suppliers` and
`/procurement/goods-receipts` mocked with contract-shaped fixtures via a
throwaway Playwright script in the session scratch directory, not part of
this repository — same discipline as T14/T19.

Full current set, MD5, all 20 files pairwise distinct
(`md5sum docs/demo-screens/*.png`):

```
0f4758073f2139056caa9eccb1907398  captain-tables.png
1c85a41d8a44bc23c94f071c7561425f  pos-order-list-empty.png
1cbd58a26aca8c5292ad7308cb417461  pos-receiving.png
412a572f9d01d7ebb3080ce0c47299f0  captain-pair.png
50ae7ecf0fdf025e7157d37efd768419  captain-modifier-sheet.png
55e95f8bc0eff9f9f3c0a8d878437237  admin-sign-in.png
65ef593004e37f919c9a7b62171d6764  admin-goods-receipts.png
6e3528a35bd33115d797d72d165d94de  pos-crash-screen.png
7acadd1008c32911ba0bb771846b827e  pos-billing-upi-qr.png
887b0c7e3e1224f744da1bf38f3d4f03  pos-current-stock.png
898c22bafbb4ebf8493362c8d8c2f332  kds-ticket.png
92f4fd83078de61f1294d0205e92a560  pos-billing.png
9e975e907ab7084a7248714e98594f8e  captain-menu-cart.png
a0b782731c989abddf2905651cd371c2  pos-grn-gaps.png
a0c246eff50ee9772cc18649cc7685e2  admin-menu.png
a15138dc8bce8025d57f0d1f81ef6e8e  captain-sent.png
ca2946ee5e1d61fa4322571e789dc47f  pos-order-list.png
d21c25d317e82c94d8ebbb368a5e0669  admin-suppliers.png
d2c0985bacbad90897a2d8803a9ce3db  pos-order-list-kots.png
db1c969ea99617e31c16bd260d68b162  pos-purchase-return.png
```

20 files, 20 distinct hashes. `MenuScreen.tsx`'s price cell was also switched
to `formatPaiseAsRupees` plus `.money` (the shared `formatPaise` import it
held no longer exists), so the *code* behind `admin-menu.png` now renders ₹
too — but the file itself is **stale, not re-captured**: this track's owned
screenshot paths are `admin-suppliers.png` and this index only, so
`admin-menu.png` on disk still shows the pre-fix render until a track that
owns it re-captures it.

## T19 update (2026-09-11)

T14 adopted the colour tokens (`.money`, status colours, the logo on
`PosScreen`) but left the *controls* on raw browser defaults and the
sub-screens with no header at all. T19 closes that: `.btn`/`.btn--primary`/
`.btn--danger` on every control on `OrderListScreen`, `BillingScreen` and
`CurrentStockScreen`; `.holler-header` (same logo, same position as
`PosScreen`) on all three; their content grouped into `.card` blocks
separated by `--space-5` (`.screen-body`); and the three screens honestly
left un-`.money`'d by T14 — `AggregatorOrdersScreen`, `PurchaseReturnScreen`,
`ReceivingScreen` — now carry it on every money cell. No fetch, command or
displayed value changed anywhere; layout and class names only.

Five files below were **re-captured** in this pass, driven by a fresh
`browser-check`-style harness (`shoot.mjs`, throwaway, not part of this
repository — same discipline as T14's: real `pnpm dev` server, real
Chromium, `window.__TAURI_INTERNALS__.invoke` mocked with contract-shaped
fixtures, every screen asserted on its own unique content before capture,
never a bare `page.goto` reload after login (that drops the in-memory-only
`useAuthStore`), and the whole set MD5-hashed afterward to prove no two
files are byte-identical:

| File | Screen | What changed |
|---|---|---|
| `pos-order-list.png` | Order List | `.holler-header` (logo + title), `.card` around the table, `.btn`/`.btn--primary` on every action |
| `pos-order-list-kots.png` | Order List, KOT panel expanded | Same header/card/button pass; KOT status-transition buttons now `.btn` |
| `pos-order-list-empty.png` | Order List, zero orders | Confirms the header/card treatment holds with an empty table body |
| `pos-billing.png` | Billing, full invoice | `.holler-header`, every section (Cash Shift / Bill / Invoice / Payments / Take Payment) now its own `.card`, `--space-5` between them, `.btn--primary` on Issue Bill / Record Payment(s), `.btn--danger` on Void/Refund |
| `pos-current-stock.png` | Current Stock | `.holler-header` (logo + title + seven nav actions + Back, wrapping on this one row deliberately — see `.current-stock-screen .holler-header` in `index.css`), `.card` around the table |

`pos-billing-upi-qr.png`, `pos-crash-screen.png`, `pos-receiving.png`,
`pos-purchase-return.png`, `pos-grn-gaps.png` and `kds-ticket.png` are
**unchanged files from the T14 pass**, kept for their MD5s below. Layout on
`ReceivingScreen` and `PurchaseReturnScreen` was not touched by T19 (only
`.money` spans were added, per the task's explicit "layout only" scope for
the three finish-`.money` screens) and `AggregatorOrdersScreen` still has no
screenshot of its own, same gap T14 recorded.

**MD5 set, this pass** (`md5sum docs/demo-screens/*.png`):

```
898c22bafbb4ebf8493362c8d8c2f332  kds-ticket.png
f69af7491d769204905582bb38c62088  pos-billing.png
7acadd1008c32911ba0bb771846b827e  pos-billing-upi-qr.png
6e3528a35bd33115d797d72d165d94de  pos-crash-screen.png
887b0c7e3e1224f744da1bf38f3d4f03  pos-current-stock.png
a0b782731c989abddf2905651cd371c2  pos-grn-gaps.png
ca2946ee5e1d61fa4322571e789dc47f  pos-order-list.png
1c85a41d8a44bc23c94f071c7561425f  pos-order-list-empty.png
d2c0985bacbad90897a2d8803a9ce3db  pos-order-list-kots.png
db1c969ea99617e31c16bd260d68b162  pos-purchase-return.png
1cbd58a26aca8c5292ad7308cb417461  pos-receiving.png
```

Eleven files, eleven distinct hashes.



Screenshots of the screens the demo touches, taken after the T14 token
adoption pass, in Chromium (1440x900) against each app's real `pnpm dev`
server. `window.__TAURI_INTERNALS__.invoke` was mocked with contract-shaped
fixtures for the POS; the KDS's LAN WebSocket was mocked the same way the
captain track mocked `/api/*`, one layer down. Every screenshot below was
taken only after asserting the screen's own unique heading/content was
actually on screen (never a bare `page.goto` + timeout), and the full set was
MD5-hashed afterward to confirm no two files are byte-identical — the
harness that produced this set previously wrote nine identical copies of the
login screen under nine different names, which is the exact failure this
process now checks for before any file is kept.

The harness itself (`browser-check2.mjs`) was a throwaway script in the
session's scratch directory and is not part of this repository.

| File | Screen | Demo step | What changed |
|---|---|---|---|
| `pos-order-list.png` | POS — Order List, one CONFIRMED order + one DRAFT | Step 1 | Light theme, `.money` tabular totals, logo in nothing here (list has no header logo — see `pos-current-stock.png` etc for the top-bar logo) |
| `pos-order-list-kots.png` | POS — Order List with the KOT panel expanded | Step 1 | Same, kitchen-ticket sub-table now on tokens too |
| `pos-order-list-empty.png` | POS — Order List, zero orders | Step 1 (empty state) | Confirms the empty state is not blank/broken on light |
| `pos-billing.png` | POS — Billing, full invoice with two lines, CGST/SGST, round-off, UPI QR, payments table | Step 2 | Every money figure now carries `.money` (tabular figures) — previously applied nowhere in this app |
| `pos-billing-upi-qr.png` | POS — Billing, UPI QR block cropped | Step 2 | Confirms the dark-module QR still reads clearly against the light `.card` background |
| `pos-current-stock.png` | POS — Current Stock, one LOW row and one BELOW ZERO row | Step 3 | Warn (amber) and danger (red) tags both legible on light, tinted row backgrounds, no colour-only signal (each also carries a text tag) |
| `kds-ticket.png` | KDS — three tickets pushed over a mocked LAN socket, ages 1/11/24 min | Step 1 payoff | Dark surface mode, logo in header, SLA border colours (green/amber/red) matching the till's own status colours |
| `pos-crash-screen.png` | POS — CrashScreen, triggered by a real render-phase throw | Not in demo script; requested anyway | Danger-tinted card (light `--color-danger-bg`, not the raw danger colour) with dark text — legible, not a wall of red |
| `pos-receiving.png` | POS — Receiving (procurement, lower priority) | Not in demo script | Form on tokens, "no permission" message legible |
| `pos-purchase-return.png` | POS — Purchase Return (procurement, lower priority) | Not in demo script | Same |
| `pos-grn-gaps.png` | POS — Delivery Problems (procurement, lower priority) | Not in demo script | Same |

## Findings

- **`.money`'s tabular-figure rule was applied nowhere in the app before this
  pass** — confirmed by grepping for `className="money"` across
  `apps/pos/src` and finding zero matches, despite `formatPaiseAsRupees`
  being called in seven components. Added to `OrderListScreen`,
  `BillingScreen` (16 occurrences: every line total, subtotal, discount,
  round-off, grand total, amount due, payment amount, entered/remaining
  tender), `PosScreen` (item price, variant price, modifier delta, cart line
  total, subtotal), and `UpiPaymentQr`. **Not yet applied**:
  `AggregatorOrdersScreen`, `PurchaseReturnScreen`, `ReceivingScreen`'s own
  money cells — out of scope for this pass (procurement is lower priority and
  the file count was already large); filed here rather than silently left.
- **UPI QR block correctness bug found while building the fixture, not a
  styling defect**: a hand-built invoice fixture with `grand_total_paise:
  87150` silently failed `InvoiceSchema`'s own `grand_total_paise % 100 ===
  0` refine inside `listInvoicesForOrder`, which made `hasInvoices` false and
  skipped the entire invoice panel — including the QR — with no visible
  error anywhere. Not a product defect (the refine is doing exactly its job);
  recorded because it is the second time in this session a screenshot
  silently showed the wrong thing for a non-obvious reason, and the fix
  (correct the fixture's rounding) is what makes `pos-billing.png` and
  `pos-billing-upi-qr.png` genuine.
- **No text found invisible or near-invisible against the new light ground**
  across the eleven screens above.
- **No status now reads as the wrong severity.** Danger/warn/ok/info are
  visually distinct in every screenshot and every occurrence also carries a
  text label — never colour alone.

## Screens still not observed

- `TicketCard`'s **status-transition interaction** (clicking "Mark ready" /
  "Accept" and watching the pending/timed-out state) — the static render is
  captured in `kds-ticket.png`, but no click was driven through it in this
  pass.
- `AggregatorOrdersScreen`, `StockCountScreen`, `WastageScreen`,
  `StockDeductionGapsScreen` — token-converted in the earlier commit but not
  individually screenshotted in either pass.

## Captain polish pass — 2026-09-12 (`docs/demo-screens/captain/`)

Phone viewport **390x844**, `deviceScaleFactor: 2`, `isMobile`, captured against
the captain's own Vite dev server on a **scratch port (5177)** with a
contract-shaped stub API on **9410**. Nothing in this capture touched 8080,
9310 or the real captain port.

| File | Screen | What it shows |
|---|---|---|
| `captain-01-pair.png` | Pair | The token field, before any device is paired. |
| `captain-02-tables.png` | Tables | Free/occupied by colour **and** a word, six tables. |
| `captain-03-menu.png` | Menu | Gong categories and prices; an unavailable item greyed with a "Not available" tag. |
| `captain-04-empty-category.png` | Menu, empty category | **The new empty state** — a category with no items now says so instead of rendering a blank region. |
| `captain-05-cart.png` | Cart bar | One line added, count and total in the bar. |

Every file was asserted on its own unique content **before** being
photographed, and the set was hashed afterwards: five files, five distinct
hashes, zero console errors.
