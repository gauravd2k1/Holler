# ADR-027 — `GET /orders` documented, and the cloud made to honour the order wire type

**Status:** ACCEPTED
**Date:** 2026-09-12
**Contracts:** v0.8.1 → **v0.8.2** (documentation-only; no schema, no field, no store changed)
**Supersedes nothing. Amends:** ADR-024 (the M6 Phase B admin routes).

## Context

Demo step 5 shows the back office. The scenario board recorded S-ADM-08 as
blocked on two things: the admin console has no orders screen, and *"no cloud
read route to build one on — `/orders` is POST-only ingest"*.

**The second half was wrong.** `backend/internal/ordering/http.go:32` has routed
`GET /orders` since Milestone 1 — `listOrders` → `Service.ListOrders` →
`PostgresRepository.ListByOutlet` — and has served it in every deployment since.
What was missing was its entry in `packages/contracts/openapi/openapi.yaml`,
which is the file the survey read. A route absent from the spec reads as a route
that does not exist, and that mis-reading was about to buy a day of work and a
contracts bump for something already shipped.

The operator's ruling was: a `GET /orders` returning the **existing order wire
type unchanged** is additive and may land as 0.8.2; if it needs a **new wire
type**, do not start.

## Decision

**1. Document the route exactly as it behaves. Change nothing about it.**

The response is a bare JSON array of `CanonicalOrder` — the same shape
`GET /orders/{id}` already returns, unwrapped. It is *not* given the
`GoodsReceiptPage` envelope, because adding pagination while claiming to
document existing behaviour would be a behaviour change wearing a documentation
label, and the ruling's own condition was "unchanged".

Two properties are written into the spec rather than fixed, so a caller plans
around them and a reader is not surprised:

- **Unbounded.** No `limit`, no `cursor`: every order the outlet has ever
  replayed arrives in one response.
- **Oldest first.** `ORDER BY created_at` ascending. The admin screen sorts
  newest-first over the response.

Both, plus the N+1 in `ListByOutlet` (list the ids, then re-query each order,
then each order's items), are filed in `docs/backlog.md` with the trigger
*before the first pilot*. Changing a Milestone 1 read path's shape and ordering
during a demo build is not a documentation task.

**2. The cloud must actually serve the wire type it claims.** Two defects were
found by pointing a real TypeScript client at the route, and both are fixed
here because without them the documented shape is a fiction:

- **`display_number` was in neither the INSERT nor the SELECT.** `grep
  display_number backend/internal/` returned nothing at all. The edge mints the
  number, `CanonicalOrder` carries it, both stores have the column — and the
  cloud discarded it on ingest and served `null` forever after. "Order #A184"
  is the only name a human has for an order.
- **Timestamps were serialised with a `+05:30` offset**, because pgx returns a
  `timestamptz` in the connection's session timezone and Go marshals RFC3339
  with whatever offset it is handed. `CanonicalOrderSchema` types these as
  `z.string().datetime()`, which accepts `Z` and **rejects an offset**. Every
  order the cloud served failed client-side validation.

Neither is a contract change: the column, the field and the type already exist.
The cloud simply did not honour them.

## Why this was not caught

**Nothing had ever read an order back from the cloud in TypeScript.** The till
reads the edge; the KDS reads the edge; the admin console had no orders screen.
The only TypeScript that had ever parsed a `CanonicalOrder` was the contract's
own drift test, against a **fixture authored with a `Z`** — a spelling the
server does not produce.

This is contracts 0.5.9's lesson twice over, and it is worth stating in its
general form: **a fidelity test proves fidelity only for the fields its fixture
populates and only for the spellings its fixture uses.** 0.5.9 was a field the
fixture left null; this is a field the fixture formatted differently from the
server. Both were green throughout.

## Consequences

- `packages/contracts` goes to **0.8.2**. No `.sql`, no `.ts`, no `.go` shape
  changed; only `openapi/openapi.yaml` gained a `get:` block under `/orders`.
- The regression guard is
  `TestPostgresRepository_DisplayNumberAndUtcTimestampsSurviveTheRoundTrip`,
  which asserts **the marshalled JSON bytes**, not the `time.Time`. A
  `time.Time` comparison is equal under both spellings and would have proved
  nothing. Each half was falsified separately by planting the old behaviour.
- `apps/admin` gains an Orders tab that labels itself a **replica**: an order is
  edge-authoritative (§50.1), and one rung while the uplink was down is real and
  complete at the till while absent from this list.
- **A third surface is now blocked on the same missing shape.** `OrderItem`
  carries `menu_item_id` and no `name`, so the screen counts lines rather than
  naming dishes — joining the live menu would print today's name against a line
  sold under the old one, which is what snapshotting `unit_price_paise` exists
  to prevent. `GoodsReceiptLineReadSchema` and `SupplierItemSchema` have the
  same hole. All three want one additive bump that denormalises the name onto
  the line, as `stock_ledger_entry` and `stock_count_line` already do.

## What was considered and rejected

- **Giving the list a page envelope now.** Rejected: it is the change the ruling
  excluded, and an unbounded list is a known, documented, filed property rather
  than a silent one.
- **Joining `menu_item` for a dish name.** Rejected: see above — it reintroduces
  exactly the drift the price snapshot prevents.
- **Fixing the offset in the TypeScript schema instead** (loosening
  `z.string().datetime()` to accept offsets). Rejected: the contract says UTC
  storage with the outlet timezone rendered locally, so a wire value carrying a
  server's local offset is the defect, not the schema that refuses it.
