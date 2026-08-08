# ADR-011 — Milestone 1 identity, RBAC and table contracts

Status: Accepted (2026-08-07)
Supersedes: nothing. Amends: ADR-003 (edge SQLite), ADR-008 (contracts freeze), ADR-009 (sync authority split).

## Context

Milestone 0.5 froze `packages/contracts/` around the order vertical slice: outlet, device, menu, order, order_item, kot, local_outbox, sync_state. Milestone 1 delivers organisation, outlet, users, RBAC and tables, none of which that freeze models. Under ADR-008 the freeze is lifted only by the orchestrator/architect session, serialized, with a version bump and this note.

## Decision

Contracts move `0.1.0 → 0.2.0`. The change is additive: `postgres/0002_m1_identity_tables.sql` and `sqlite/0002_m1_identity_tables.sql` add tables, and no column of any 0001 table is altered. The one edit to a frozen file is the `AggregateType` enum and `AGGREGATE_AUTHORITY` map in `src/types/sync.ts` / `go/sync.go`.

### 1. Users, roles and permissions are configuration

Under the §50.1 authority rule, `app_user`, `role` and `restaurant_table` are catalog/config: the cloud owns them, they sync down versioned, and the edge replaces them wholesale rather than merging. This follows the rule as written in docs/spec/sync.md, which names users and roles as config explicitly.

Role assignment is scoped per outlet. `user_role.outlet_id IS NULL` means tenant-wide (Organisation Owner, Auditor). A nullable column cannot carry primary-key identity, so `user_role` has a surrogate UUIDv7 primary key and two partial unique indexes — one over `(user_id, role_id, outlet_id)` for the scoped case, one over `(user_id, role_id)` for the tenant-wide case — which together forbid duplicate assignments in both shapes.

All identifiers are application-generated UUIDv7 (§74). No table carries a `gen_random_uuid()` default, matching the 0001 order/kot tables: the database never mints an entity id.

### 2. A table's definition and a table's state are different aggregates

`restaurant_table` holds only the physical definition — section, label, seat count, active flag. It is config, cloud→edge.

`table_session` is a separate operational aggregate: one seating, holding `state`, `current_order_id`, `guest_count`, `opened_at`/`closed_at`. It is edge-authoritative, replayed edge→cloud append-only, and carries `EDGE_TO_CLOUD` in `AGGREGATE_AUTHORITY`. A partial unique index enforces at most one open session per table.

The alternative — one `restaurant_table` row carrying both definition and live state — was rejected: it would make a single row half config and half transaction, so the config sync worker could not replace the row wholesale without destroying operational state, and every writer would need per-column authority knowledge. Splitting the aggregate keeps §50.1 mechanically checkable at the envelope, which is the whole point of the rule.

`AVAILABLE` is therefore not a stored value. A table with no open session is available. `RESERVED` exists in the display-state enum because docs/spec/tables.md defines it, but nothing in Milestone 1 produces it — reservations are Milestone 9.

### 3. Credential material: cached at the edge, absent from the wire and from audit

The Milestone 1 acceptance criterion is that a cashier can work with the internet disconnected, which includes logging in. Offline authentication requires the Argon2id password hash to be present on the POS device, so `sqlite/0002` caches `password_hash` and `pin_hash` in `app_user`.

Two constraints follow, and both are binding on builder agents:

- **Encryption at rest.** The edge database file now holds credential material and falls under the edge encryption-at-rest requirement. `edge/database` must open the file encrypted and must never copy it, or any backup of it, to an unencrypted location. This amends ADR-003, which specified WAL mode but no at-rest requirement.
- **Never on the wire, never in audit.** Hashes are returned by exactly one endpoint, `GET /sync/config`, over TLS, to an enrolled edge node (`EdgeUserCacheEntry`). The `AppUser` wire shape has no hash fields at all. `AUDIT_REDACTED_FIELDS` / `AuditRedactedFields` name `password_hash` and `pin_hash`; the audit helper in each runtime strips those keys from `old_value`/`new_value` before a row is written, so a user-update audit entry can never carry a hash as its "old value". Drift tests assert the two lists match and that no wire fixture contains either field.

## Consequences

- `AggregateType` gains `table_session` (EDGE_TO_CLOUD) and `app_user`, `role`, `restaurant_table` (CLOUD_TO_EDGE). Every aggregate has exactly one direction; a drift test asserts the map is total.
- New TS+Zod and mirrored Go types: `identity.ts`/`identity.go`, `table.ts`/`table.go`, exported from the package entrypoint. New fixtures `app_user.json`, `restaurant_table.json`, `table_session.json` with round-trip tests in both languages.
- OpenAPI `0.2.0` adds `/auth/*`, `/users`, `/users/{id}/roles`, `/roles`, `/outlets`, `/outlets/{outletId}/tables`, `/menu/categories`, `/menu/items`, `/sync/config`.
- Contracts are frozen again at `0.2.0`. Builder agents treat them as read-only; the next semantic change needs the same process and a new ADR.

---

## Addendum — 0.2.1 (2026-08-07)

Five changes, all additive, all driven by findings from the Milestone 1 build rather than by new design. Contracts move `0.2.0 → 0.2.1`.

### 1. Envelope-wrapped ingest is the single edge→cloud replay pattern

The 0.2.0 OpenAPI defined `POST /orders` as taking a raw `CanonicalOrder`. That shape cannot work. `record_id`, `device_id` and `version` are edge-owned facts, and `CanonicalOrder` carries no `version` field by design — the concurrency token's home is the envelope. A raw body therefore cannot express what a replay is.

Every mutating route for an `EDGE_TO_CLOUD` aggregate now takes a `SyncEnvelope` whose `payload` is the aggregate. Each route pins its `aggregate_type`, and §50.1 pins that aggregate's `direction`, so the server validates both against the route and returns 422 on mismatch — never coercing the envelope into the route's expected values. There is no unwrapped write route for any edge-authoritative aggregate.

Read paths stay unwrapped: a GET returns the aggregate, not an envelope.

### 2. `table_session` rides that same pattern

`table_session` already carried `EDGE_TO_CLOUD` in `AGGREGATE_AUTHORITY` from 0.2.0, but had no ingest route, so the cloud had no contracted way to receive a seating. It now has `/outlets/{outletId}/table-sessions` and `/outlets/{outletId}/table-sessions/{sessionId}`, both envelope-wrapped. Deliberately *not* a bespoke REST write route — the point of one replay pattern is that adding an edge-authoritative aggregate later does not add a new ingest idiom.

### 3. Menu item availability endpoint

`POST /menu/items/{itemId}/availability`, body `{available, reason?}`. Item snooze is in Milestone 1 scope and `is_available` exists in the frozen schema, but 0.2.0 exposed no way to write it. It is an ordinary catalog write: bumps `config_version`, requires `menu.manage`, audited. `additionalProperties: false` matches the `DisallowUnknownFields` decoder in `platform/httpx`, so a typo'd field is a 400 rather than a silent no-op.

### 4. `refresh_token` table

`postgres/0003_refresh_token.sql`. The auth context implemented refresh rotation against an in-process map because no table existed, which meant sessions died on restart and the design could not run on more than one instance — a Definition of Done violation, not an acceptable Milestone 1 shortcut.

Cloud-only state: refresh tokens never sync to the edge, because offline login verifies the cached Argon2id hash and issues a local session. It therefore has **no** `AggregateType` entry and never appears in a sync envelope. `token_hash` stores a SHA-256 — the token itself is never persisted. Its uniqueness is global rather than tenant-scoped, deliberately: a token must be unique across all tenants, since per-tenant scoping would let one secret authenticate twice.

### 5. `AuditEvent.tenant_id`, and `token_hash` redaction

The 0.2.0 `AuditEvent` type omitted `tenant_id` even though `audit_event.tenant_id` is `NOT NULL` in `postgres/0002`. That was an error in the original ADR, caught when the auth builder added the field locally and the verification pass flagged it as drift. The type now carries it, non-null.

`AUDIT_REDACTED_FIELDS` / `AuditRedactedFields` gain `token_hash`, so a `refresh_token` row can never be audited into an `audit_event` value — the same guarantee already held for `password_hash` and `pin_hash`. Both languages, asserted equal by the drift test.

New fixture `audit_event.json` with round-trip tests in Go and TypeScript, and included in the sweep asserting no wire fixture carries credential material.

---

## Addendum — 0.2.2 (2026-08-07)

Four changes closing gaps the edge implementation exposed.

### 1. The four missing event types are now frozen

`events.ts` and `events.go` defined only `OrderCreated`, `ItemAdded`, `KOTCreated` and `OrderReady`. The sync worker also had to replay send-to-kitchen, cancellation and table seatings, so it coined `SentToKitchen`, `OrderCancelled`, `TableSessionOpened` and `TableSessionUpdated` as Rust string literals and documented them as pending a contracts revision. That was a de-facto unfrozen contract: the cloud must match those strings exactly to interoperate, and a divergence fails silently at replay — no compile error, no test failure, just orders that never arrive.

All four are now frozen as `EventEnvelope` schemas using the exact strings already in use, so freezing required no edge change. `OUTBOX_EVENT_TYPES` / `OutboxEventTypes` carry the authoritative list in both languages, asserted identical by a drift test.

### 2. Rust has no binding — a bidirectional grep stands in

TypeScript and Go are bound to the contract and drift-tested against each other. The Rust crates (`edge/sync`, `apps/pos/src-tauri`) are not: they hold event types as bare literals with no compile-time link. A generated Rust binding under `packages/contracts/rust/` is the real answer, and is **deferred until a fourth Rust consumer** justifies the generation step.

Until then, `scripts/check-event-type-drift.mjs` runs in CI over both crates in **both directions**: forward, every event-type-shaped literal in Rust must exist in the frozen list, catching an invented or misspelled string; backward, every frozen type must appear in Rust or sit in `NOT_YET_EMITTED` with a stated reason, catching a contract addition the edge silently never adopted.

The check earned itself immediately: it found that `edge/sync/src/route.rs` handles `TableSessionOpened` explicitly but routes `TableSessionUpdated` through a `("table_session", _)` wildcard. Behaviourally that works, but the literal appears nowhere, so a POS emitting a misspelled table-session event would be silently accepted by the wildcard and replayed to the wrong shape instead of erroring. Unknown event types must be an error, not a default branch.

### 3. Menu types

`menu.ts` and `menu.go` add `MenuCategory`, `MenuItem`, `MenuItemVariant` and `MenuItemModifier`, matching the frozen SQLite and Postgres columns field-for-field, with fixtures and round-trip tests. Without them the POS frontend would hand-roll menu shapes in TypeScript against raw SQLite column names — the drift that already produced the `AppUser`/`Role` divergences, except across the Rust↔TypeScript boundary where no drift test reaches.

### 4. `EdgeUserCacheEntry.updated_at` added; `/sync/config` `roles` removed

`app_user.updated_at` is `NOT NULL` in the SQLite schema but the endpoint never supplied it, so the sync worker synthesized a wall-clock value on every pull. Harmless — `config_version` drives replace-or-ignore, not `updated_at` — but the endpoint should supply what the schema demands.

`roles` is **removed** from the `/sync/config` bundle. This is the only non-additive change in the bump. The edge has no `role` table by design: permissions arrive pre-flattened on each user entry, so the field promised storage that does not exist and the sync worker parsed then discarded it. Removing it beats adding a table nothing reads.

---

## Addendum — 0.2.3 (2026-08-07)

Two changes, both closing gaps where the contract promised more than the schema could hold. Fully additive.

### 1. `order_item_modifier` — the wire promised fidelity the storage could not deliver

`OrderItem` has carried a `modifiers` array since 0.1.0, and `fixtures/order.json` round-trips a real modifier through both languages. No table ever held them. So every `ItemAdded` event replayed an empty modifier list, and an order for "Large / Cheese Burst / extra paneer" — the example in docs/spec/menu.md itself — arrived at the cloud stripped of its selections. Every drift test passed throughout, because they check shape, not storage.

`sqlite/0003` and `postgres/0004` add a typed `order_item_modifier` table on both sides. **Typed table, not a `modifiers_json` column**: these rows carry money (`price_delta_paise`) and must survive replay with the same fidelity as the line itself, which is exactly what ADR-008's typed-tables-over-JSONB rule is for.

`modifier_id` is deliberately **not** a foreign key to `menu_item_modifier`. The catalog is config and gets replaced wholesale at a newer `config_version` (§50.1); a completed order's snapshot must never move because the menu changed underneath it. `group_name`, `option_name` and `price_delta_paise` are snapshotted for the same reason.

The money invariant is stated in both migrations so the edge recompute path and the cloud replay cannot diverge:

    unit_price_paise = snapshot of menu_item.base_price_paise + variant delta
    line_total_paise = (unit_price_paise + SUM(price_delta_paise)) * quantity

### 2. `ItemRemoved`

docs/spec/sync.md §Event model always listed it; it was simply never frozen in `events.ts`. The consequence was asymmetry: `edge/database` hardened its add path to derive the outbox payload from the row it wrote, but could not do the same for removal because no frozen event type existed, leaving removal caller-described — a caller could delete a line and emit a misleading record, with the local row already gone.

The payload carries the **full item**, not just an id: once the row is deleted the cloud cannot look up what left the order, so the event has to be self-describing.

### Practice note — verify storage, not only shape

This bump exists because round-trip drift tests proved the *shape* was consistent across languages while nothing proved the data could be *stored*. Contract fixtures round-tripped modifiers for three milestones over a schema that had nowhere to put them.

**For future contract freezes: pair each shape round-trip with a persistence round-trip** — wire fixture → store → read back → re-serialize → byte-compare against the fixture. A shape test proves the languages agree; a persistence test proves the database can actually hold what they agreed on. The `edge/database` follow-up for this bump adds that test for `order_item` and its modifiers as the reference implementation.

---

## Addendum — 0.2.4 (2026-08-07)

The practice note above paid off immediately. Writing the `order_item` persistence round-trip surfaced the same class of gap one level up: **a full `CanonicalOrder` could not round-trip through the `order` table.** The wire type carries 25 fields; the table had 14 columns.

### The gap

Missing on **both** SQLite and Postgres: `external_order_id`, `source`, `customer`, `delivery_address`, `packaging_paise`, `delivery_charge_paise`, `aggregator_discount_paise`, `merchant_discount_paise`, `payment_status`, `payment_source`, `preparation_time_minutes`, `rider`, `confirmed_at`, `schema_version`. SQLite additionally lacked `source_payload`. And the wire said `taxes_paise` while both schemas said `tax_paise`.

Fields with no column were synthesized at serialization time. For Milestone 1 that mostly happened to be harmless — POS orders are `source: POS`, `payment_status: UNPAID`, no delivery, no aggregator — but `confirmed_at` was genuinely lost on every replay despite the DRAFT→CONFIRMED transition being implemented, and the canonical model was fictional wherever no column backed it.

### Decision

`sqlite/0004` and `postgres/0005` add the Milestone 1–3 lifecycle fields: `source`, `external_order_id`, `payment_status`, `payment_source`, `confirmed_at`, `schema_version`, plus `source_payload_json` on the edge.

**`tax_paise` is renamed to `taxes_paise` on both sides.** This is the one non-additive change. It is accepted deliberately: Milestone 3 builds a tax engine on this column, and a silent name mismatch between contract and storage feeding financial calculation is a worse failure than a coordinated rename while there is no production data. The rename ships as one sequence — contracts, then edge, then backend — never as a contracts commit left sitting alone, because every consumer breaks in between.

### Deferred columns and their landing milestones

These wire fields stay unpersisted for now. Each is synthesized at a fixed value, and the order-level persistence round-trip test **pins those exact values** rather than merely asserting the fields are absent — so when a milestone starts persisting one, the test fails and forces this table to be updated instead of drifting quietly.

| Wire field | Synthesized as | Lands in |
|---|---|---|
| `packaging_paise` | `0` | Milestone 6 — aggregator orders carry it |
| `delivery_charge_paise` | `0` | Milestone 6 |
| `aggregator_discount_paise` | `0` | Milestone 6 |
| `merchant_discount_paise` | `0` | Milestone 6 |
| `customer` (`{name, phone}`) | `null` | Milestone 6 (aggregator payloads), enriched Milestone 9 |
| `delivery_address` | `null` | Milestone 6 |
| `rider` (`{name, phone, status}`) | `null` | Milestone 6 |
| `preparation_time_minutes` | `null` | Milestone 2 — the kitchen owns prep time |

The three `NOT NULL` columns carry `DEFAULT`s only because SQLite requires a non-null default when adding `NOT NULL` to an existing table. They are a migration mechanism, not an application fallback: writers set `source`, `payment_status` and `schema_version` explicitly.

---

## Addendum — 0.2.5 (2026-08-07)

0.2.4 gave `confirmed_at` a column on both sides. Adapting the edge and the backend then showed that **nothing could stamp it**: no crate has a DRAFT→CONFIRMED command, and cloud-side `transition()` calls `UpdateStatus`, which sets only status, version and `updated_at`. A column no production path writes is the same fictional-field problem 0.2.4 existed to fix, one level down — so the gap was ruled a Milestone 1 blocker rather than backlog.

### `OrderConfirmed`, not `OrderAccepted`

`docs/spec/sync.md` §Event model lists `OrderAccepted`, and reusing that name was considered and rejected. Cashier confirmation of a draft and a merchant accepting an inbound aggregator order are **different business events**: they have different actors, different preconditions and different side effects. When Milestone 6 lands, `AcceptOrder` on the `AggregatorProvider` interface gets its own event rather than sharing this one. Sharing a name would have forced every future consumer to disambiguate by context — the sort of ambiguity that reads as harmless at freeze time and becomes unfixable once replayed events exist in production.

The payload carries `confirmed_at` explicitly, and the contract states it is **the moment the edge recorded, not the moment the cloud received it**. The edge is authoritative for order transactions (§50.1); a cloud that stamped its own clock would be inventing a fact about the shop floor.

### `POST /orders/{id}/confirm`

The generic transition routes carry no payload, so there was no way to replay a timestamp. Rather than widen those routes — which would let a payload reach transitions that must not carry one — 0.2.5 adds a dedicated envelope-wrapped ingest route with `OrderConfirmEnvelope`, whose payload is `{confirmed_at}` and nothing else (`additionalProperties: false`). `409` is returned for an order not in `DRAFT`, so an illegal transition is rejected rather than applied.

### Sequencing

`OrderConfirmed` sits in the drift check's `NOT_YET_EMITTED` with a stated reason until the edge emits it, so CI stays green across the gap; that entry is deleted by the commit that starts emitting it. The work spans five parts — contracts, `edge/database` (transition, stamp and event in one transaction), the Tauri command, the POS wiring, and the backend ingest route — because a timestamp the edge stamps but the cloud cannot receive would be no better than the column that nothing stamped.
