M4 planning session — plan only, do NOT dispatch or build yet.

## Step 1 — Read the ground truth first
Read RESUME.md, the backlog, all ADRs, and CLAUDE.md. Everything below is external market
input; the repo is the authority on what already exists. For every feature discussed,
mark it partially-built vs greenfield by citing actual code/contracts/ADRs — not memory,
not my summary. If you can't cite the repo for a "partial" claim, call it greenfield.

## Step 2 — Reconcile against the competitive gap list
From a 2026 market scan of the major Indian POS players (Petpooja ~100k outlets is the
benchmark; also Recaho, BillBoox, BhojanSetu). These are the features they ship that we
do not yet, in rough priority by how often the gap loses a deal:

1. Inventory + recipe-level auto-deduction on sale + low-stock alerts + day-end inventory reports
2. Procurement / purchase orders (supplier mgmt, GRN, feeds inventory)
3. Aggregator integration — Swiggy / Zomato / ONDC into one order queue
4. Deep reporting/analytics (their benchmark is ~80 report types; sales, staff actions, inventory consumption)
5. Captain / waiter ordering app (Android, table-side)
6. CRM — customer feedback, SMS/WhatsApp marketing, loyalty programs
7. Online-ordering storefront (own-brand website/widget) + queue management
8. Multi-outlet / central-kitchen management (franchise monitoring, central menus, per-outlet overrides)
9. Accounting integration (Tally etc.) + broad payment-gateway integrations
10. AI layer — demand forecasting, predictive inventory, menu engineering, anomaly detection

For each: state partial-vs-greenfield (with repo citation), rough size, and any dependency
on another item.

## Step 3 — Our constraints that shape all of this
- **Offline-first is non-negotiable.** Every feature must preserve the guarantee that the
  restaurant keeps running with no internet, edge authoritative for transactions. Flag any
  feature that fights this and explain the tension. Inventory especially: name what's
  authoritative where (single-outlet edge vs multi-outlet), how concurrent stock deduction
  across terminals resolves, and how low-stock/sync behave offline. This is where we can be
  BETTER than the cloud-only competitors — stock deduction during an outage is exactly the
  failure mode their cloud inventory can't handle — so get the semantics right, don't gloss.
- **Don't chase 100% parity.** They have 200+ integrations and ~80 reports built over 13
  years; we will not match breadth soon and shouldn't scatter the team trying. Goal =
  parity on the features that close deals + our offline moat + lead on AI where they're
  weak. Prioritize accordingly.
- Our known competitive openings (from their critical reviews): they're slow on AI /
  predictive inventory; they charge extra for loyalty, advanced analytics, WhatsApp; their
  menu changes are slow; late-night support is weak. Bundling what they nickel-and-dime and
  leading on AI are positioning wedges — note where the roadmap can exploit these.

## Step 4 — Propose M4 scope
Recommend M4 = inventory + recipes unless the backlog argues otherwise (it's the #1 gap,
everything in procurement/costing/waste depends on it, and offline-first stock deduction is
a feature we can do better than they can). But:
- Keep M4 to inventory CORE, not everything adjacent. Likely M4 = ingredient/stock model,
  recipe-to-ingredient mapping, auto-deduction on sale, low-stock alerts. Push procurement/
  PO and variance/waste analytics to M4.5 or M5 — they depend on inventory but needn't ship
  with it. If the proposed scope is large, split it into a sequenced track graph (T1..Tn
  like M3), not one big wave.
- Surface the recipe-complexity decision for me: fixed per-dish ingredient quantities
  (simple, ships fast) vs variant/modifier-aware deduction (extra paneer → extra paneer
  deduction; matches the competitor claim, much harder). Propose which belongs in M4 vs
  deferred, and let me decide.
- The 41-item seed menu spec is ready to load — recipes need items to attach to, so note
  where seeding fits.

## Step 5 — Process rules that carry over from M3 (state that they'll be honored)
- Any new schema = contract version bump + ADR + my approval before it's frozen (present
  with the full rubric self-review; wait for approval).
- rust-seams CI gate applies; commit-before-gate; each verifier falsifies a property the
  builder did not target.
- Green-on-absent-data is the recurring trap: any test/harness invariant must be exercised
  with the real data shape (a real deduction, a real low-stock trip), not pass because
  nothing produced the shape.
- Acceptance = observed behaviour on the target, not passing tests. Note M4's acceptance
  plan (inventory is more testable without hardware than M3's printer path — easier, but
  still observed-behaviour).

## Carry-forward items still open (not M4, but keep visible)
- ADR-013 acceptance is marked INCOMPLETE: still needs a real thermal printer (ESC/POS on
  paper) and a real 4GB Windows 10 run. M3 is done-by-test, not done-by-acceptance.
- M2 acceptance item 5 (KDS<->edge LAN session) was silently red for a stretch and has been
  re-evidenced — make sure the acceptance record reflects that.
- CLAUDE.md contracts-version line should read the current frozen baseline (0.4.7,
  migrations through 0012), not 0.3.0, if not already fixed.

## Deliverable for this session
A written M4 plan I approve before any dispatch: repo-grounded partial-vs-greenfield table,
recommended M4 scope as a sequenced track graph, the offline-first analysis for inventory,
the recipe-complexity question teed up for my decision, and where the rest of the
competitive gaps land across M5+. Do NOT dispatch until I approve.
