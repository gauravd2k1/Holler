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

## Table-side ordering (customer tab) — FUTURE SCOPE, PROPOSED FOR M8

**Nothing in this section is built, and none of it is M6 work.** It is recorded
here so the shape is settled before anything is written, per **ADR-025
(PROPOSED)**, whose landing milestone is **M8** with the trigger *after the
first pilot runs on `STAFF_ONLY`*. One item is pulled forward — the
`order.source` widening — because it travels with a contract change already
pending for 0.8.1.

**Product intent.** Some outlets will place a tab on each dining table so
customers see live availability and order directly; other outlets stay
staff-ordered. **Both modes coexist across outlets and must coexist within one
outlet** — not every table gets a tab.

1. **The tab is a LAN client of the till, the same shape as the KDS.** It talks
   to the edge over the existing LAN transport (ADR-015) and never to the cloud.
   Orders are edge-authoritative (§50.1) and table ordering must work with the
   uplink down; a tab that needs the cloud is the opposite of the product. A web
   app served from the till's LAN server, enrolled as a device, in `apps/table`.

2. **`table_device` is a new principal kind, not a cashier account**, with a
   permission set structurally narrower than any staff role: read menu and
   availability for its outlet; create and append items to the order bound to
   ITS table only; nothing else — no billing, no void, no discount, no other
   table's data. **Enforced at the LAN boundary, not by the tab's UI.** This
   falls under the M2 LAN socket security gate and needs that gate reviewed
   before any customer-facing device is enrolled: a threat model for a device a
   member of the public holds, rate limiting, and a per-table binding that
   cannot be re-pointed from the tab itself.

3. **Ordering mode is cloud config that syncs down.** Per outlet,
   `ordering_mode ∈ {STAFF_ONLY, TABLE_TAB}`; per table, `tab_enabled`. Set in
   `apps/admin`, delivered by the config pull that now rides the A5 loop. **The
   edge refuses tab enrolment for a table that is not enabled.**

4. **Order source.** Widen `order.source` to name `TABLE_TAB` in the same
   additive change that names the platform for aggregator orders (already
   pending, 0.8.1): one CHECK widening, two values, one ADR note. Reports must
   distinguish staff-entered, tab-entered and aggregator orders.

5. **Table binding rides `table_session`**, which already exists and already
   syncs. A tab binds to a table; its orders attach to that table's open
   session. **No new aggregate.**

6. **Append-only from the tab.** A tab may add items to its table's order; it
   may not modify or remove them. Corrections go through staff on the till. This
   sidesteps tab-versus-waiter concurrent edits — per-aggregate ordering already
   exists, and append-only makes it sufficient.

7. **Availability is edge state and is answered by the edge.** The till already
   computes stock-out and low-stock; expose that as a LAN read. When an item goes
   unavailable between the tab showing it and the till receiving the order, **the
   till rejects that item with a typed reason the tab renders** — never silently
   accepted, never silently dropped. Same discipline as `missing_reference`.

8. **Payment stays on the till for the first version.** Tab orders, staff bills.
   Customer-side payment is a separate decision with its own gate.

9. **Prerequisites, in order:** M6 **A7** (the `kot` routes — a tab-originated
   order must reach the kitchen and replay), the `order.source` widening, the LAN
   security gate review, and a pilot of `STAFF_ONLY` mode in at least one outlet.
