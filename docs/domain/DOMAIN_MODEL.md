# Holler Domain Model

## Tenant hierarchy
```
Organisation
└── Brand
    └── Outlet
        ├── Revenue Centers
        ├── Floors
        ├── Tables
        ├── Kitchens
        ├── Stations
        ├── Registers
        └── Devices
```
See docs/spec/multi-outlet.md. Never assume one restaurant = one outlet.

## Bounded contexts (backend/internal/<context>)
| Context | Owns | Spec |
|---|---|---|
| auth | authentication, sessions, tokens | docs/spec/security-rbac.md |
| tenant | organisation/brand/outlet hierarchy | docs/spec/multi-outlet.md |
| outlet | outlet settings/config | docs/spec/multi-outlet.md |
| menu | menu/category/item/variant/modifier/pricebook | docs/spec/menu.md |
| ordering | order lifecycle, canonical order model | docs/spec/ordering.md |
| kitchen | KOT, stations, KDS | docs/spec/kitchen.md |
| inventory | raw materials, recipes, ledger | docs/spec/inventory.md |
| procurement | suppliers, PO/GRN, central kitchen | docs/spec/procurement.md |
| payments | payment/refund/settlement/reconciliation | docs/spec/payments.md |
| aggregators | Swiggy/Zomato/UrbanPiper gateway | docs/spec/aggregators.md |
| compliance | GST tax engine, invoicing | docs/spec/compliance.md |
| reporting | operational reports, analytics | docs/spec/reporting.md |
| crm | customer profile, loyalty, WhatsApp | docs/spec/crm-loyalty.md |

Tables (docs/spec/tables.md) and sync (docs/spec/sync.md) are cross-cutting rather than owning a single backend/internal directory: tables belong conceptually to outlet+ordering, sync is implemented in edge/sync + backend outbox workers.

## Canonical Order (see docs/spec/ordering.md, packages/contracts/)
```
CanonicalOrder {
  holler_order_id, external_order_id, source, outlet_id,
  customer, delivery_address,
  items[], modifiers[],
  subtotal, discount, packaging, delivery_charge, taxes,
  aggregator_discount, merchant_discount, total,
  payment_status, payment_source,
  preparation_time, rider,
  timestamps, source_payload
}
```
Every order-producing channel (POS, QR, aggregator, direct) normalizes into this one shape.

## Inventory model summary
```
Raw Material → Semi-Finished Product → Recipe → Menu Item
```
Stock is derived from an immutable ledger (PURCHASE/CONSUMPTION/WASTAGE/TRANSFER/ADJUSTMENT/PRODUCTION_*), never overwritten directly. See docs/domain/INVENTORY_MODEL.md for detail.

## Identifiers & money (see CLAUDE.md)
Internal IDs: UUIDv7/ULID. Human-facing: short sequential-looking numbers (Order #A184) that are never used as security identifiers. Money: integer paise, never floating point.
