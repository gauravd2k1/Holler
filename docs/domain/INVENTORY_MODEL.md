# Inventory Domain Model

Source: docs/spec/inventory.md, HOLLER_MASTER_PROMPT.md §19–§26, §29–§30.

## Hierarchy
```
Raw Material → Semi-Finished Product → Recipe → Menu Item
```
Example: Tomato, Onion, Cashew, Butter, Cream, Spices → Makhani Gravy (semi-finished) → Butter Chicken (menu item via recipe).

## Core entities
- **InventoryItem** (raw material): SKU, Name, Category, Base unit, Purchase unit, Conversion, Yield %, Wastage %, Current cost, Weighted average cost, Last purchase price, Reorder level, Par level, Supplier, Storage location, Batch, Expiry, Tax, Outlet.
- **Recipe**: ingredients[], quantity, unit, yield, preparation loss, sub-recipes[]. One recipe version is snapshotted per sale for forensic reproducibility (docs/spec/security-rbac.md §Audit).
- **SemiFinishedBatch**: inputs[], output quantity, input cost, expected vs actual yield, variance, wastage, batch number, production timestamp, expiry.
- **StockLedgerEntry** (immutable, append-only, source of truth for stock):
  ```
  PURCHASE +qty | CONSUMPTION -qty | WASTAGE -qty
  TRANSFER_OUT -qty | TRANSFER_IN +qty | ADJUSTMENT ±qty
  RETURN_TO_VENDOR -qty | PRODUCTION_CONSUMPTION -qty | PRODUCTION_OUTPUT +qty
  ```
  Current stock is a derived/materialized projection for performance; the ledger itself is never overwritten.

## Units
kg, g, litre, ml, piece, dozen, packet, bottle, tray, portion. All conversions explicit (e.g. 1 bag flour = 25 kg); internal calculations always normalize to base units — display units are presentation-only.

## Deduction flow
Confirming a sale of one menu item → resolve its Recipe → for each ingredient, post a `CONSUMPTION` ledger entry for `recipe.quantity` (adjusted for any modifier-driven deltas, e.g. "Extra Paneer +50g" posts an additional consumption entry) → theoretical stock updates immediately; actual stock is reconciled against physical counts (see Variance below).

## Theoretical vs actual consumption
- **Theoretical** = Σ(recipe quantity × units sold) over a period.
- **Actual** = Opening Stock + Purchases + Transfers In − Transfers Out − Closing Stock.
- **Variance** = Actual − Theoretical, reported as quantity, value, and %. Large variances flag over-portioning, waste, spoilage, recipe error, or pilferage (docs/spec/inventory.md).

## Conflict/sync policy
Inventory ledger entries are edge-authoritative, append-only, replayed to cloud — never merged (docs/spec/sync.md conflict table).

## Deferred (per Milestone 4 EXCLUDES)
Procurement integration and batch/expiry alerting are modeled as fields now (batch, expiry on InventoryItem/SemiFinishedBatch) but not acted upon (no alerts, no PO triggers) until Milestones 5 and beyond.
