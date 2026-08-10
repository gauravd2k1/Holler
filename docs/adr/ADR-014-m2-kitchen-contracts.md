# ADR-014 — Milestone 2 kitchen contracts (stations, printers, KOT lifecycle)

**Status:** Accepted
**Date:** 2026-08-10
**Contracts version:** 0.2.5 → 0.3.0
**Supersedes:** nothing. **Amends:** ADR-008 (contracts frozen), ADR-009 (§50.1 authority split).

## Context

Milestone 2 delivers KOT, station routing, printer abstraction, KDS, LAN realtime delivery and order status (§81). Milestone 0.5 froze the `Kot` shape and the `KOTCreated` event, but nothing else the kitchen needs:

- There was no concept of a **station** anywhere in the contracts. `kot.station` was a free `TEXT` column with a comment listing example values, and no table said which menu item routes where. Station routing — the deliverable — had no data to route on.
- There was no **printer** either. `docs/spec/hardware-printing.md` specifies per-station printer routing, a spool, retry and staff-visible failures; none of that had a shape.
- `KOTCreated` was frozen and unroutable: no ingest route accepted it. So was `ItemRemoved`, which `edge/database` has been emitting since 0.2.3 — a removal could be written to the outbox and could never arrive.
- A KOT had a `status` column and no way for a transition to reach the cloud. Reporting could see that a ticket existed and never that the kitchen worked it.

## Decision

### 1. Stations and printers are config; tickets and spool entries are not

`station`, `menu_item_station`, `printer` and `station_printer` are `CLOUD_TO_EDGE` config aggregates versioned by `config_version` and replaced wholesale at the edge. `kot` stays `EDGE_TO_CLOUD`.

This is the ADR-011 rule applied to the kitchen: a table's *definition* is config and its *live state* is an edge-authoritative aggregate. A station's definition is a management decision; the ticket at that station is a shop-floor transaction. No row is half-config, half-transaction.

`kot.station` stores the station's `code`, not its `station_id`. A ticket already on the pass — or sitting in an undrained outbox — stays readable after a station is renamed or deleted.

`code` is unique per `(outlet_id, code)` and `printer.name` per `(outlet_id, name)`. Never global: two outlets both having a `TANDOOR` is the normal case.

### 2. Item→station routing is a join table, not a column

An item may route to more than one station (`docs/spec/kitchen.md §Stations`) — a thali hits `MAIN_KITCHEN` and `TANDOOR` and must print at both. A `station_id` column on `menu_item` would have made that unrepresentable. Same for `station_printer`, which is many-to-many in both directions.

Both routing routes are `PUT` (replace), not `POST` (append): config is replaced rather than merged at the edge, and an append-only route would leave no way to stop an item printing at a station it no longer belongs to.

### 3. `print_job` is edge-local and deliberately not an `AggregateType`

The spool never crosses a boundary. It is a fact about one outlet's paper and one printer's socket; the cloud has no use for it and no authority over it. Listing it in `AggregateType` would promise a sync direction and invite a replay path that must not exist.

This mirrors the `refresh_token` precedent from 0.2.1 — cloud-only, deliberately excluded from `AggregateType` for the same reason, from the other side of the boundary.

It lives in `packages/contracts/sqlite/` anyway, because that directory is the single source of the edge schema; splitting it would leave the edge with two migration sources to keep in step. It has a TypeScript and Go type because the POS reads it across the Tauri boundary to show staff a failed print, which `hardware-printing.md` requires.

A `UNIQUE (kot_id, printer_id)` index makes a duplicate spool entry unrepresentable. `hardware-printing.md` requires that a late printer ack never cause a duplicate KOT; enforcing that in the schema is cheaper than trusting every retry path to remember.

`kot_status_history` is edge-local for a related reason: its transitions reach the cloud as `KOTStatusChanged` events replayed onto the existing `kot` row, not as a mirrored table. One authority for KOT state, one path to it.

### 4. `KOTStatusChanged`, and one writer for `kot.status`

New frozen event, spelled `KOT-` to match its sibling `KOTCreated`. It carries `changed_at` as **the moment the edge recorded**, not the moment the cloud received: kitchen timing analytics are the point, and an outlet syncing hourly would otherwise report every ticket as prepared in the same instant.

`POST /kots/{kotId}/status` is the only route that writes `kot.status`, and it only ever replays. No cloud-side handler may transition a ticket. This is a verifier checklist item for the Milestone 2 backend track, not just a comment.

### 5. Routability fixes

`POST /orders/{id}/kots` gives `KOTCreated` an ingest route. `DELETE /orders/{id}/items/{itemId}` gives `ItemRemoved` one, closing the item filed in `docs/backlog-m2.md` under **Contracts** ("`ItemRemoved` is unroutable").

That backlog entry also named the real limitation it exposed: `scripts/check-event-type-drift.mjs` verifies that an event type *appears* in Rust, not that it is *deliverable*. This ADR does not fix that — both holes were found by reading, not by the check. Cross-referencing frozen event types against OpenAPI routes remains open, and is the stronger argument for generating a Rust binding rather than grepping for one.

### 6. KDS LAN messages are frozen, but are not sync

`src/types/lan.ts` defines `KdsLanMessage` (edge→KDS) and `KdsLanCommand` (KDS→edge). Nothing here carries a `SyncEnvelope` or touches `AGGREGATE_AUTHORITY` — it is one hop across the outlet LAN, specified so a ticket lands on the pass inside the 250ms target.

It is frozen in contracts because that hop has two implementations in two languages (Rust in `edge/device`, TypeScript in `apps/kds`) and nothing else would keep them agreeing. There is no Rust binding, so `check-event-type-drift.mjs` now scans `edge/device` and `edge/printer` too.

Both message types carry whole objects rather than deltas — a snapshot on connect, a full `Kot` on upsert — so a screen that missed a message still converges. A KDS sends *intent*; the edge validates and answers. The screen never becomes a second writer.

## Consequences

**This bump is not purely additive**, which is why it is 0.3.0 and not 0.2.6:

1. `/sync/config` grows four entries in its `required` list. The edge sync worker is its only consumer and is updated in the same milestone. A tolerated-absence field would have hidden a half-configured kitchen until the first ticket failed to route.
2. `Kot.status` moved from an inline enum to a `KotStatus` `$ref`. Same values, one definition.
3. `preparation_time_minutes` gains a column in both stores, per the Milestone 2 deferral noted in `sqlite/0004` and `postgres/0005`. It was synthesized as `NULL` and pinned by the order-level round-trip test; that pin now moves to the column.

New Postgres tables carry **no `DEFAULT gen_random_uuid()`**, departing from `0001_init.sql` and following `0002`. §74 requires app-generated UUIDv7; a DB-side default silently produces a UUIDv4 whenever a writer forgets to supply one, which is exactly what the rule exists to catch.

Round-trip fixtures exist for every new boundary-crossing table — `station`, `menu_item_station`, `printer`, `station_printer` — in both the Go and TypeScript drift suites, plus `print_job` (SQLite-only, round-tripped so the two language bindings cannot drift). Two negative tests were added: one asserting the config aggregates never become edge-authoritative, one asserting `print_job`, `kot_status_history` and `refresh_token` stay out of `AggregateType`.

## Alternatives considered

**A `station` column on `menu_item`.** Rejected: makes multi-station items unrepresentable, and `kitchen.md` calls for them explicitly.

**`print_job` as a synced aggregate.** Rejected: gives the cloud a replay path into outlet hardware state it cannot act on. Print failures are actionable only at the outlet.

**KDS writing KOT status directly to the edge database.** Rejected: makes every screen a writer of an edge-authoritative aggregate. `docs/backlog-m2.md` already flags that `ReplayTransition` treats `version <= stored` as a duplicate and silently returns current state — correct under single-writer monotonic versioning, a silent-drop risk the moment a second writer exists. Keeping KDS to intent-only avoids creating that second writer. **That backlog item remains open**: the waiter app and multi-POS outlets will still force it, and it needs its own ADR before either lands.
