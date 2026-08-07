# Sync Protocol

Source: docs/spec/sync.md, HOLLER_MASTER_PROMPT.md §49–§51, §50.1, ADR-007, ADR-009.

## Record envelope
Every locally created record carries: `id` (UUIDv7/ULID, sortable), `tenant_id`, `outlet_id`, `device_id`, `created_at`, `updated_at`, `version`, `sync_status`.

## Pipeline
```
local operation → SQLite transaction (edge) → local outbox (same txn) → sync worker
→ cloud API → cloud transaction (+ cloud outbox) → ack → mark synchronized
```
Resumable at every stage: a crash between any two steps leaves the outbox row unpublished/unacked, and the worker retries. Local transactions are never deleted immediately after sync — they remain for audit/replay.

## §50.1 Authority rule (frozen — do not redesign)
- **Cloud → Edge (down), versioned, replace-not-merge**: catalog/config — menu, price books, tax profiles, users, roles, outlet settings. The edge always applies the latest *authorized* version; there is no local edit path for these that isn't itself an authorized cloud-issued version.
- **Edge → Cloud (up), append-only, replay-not-merge**: operational transactions — orders, KOTs, payments, shifts, stock movements. The cloud never rewrites or merges an edge-originated row; it replays the sequence of events that produced it.
- No CRDTs, no bidirectional merge machinery. This split, plus the per-aggregate table below, is the complete conflict-resolution design — future agents must not introduce merge logic beyond this.

## Per-aggregate conflict policy
| Aggregate | Policy | Direction |
|---|---|---|
| Menu / price books / tax profiles / users / roles / outlet settings | version-based, latest authorized wins | cloud → edge |
| Orders / KOTs | state machine + command validation (see ORDER_STATE_MACHINE.md) | edge → cloud |
| Payments / financial transactions | append-only; never last-write-wins | edge → cloud |
| Inventory transactions | append-only ledger | edge → cloud |
| Availability (stock-out/snooze) | latest authorized version | edge ↔ cloud, event-driven |

## Event model
Immutable business events (OrderCreated, ItemAdded, ItemRemoved, KOTCreated, OrderAccepted, OrderPrepared, OrderReady, InvoiceCreated, PaymentReceived, PaymentRefunded, StockConsumed, StockAdjusted, PurchaseReceived, AggregatorOrderReceived, SettlementReceived) are published exclusively via the transactional outbox (ADR-007) — never a bare post-commit publish. Payload schemas are defined once in `packages/contracts/` and shared by Go, TypeScript, and Rust representations (contract-drift tested, ADR-008).

## Idempotency requirement
All externally-triggered writes — Swiggy/Zomato order webhooks, Razorpay payment/refund webhooks, settlement imports, aggregator menu-sync requests — must be safe under duplicate delivery: a repeated webhook must never create a duplicate order, payment, KOT, stock deduction, or refund. Achieved via idempotency keys stored alongside the external event id on first processing.
