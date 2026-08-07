# Holler System Architecture

## Topology
```
                  ┌──────────────────────────────┐
                  │        HOLLER CLOUD          │
                  │ API Gateway / Auth           │
                  │ Orders                       │
                  │ Menu                         │
                  │ Inventory                    │
                  │ Aggregators                  │
                  │ Payments                     │
                  │ Analytics                    │
                  │ CRM                          │
                  │ Multi-outlet                 │
                  │ Integrations                 │
                  │ PostgreSQL / Redis / NATS    │
                  └──────────────┬───────────────┘
                                 │
                           Secure Sync
                                 │
                      Internet available?
                          /           \
                        YES            NO
                        │               │
           ┌────────────▼────────────────────┐
           │      HOLLER EDGE NODE           │
           │ SQLite                          │
           │ Sync Engine                     │
           │ Local Event Log                 │
           │ Printer Service                 │
           │ KDS Gateway                     │
           │ Device Gateway                  │
           │ Local WebSocket Server          │
           │ LAN Discovery                   │
           └────────────┬────────────────────┘
                        │ LAN
         ┌──────────────┼───────────────┐
         │              │               │
         ▼              ▼               ▼
       POS #1          POS #2           KDS
         │                              │
         ▼                              ▼
      Cashier                       Kitchen
         │
         ├──────── Waiter devices
         ├──────── QR orders
         └──────── Printers
```

## Layers
1. **Holler Cloud** (Go modular monolith, PostgreSQL, Redis, NATS JetStream) — authoritative for tenant/catalog/config, aggregator/payment integration hub, cross-outlet analytics. See ADR-004, ADR-005, ADR-006.
2. **Holler Edge Node** (Rust, SQLite WAL) — per-outlet local server: sync engine, local event log, printer/device/KDS gateways, local WebSocket server, LAN discovery. Authoritative for operational transactions. See ADR-001, ADR-003.
3. **Clients** — POS (Tauri+React, ADR-002), KDS (web/PWA), Waiter (Flutter, ADR-010), Admin (React web), Customer Ordering (web).
4. **`packages/contracts/`** — the only shared source of truth crossing all of the above (ADR-008).

## Data flow: order lifecycle (happy path)
```
POS → local SQLite (DRAFT/CONFIRMED) → local outbox
    → KOT Router (edge) → station KDS (LAN, <250ms)
    → sync worker → Cloud API → Postgres (durable copy)
```
Aggregator orders enter via cloud webhook → aggregator_gateway → normalized into CanonicalOrder → pushed down through outlet sync → edge → KOT router → KDS. See docs/spec/aggregators.md §Event flow.

## Sync authority (see ADR-009 / docs/spec/sync.md)
Cloud owns catalog/config (menu, prices, tax, users, roles, outlet settings) — pushed down, versioned, replace-not-merge.
Edge owns operational transactions (orders, KOTs, payments, shifts, stock) — pushed up, append-only, replay-not-merge.

## Cross-cutting concerns
- **Contracts** (`packages/contracts/`): frozen after Milestone 0.5, read-only to builder agents.
- **Observability**: OpenTelemetry across cloud and edge; metrics include orders/minute, KOT latency, sync delay, aggregator/payment/printer failures (§55).
- **Security**: OWASP baseline, tenant isolation by `tenant_id` scoping (ADR-006), RBAC (docs/spec/security-rbac.md).
- **Outbox**: every business-event-producing write uses the transactional outbox (ADR-007), both in cloud Postgres and edge SQLite.

## Deployment (initial)
AWS with portable containers: CloudFront, ALB, ECS/Fargate, RDS PostgreSQL, ElastiCache, S3, CloudWatch/OpenTelemetry. Kubernetes only when justified. Terraform for infra. Local dev: Docker Compose for Postgres/Redis/NATS/backend — WSL2 is one convenient host for that and is not required (Hyper-V or a remote database work equally well); Tauri/Rust Windows builds on the Windows side.

**Outlet runtime is a different world entirely (ADR-013):** a restaurant machine runs bare Windows 10 with no WSL, no Docker and no database server — one native POS executable over a statically-linked SQLite file, syncing outbound over HTTPS. Nothing in this cloud tooling section applies to it.
