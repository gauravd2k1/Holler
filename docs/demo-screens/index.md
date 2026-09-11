# Demo screens — presentability check (T14 follow-up)

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
