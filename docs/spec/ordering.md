# Spec: Ordering

Owns: order creation/lifecycle, POS interaction model, canonical order model, order state machine.
Source: HOLLER_MASTER_PROMPT.md §7, §8, §16, §52.

## Order types
Dine In, Takeaway, Delivery, Aggregator, QR Order, Room Service, Catering. Creation requires minimal interactions; a trained cashier operates from muscle memory (search, barcode/PLU, favorites, recent items, quick keys).

## POS layout
LEFT categories · CENTER menu grid · RIGHT cart/order · TOP search/order-type/customer/table · BOTTOM subtotal/tax/discount/payment/hold/send-KOT.

## Canonical order model (versioned in packages/contracts/)
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
Every channel (POS, QR, aggregator, direct) normalizes into this shape. Store raw external payload for audit.

## Order state machine
```
DRAFT → CONFIRMED → SENT_TO_KITCHEN → PREPARING → READY
→ SERVED → BILLED → PAID → CLOSED
Alternative: CANCELLED
```
Illegal transitions (e.g. CLOSED → DRAFT) must be rejected at the command layer, not just the UI.

## Conflict policy
Order: state machine + command validation (see docs/spec/sync.md §51 policy table). Financial line items: append-only.

## Cross-context dependencies
- Menu (docs/spec/menu.md) for items/prices/modifiers.
- Tables (docs/spec/tables.md) for dine-in association.
- Kitchen (docs/spec/kitchen.md) for KOT generation on send-to-kitchen.
- Payments (docs/spec/payments.md) for BILLED→PAID.
- Sync (docs/spec/sync.md) for local↔cloud propagation; edge is authoritative for order transactions (§50.1).
