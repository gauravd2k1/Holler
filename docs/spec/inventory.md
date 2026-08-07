# Spec: Inventory

Owns: raw materials, recipes, semi-finished goods, stock ledger, wastage, food cost, batch/expiry.
Source: HOLLER_MASTER_PROMPT.md §19–§26, §29, §30.

## Model
```
Raw Material → Semi-Finished Product → Recipe → Menu Item
```
Not "menu item stock" — a proper food inventory engine.

## Inventory item
SKU, Name, Category, Base unit, Purchase unit, Conversion, Yield %, Wastage %, Current cost, Weighted average cost, Last purchase price, Reorder level, Par level, Supplier, Storage location, Batch, Expiry, Tax, Outlet. Units: kg/g/litre/ml/piece/dozen/packet/bottle/tray/portion — conversions explicit, quantities normalized internally.

## Recipes
`Recipe { ingredients, quantity, unit, yield, preparation loss, sub-recipes }`. Confirming a sale produces exact theoretical deductions (e.g. Butter Chicken: Chicken 220g, Makhani gravy 180ml, Butter 20g, Cream 30ml, Kasuri methi 2g).

## Semi-finished / batch production
Track input cost, yield, actual vs expected output, variance, wastage, batch number, timestamp, expiry.

## Ledger (immutable, source of truth)
`PURCHASE +50kg | CONSUMPTION -3kg | WASTAGE -1kg | TRANSFER_OUT -5kg | TRANSFER_IN +5kg | ADJUSTMENT -0.5kg | RETURN_TO_VENDOR -2kg | PRODUCTION_CONSUMPTION -10kg | PRODUCTION_OUTPUT +8kg`. Current stock is derived/projected, never overwritten directly.

## Theoretical vs actual consumption
Theoretical = recipe × units sold. Actual = Opening + Purchases + Transfers In − Transfers Out − Closing. Report variance qty/value/%.

## Food cost
Ingredient/recipe/menu food cost, food cost %, contribution margin. Menu engineering matrix: STAR/PLOWHORSE/PUZZLE/DOG.

## Wastage
Ingredient, quantity, value, reason (Spoilage/Overproduction/Prep loss/Customer return/Kitchen mistake/Breakage/Expired/Unknown), employee, timestamp, manager approval.

## Conflict policy
Inventory transaction: append-only ledger, replayed not merged.

## Milestone note
Batch/expiry alerting and procurement are deferred past Milestone 4 (model fields now, alert/act later) — see HOLLER_MASTER_PROMPT.md §81.
