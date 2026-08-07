# ADR-007: Transactional Outbox for Event Publishing

## Context
Business events (OrderCreated, KOTCreated, PaymentReceived, StockConsumed, etc.) must never be lost or published inconsistently with the database transaction that produced them. A naive `commit → then publish` sequence risks publishing an event for a transaction that later fails, or committing a transaction whose event publish then fails silently.

## Decision
Every state change that produces a business event writes the event to an **outbox table in the same database transaction** as the state change (both edge SQLite and cloud PostgreSQL). A separate outbox worker reads unpublished rows and publishes them to NATS JetStream (cloud) or the local sync/outbox pipeline (edge), marking them published only after a confirmed send.

## Alternatives
- **Direct publish inside the request handler after commit**: rejected — a crash or failure between commit and publish silently drops the event, violating the zero-lost-orders principle (§2.3).
- **Dual-write to DB and message bus without a shared transaction**: rejected — no atomicity guarantee; the two can diverge under partial failure.

## Consequences
- Every module that emits business events needs an outbox table and a publishing worker, not just a direct bus client.
- Publishing becomes at-least-once; all consumers (aggregator handlers, sync workers, projections) must be idempotent.
- This is the same mechanism underlying both the local edge outbox (docs/spec/sync.md) and cloud event publishing — one pattern, two deployments.
