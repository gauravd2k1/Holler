# Spec: Local ↔ Cloud Sync

Owns: sync protocol, conflict policy, event model, outbox.
Source: HOLLER_MASTER_PROMPT.md §49–§51, §50.1.

## Record envelope
Every locally created record: id, tenant_id, outlet_id, device_id, created_at, updated_at, version, sync_status. IDs are UUIDv7/ULID (sortable).

## Flow
```
local operation → SQLite transaction → local outbox → sync worker
→ cloud API → cloud transaction → ack → mark synchronized
```
Resumable; never delete local transactions immediately after sync.

## Authority rule (do not redesign)
- **Cloud is source of truth for catalog/config**: menu, price books, tax profiles, users, roles, outlet settings — sync down, versioned; edge applies latest authorized version.
- **Edge is source of truth for operational transactions**: orders, KOTs, payments, shifts, stock movements — sync up, append-only.
- Transactions are replayed, never merged. Config is versioned and replaced, never appended.
- No CRDTs, no bidirectional merge machinery — this split plus the per-aggregate policy below is the entire design.

## Conflict policy per aggregate
| Aggregate | Policy |
|---|---|
| Financial transactions | append-only; never last-write-wins |
| Menu description | version-based merge / admin resolution |
| Inventory transaction | append-only ledger |
| Availability | latest authorized version |
| Order | state machine + command validation |

## Event model
Immutable business events: OrderCreated, ItemAdded, ItemRemoved, KOTCreated, OrderAccepted, OrderPrepared, OrderReady, InvoiceCreated, PaymentReceived, PaymentRefunded, StockConsumed, StockAdjusted, PurchaseReceived, AggregatorOrderReceived, SettlementReceived. Published via **transactional outbox** — never `commit → publish` without it. All payload schemas live in `packages/contracts/`.

## Idempotency
External webhooks (Swiggy, Zomato, Razorpay, refund callbacks, settlement imports, menu sync requests) must tolerate duplicate delivery without duplicating orders/payments/KOTs/deductions/refunds.
