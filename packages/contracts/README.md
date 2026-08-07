# packages/contracts

Source of truth for all cross-boundary shapes: TypeScript+Zod, mirrored Go structs, OpenAPI, SQLite/PostgreSQL schema, and JSON fixtures. See ADR-008.

**Frozen as of Milestone 0.5.** Builder agents treat this directory as strictly read-only. Only the orchestrator/architect session edits it, and every semantic change increments a version and gets an ADR note (ADR-008).

## Layout
- `sqlite/0001_init.sql` — edge SQLite schema (outlet, device, menu_category, menu_item, menu_item_variant, menu_item_modifier, order, order_item, kot, local_outbox, sync_state).
- `postgres/0001_init.sql` — cloud PostgreSQL schema mirroring the same vertical slice (tenant/brand/outlet/menu/order).
- `src/types/*.ts` — Zod schemas + inferred TS types: `CanonicalOrder`, `OrderCommand`, `Kot`, outbox events (`OrderCreated`, `ItemAdded`, `KOTCreated`, `OrderReady`), `SyncEnvelope`.
- `go/*.go` — hand-mirrored Go structs for the same shapes, in package `contracts`.
- `openapi/openapi.yaml` — the §70 order endpoints needed for the vertical slice.
- `fixtures/*.json` — canonical example payloads used by both `go/drift_test.go` and `src/types/drift.test.ts` to prove Go/TypeScript representations round-trip identically. A Rust representation and its drift check are added when `edge/` implementation begins.

## Why typed tables, not JSONB, for variants/modifiers
See the ADR-008 amendment: variants and modifiers are core relational entities (price deltas, min/max selection, recipe/inventory implications) and get typed columns/constraints; JSONB is reserved for external payloads and denormalized ticket snapshots only.
