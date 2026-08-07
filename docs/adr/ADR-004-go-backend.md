# ADR-004: Go for the Cloud Backend

## Context
The cloud backend serves as API gateway, catalog authority, aggregator/payment integration hub, and sync counterpart to every outlet's edge node. It needs to be fast, easy to deploy as small statically-linked services, and approachable for a small engineering team to maintain long-term.

## Decision
Build the cloud backend in **Go**, against **PostgreSQL** and **Redis**, using **NATS JetStream** for the event/message system. Structure it initially as a **modular monolith** with strongly isolated bounded contexts (auth, tenant, outlet, menu, ordering, kitchen, inventory, procurement, payments, aggregators, compliance, reporting, crm) rather than microservices.

## Alternatives
- **Node.js/TypeScript backend**: rejected for the backend tier — Go's static typing, performance, and simpler deployment story (single binary) fit a latency-sensitive, integration-heavy backend better; TypeScript is retained for frontend/contracts.
- **Kafka for event streaming**: rejected at this stage — NATS JetStream is lighter to operate and sufficient until real scale demands otherwise.
- **Microservices from day one**: rejected — 40 services for architectural fashion adds operational overhead with no scale justification yet; each bounded context is built with clean internal interfaces so it *can* become a service later.

## Consequences
- One deployable backend simplifies local dev (`make dev`) and CI.
- Internal module boundaries (backend/internal/<context>) must be respected strictly so a future service split is mechanical, not a rewrite.
- NATS JetStream is the backbone for the transactional outbox (ADR-007) and aggregator event flow.
