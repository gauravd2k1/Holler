# ADR-025 — The table tab is a LAN client of the till, with its own principal kind

- **Status: PROPOSED, 2026-09-10.** Recorded now so the shape is decided before
  anything is built. **Nothing here is M6 work**, and no code accompanies it.
- **Proposed landing milestone: M8**, trigger *after the first pilot runs on
  `STAFF_ONLY`*. Not M6 (aggregators and the sync gaps), not M7 (reporting).
- **Contracts:** one additive item is pulled forward — `order.source` must name
  `TABLE_TAB` — and it travels with the platform widening already pending for
  **0.8.1**, before M6 closes. Everything else waits for M8 planning.
- **Landed in commit `fc38006`**, whose message describes only the payments-
  allocation fix. The two are unrelated: these documents were staged before that
  fix and were committed with it by mistake. Recorded here rather than rewritten,
  because history rewriting is not a builder's call.
- **Extends** ADR-009/§50.1 (authority), ADR-015 (edge credential sync and the
  LAN transport), ADR-017 (device enrolment), and ADR-011's `restaurant_table`
  (config) versus `table_session` (edge-authoritative) split.

## Context

Some outlets will place a tab on each dining table so customers see live
availability and order for themselves. Other outlets stay staff-ordered. **Both
modes coexist across outlets and must coexist inside one outlet** — not every
table gets a tab, and an outlet that buys six tabs for twenty tables is the
ordinary case, not an edge case.

That last sentence is the whole design pressure. A mode that is true of an
outlet as a whole would be cheap; a mode that is true of *some tables* means the
till serves two ordering paths at once, for the same service, on the same
`table_session`.

## Decision

### 1. The tab is a LAN client of the till, never a cloud client

Same shape as the KDS: a web app served from the till's LAN server, enrolled as
a device, talking to the edge over the existing LAN transport (ADR-015). It
never talks to the cloud.

Orders are edge-authoritative (§50.1), and **table ordering must work with the
uplink down** — an outlet in this market loses its connection routinely, and a
tab that needs the cloud is the opposite of the product. Directory: `apps/table`,
scaffolded from `apps/kds`.

### 2. `table_device` is a new principal kind, not a cashier account

Its permission set is **structurally narrower than any staff role**: read the
menu and availability for its own outlet; create and append items to the order
bound to **its** table; nothing else. No billing, no void, no discount, no other
table's data, no read of another table's order.

**Enforced at the LAN boundary, not by the tab's UI.** A device a member of the
public physically holds is not a trusted client, and a permission that lives in
the client is not a permission. This falls under the M2 LAN socket security gate
and **needs that gate reviewed before any customer-facing device is enrolled
anywhere**: a threat model for a device in public hands, rate limiting, and a
per-table binding that cannot be re-pointed from the tab itself.

### 3. Ordering mode is cloud config that syncs down

Per outlet: `ordering_mode ∈ {STAFF_ONLY, TABLE_TAB}`. Per table: `tab_enabled`.
Both are set in `apps/admin` and delivered by the config pull that now rides the
A5 periodic loop (ADR-024). **The edge refuses tab enrolment for a table that is
not enabled** — the refusal is at the edge because that is where enrolment is
decided, and a config that only the admin console honours is not a control.

Config, cloud→edge, exactly like `restaurant_table`: which tables have tabs is a
management decision, not a shop-floor one.

### 4. `order.source` names `TABLE_TAB`

Widen the `order.source` CHECK — today
`POS`/`QR`/`AGGREGATOR_ZOMATO`/`AGGREGATOR_SWIGGY`/`DIRECT` — to name
`TABLE_TAB` in the **same additive change** that names the platform for
aggregator orders. One CHECK widening, two values, one ADR note, one consumer
list.

Reports must distinguish staff-entered, tab-entered and aggregator orders. A
tab-entered order recorded as `POS` is the same defect as an ONDC order recorded
as `DIRECT`: a stored value that lies, wrong in every report that groups by
channel.

### 5. Binding rides `table_session`; no new aggregate

A tab binds to a table, and its orders attach to that table's open
`table_session` — which already exists, is edge-authoritative, and already syncs
up. **No new aggregate is introduced**, and the tab authors nothing the till
does not already author.

### 6. The tab is append-only

A tab may **add** items to its table's order. It may not modify or remove them;
corrections go through staff at the till.

This sidesteps tab-versus-waiter concurrent editing entirely. Per-aggregate
ordering already exists in the outbox (M6 A2), and with an append-only client it
is sufficient — there is no edit to lose and no last-writer to pick. The
restriction is a design decision, not a limitation to be relaxed quietly later:
relaxing it reintroduces the concurrency problem in full.

### 7. Availability is edge state, answered by the edge

The till already computes stock-out and low-stock. Expose that as a LAN read;
the tab renders what the edge says rather than keeping its own view.

**When an item goes unavailable between the tab showing it and the till
receiving the order, the till rejects that item with a typed reason the tab
renders.** Never silently accepted, never silently dropped — the same discipline
as `missing_reference` (M6 A1/C7). A customer who ordered something the kitchen
cannot make must be told by the tab, immediately, and a rejection nobody sees is
the failure this project keeps finding under different names.

### 8. Payment stays on the till in the first version

Tab orders, staff bills. **Customer-side payment is a separate decision with its
own gate** — it brings a payment provider, refunds initiated by a customer, and
a device in public hands touching money. Parked deliberately, not forgotten.

## Prerequisites, in order

1. **M6 A7 — the `kot` routes.** A tab-originated order must reach the kitchen
   and must replay. Today `edge/sync/src/route.rs` maps only `order` and
   `table_session`, so a KOT has no route at all; a tab on top of that is a
   customer ordering into a queue nothing drains.
2. **The `order.source` widening** (0.8.1, before M6 closes).
3. **The LAN security gate review**, for a device the public holds.
4. **A pilot running `STAFF_ONLY`** in at least one outlet. Table ordering is
   not the thing to discover an outlet's LAN with.

## Consequences

- One outlet runs two ordering paths against one `table_session`, and the till
  is the only writer of the order either way.
- The LAN transport gains its first untrusted client. Every existing LAN
  consumer (KDS, printer bridge) is staff-side; the threat model changes, and
  the security gate is the place that gets decided.
- `apps/table` is a fourth frontend to keep alive. That cost is accepted: the
  alternative — a cloud-hosted tab — fails the offline requirement outright.
- The append-only rule is load-bearing. If a later version lets a tab edit or
  remove a line, the concurrency question this ADR avoided comes back and must
  be answered before that ships.

## Alternatives rejected

- **Tab talks to the cloud.** Fails offline, contradicts §50.1, and would make a
  customer-facing screen depend on the least reliable component in the system.
- **Tab authenticates as a cashier with a narrow role.** A staff role is a role a
  human is trusted with; reusing one for a device in public hands means every
  future widening of that role silently widens what a customer's tab can do.
- **A new aggregate for tab sessions.** `table_session` already models "this
  table is in service" and already syncs. A parallel aggregate would need its own
  authority answer for no gain.
- **Editable tab orders.** Requires conflict resolution between a customer and a
  waiter editing the same order, which is a genuinely hard problem and is not
  worth buying with the first version.
