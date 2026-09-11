# ADR-026 — `order.source` names the aggregator channel and the table device

**Status:** ACCEPTED, 2026-09-11
**Contracts:** 0.8.0 → **0.8.1** (additive)
**Supersedes nothing. Amends:** ADR-011 (0.2.4, which wrote the original CHECK), ADR-022 (aggregator authority), ADR-025 (table ordering device, PROPOSED)

## Context

`order.source` has carried a closed CHECK since contracts 0004: `POS`, `QR`,
`AGGREGATOR_ZOMATO`, `AGGREGATOR_SWIGGY`, `DIRECT`. Two channels that exist
today cannot name themselves in it.

The aggregator accept path landed in M6 Phase C and writes `DIRECT`. Its own
comment says why, and the reasoning is worth preserving because it is the
argument for this ADR:

> `DIRECT`, and the reason is a contract limit, not a preference. […] Writing
> `AGGREGATOR_SWIGGY` for an ONDC order would be a stored value that lies;
> `POS` would claim a cashier typed it. `DIRECT` is the only member that is not
> a false claim.

So every aggregator order at an outlet is currently stored as if a customer
ordered directly. Nothing is lost — the platform is recorded in
`aggregator_order.platform` and in `order.source_payload_json` — but the column
that exists to say where an order came from does not say it, and any report
grouping by channel is wrong by exactly the aggregator volume.

The table-ordering device (ADR-025, PROPOSED) has the same problem with no
partial answer available: an order a customer placed from a tab on their table
is not `POS`, not `QR` as that member is used today, and not `DIRECT`.

## Decision

Widen the CHECK in both stores with **two** members:

- **`AGGREGATOR`** — every aggregator channel, without exception.
- **`TABLE_TAB`** — an order placed by a customer from a table device.

Both are additive. The existing five members are unchanged.

### One generic aggregator member, not one per platform

This is the substantive decision and it goes against the two members already
present.

`aggregator_order.platform` is free `TEXT`, not an enum, and contracts 0032
states the reason in the migration itself: a new platform must not require a
migration, and the aggregator boundary check keeps platform names out of the
core. A per-platform member here would re-import exactly the decision 0032
rejected — every new platform becomes a contracts bump — and would write a
platform's identity into a schema file every consumer reads.

The truthfulness argument runs the same way. `AGGREGATOR` is true of every
aggregator order. A platform-named member is true of one platform's orders and
becomes a new false claim the first time a different platform arrives down the
same path — which is the defect this ADR exists to fix, reintroduced under a
different name.

The platform is not lost. It is recorded twice already, in
`aggregator_order.platform` and `order.source_payload_json`.

### The two platform-named members are DEPRECATED, not removed

`AGGREGATOR_ZOMATO` and `AGGREGATOR_SWIGGY` stay in the CHECK. Removing a member
is a breaking change and this bump is additive; they are marked deprecated in
the Zod enum, the Go constants, the OpenAPI schema and this ADR, with the
removal trigger **the next breaking contracts bump**.

Neither has ever been written. That claim is enforced rather than asserted:

- The postgres migration **raises** if any row carries either, instead of
  widening over it.
- The SQLite migration's pre-condition in `edge/database/src/migrations.rs`
  **refuses to run** on the same condition.

A deprecation note that silently widens over live data it claims does not exist
is a note that was false when written.

### Nothing writes the new members yet

The accept path keeps writing `DIRECT` until it is switched in its own change;
that switch is where `AGGREGATOR` is first written and it is **not** part of
0.8.1. `TABLE_TAB` has no writer and no code at all — ADR-025 is PROPOSED.

"Declared but never emitted" is therefore the correct state, and it is pinned by
exact assertion in `scripts/check-order-source-drift.mjs`, which fails the build
if either member is emitted by anything under `edge/` or
`apps/pos/src-tauri/src`. The list of unwritten members is removed **in the same
commit** as the writer that starts writing one — forced removal, not remembered
removal. This is the discipline contracts 0004 already applied to the wire
fields it deliberately did not add.

### The SQLite mechanism is a rebuild

SQLite cannot `ALTER` a CHECK, so the edge side rebuilds the `order` table:
create, copy, drop, rename, restore indexes. PostgreSQL is one `DROP CONSTRAINT`
and one `ADD CONSTRAINT` and rewrites nothing.

A rebuild proves its own guarantees or it has none (ADR-021's rule, second
table). `migrations.rs` asserts, after the batch:

1. the row count is unchanged;
2. a sampled row is identical column for column, nulls encoded rather than
   skipped — a column-order mistake in `INSERT ... SELECT` produces a table of
   the right size and shape that is wrong;
3. all four indexes are back (`DROP TABLE` takes them in silence);
4. the widened CHECK **actually rejects** — a real `INSERT` of an invalid value
   inside a savepoint that is always rolled back.

Note what (4) replaces. `stock_ledger_entry`'s rebuild demonstrates an
append-only *trigger* firing; the order table **carries no triggers at all**, so
there is no such guard to watch. The CHECK is the only live guard on this table,
and a `sqlite_master` name lookup would pass against a CHECK that came back
empty.

## Consequences

- One column now answers "where did this order come from" for every channel.
  Reports grouping by `source` stop silently attributing aggregator volume to
  direct orders — once the accept path is switched.
- A new aggregator platform needs no migration, matching 0032.
- Two dead members sit in the schema until the next breaking bump. Accepted:
  the alternative is a breaking change for cosmetics.
- Five declarations of one set now have a check that they agree. That check
  was watched RED (a member removed from one store) before it was trusted.

## Found while landing this, and fixed here

**The aggregator boundary check could not see `packages/contracts`.** Its
`SEARCH_ROOTS` covered `backend/internal`, `backend/cmd`, `edge`,
`apps/pos/src` and `apps/admin/src`, and its extensions were `.go/.ts/.tsx/.rs`
— so the shared schema, the one place a platform name binds every consumer at
once and is hardest to remove later, was the one place the check was blind.
`packages/contracts`, `.sql` and `.yaml` are now in scope.

**And its word boundary could not match an enum member.** `\b(ondc)\b` does not
match `AGGREGATOR_ONDC`, because `_` is a word character. A member named for a
platform — the single most likely way a platform name enters a shared schema —
would have passed. Demonstrated: `AGGREGATOR_ONDC` was planted in
`packages/contracts` and the check stayed **GREEN**. Platform names are now
matched with separator-aware lookarounds; protocol tokens keep `\b`, so a
trigger named `order_item_quantity_is_bounded_on_update` is not mistaken for a
Beckn callback.

The deprecated members are exempted **narrowly** — declaration files under
`packages/contracts` only, by the exact `AGGREGATOR_(ZOMATO|SWIGGY)` shape. The
same member in backend, edge or app code remains a violation, because that would
be the core branching on a platform rather than a schema carrying a legacy
value. The exemption is removed in the same commit as the members, at the next
breaking bump: an exemption that outlives its reason is a silenced failure
(contracts 0.6.0).
