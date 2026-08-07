# ADR-005: NATS JetStream for Event/Message System

## Context
Holler needs durable, at-least-once event delivery for business events (OrderCreated, KOTCreated, PaymentReceived, etc.) and for aggregator/webhook fan-out, without the operational weight of a system sized for internet-scale streaming.

## Decision
Use **NATS JetStream** as the event/message backbone for the cloud backend, paired with the transactional outbox pattern (ADR-007). Do not begin with Kafka.

## Alternatives
- **Kafka**: rejected for the current stage — heavier to operate (ZooKeeper/KRaft, partitioning, ops tooling) than the current scale justifies; revisit only when actual throughput demands it.
- **Plain PostgreSQL LISTEN/NOTIFY or polling**: rejected as the sole mechanism — insufficiently durable/replayable for financial event history at scale, though outbox rows themselves live in Postgres.
- **Redis Streams**: rejected — JetStream gives comparable simplicity with stronger durability/replay guarantees suited to financial events.

## Consequences
- Event payload schemas are defined once in `packages/contracts/` and consumed identically by publishers and subscribers.
- JetStream persistence + outbox pattern together give at-least-once delivery; consumers (including aggregator/payment webhook handlers) must be idempotent (see docs/spec/sync.md).
- Revisit this decision only with concrete evidence NATS JetStream throughput/retention is insufficient — not preemptively.
