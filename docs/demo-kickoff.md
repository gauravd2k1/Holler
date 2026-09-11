# Demo build

Purpose: client demo. Scope is presentability and reliability of what exists. No new features, no M6.1, no pilot-only work. Pilot items stay in docs/pilot-readiness.md untouched.

Story to demo, in order:
1. Dine-in order at the till with a modifier → KOT on the KDS → kitchen bumps it.
2. Bill: GST invoice, split cash + UPI, receipt printed to PDF and opened on screen.
3. Stock screen: ingredients deducted per recipe from that sale; low-stock warning visible.
4. Stop the cloud. Take and bill another order. Restart the cloud. Banner clears, order appears in admin.
5. Admin: orders, the received GRN, stock variance.
6. Optional, only if clean three times: fake ONDC order arrives, accepted on the till, billed.

Work, in order:
1. Seed parity. One seed source loads identical menu, variants, recipes, inventory, supplier into BOTH cloud and edge. Ruling: P2/P3/P5 are fixed, so the M5 prohibition on seeding the cloud is lifted. Bootstrap the edge from this clean seed (0035 applies here). Assert after seeding: zero blocked rows, zero deduction gaps, banner empty.
2. Demo seed content: outlet with a real name and GSTIN-shaped placeholder, ~40 items with real prices, ≥10 recipes, opening stock for every ingredient, one supplier, one GRN received. One command resets cloud + edge to this state.
3. Receipt: report what the file-sink emits today. If raw ESC/POS, add a rendered receipt (HTML→PDF or plain text) written beside it and opened on print. Same content as the bytes.
4. Sync banner readable: full width, order id + item + reason legible from a metre.
5. Offline tick: if the till is visibly sluggish with the cloud down, widen the pump interval via config for the demo build and record it.
6. Presentability: window title and icon; no dev labels, raw UUIDs or internal notes on till, KDS or admin screens; IST timestamps; consistent naming. List every screen the story touches; screenshot each for review.
7. docs/demo-script.md: the six steps with exact clicks, expected screen, and fallback. Include the cloud stop/start commands for step 4.
8. Three full rehearsals from a clean reset, each timed; each failure fixed or the step cut. Record one clean run.

Report demo blockers only.
