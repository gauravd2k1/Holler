# Spec: Procurement

Owns: suppliers, purchasing flow, central kitchen.
Source: HOLLER_MASTER_PROMPT.md §27, §28.

## Entities
Supplier, Purchase Requisition, RFQ, Purchase Order, Goods Receipt Note (GRN), Supplier Invoice, Purchase Return, Supplier Credit, Payment status.

## Flow
Stock Low → Purchase Requisition → Approval → Purchase Order → Supplier → GRN → Inventory → Invoice → Accounts. Approval limits enforced.

## Central kitchen
Modeled as an inventory/production location. Flow: Outlet indent → Central kitchen approval → Production → Dispatch → Goods in transit → Outlet receipt → Inventory update. Track dispatched-vs-received variance.

## Cross-context dependencies
- Inventory (docs/spec/inventory.md) — GRN posts PURCHASE/PRODUCTION_OUTPUT ledger entries.
- Multi-outlet (docs/spec/multi-outlet.md) — central kitchen spans outlets.
