# Demo screens — presentability check (T14 follow-up, T19 controls/header pass)

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
