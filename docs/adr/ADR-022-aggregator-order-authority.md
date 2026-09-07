# ADR-022 — Aggregator orders are two aggregates, not one

- **Status: ACCEPTED, 2026-09-08.** Approved to open M6 Phase C. The four
  questions the draft escalated are settled in the addendum below; the body
  above is unchanged from the draft that was approved.
- **Contracts: 0.8.0, not 0.7.0.** The draft targeted 0.7.0 because that was
  the next bump after Phase A. 0.7.0 was then spent on Phase B's admin routes
  (ADR-024). The repo wins over the plan: aggregator shapes land at 0.8.0.
- **Date:** 2026-09-02
- **Milestone:** M6 (aggregator integration), Phase C
- **Contracts:** targets **0.7.0**, which lands **after M6 Phase A is green** —
  A1–A3 change the outbox these shapes will ride on, and bumping contracts across
  a wedged outbox would bury the same defect twice.
- **Supersedes nothing.** Extends the authority rule ADR-011 drew between
  `restaurant_table` (config) and `table_session` (edge-authoritative), and
  ADR-014 drew again between `station` and `kot`.

## Context

M6 integrates Swiggy, Zomato and ONDC: inbound orders, menu and availability
push, order state round-trip, stock-out snooze.

An inbound aggregator order arrives from an external system over the public
internet. **The till has no public address**, so it cannot receive one directly.
Every other order in this product is created at the till.

`order` in contracts 0.6.3 already carries `external_order_id` (null for
POS/QR/Direct) and `aggregator_discount_paise`. **No aggregator-specific table
exists anywhere in 0.6.3** — no platform credential, no menu/item mapping, no
webhook dedupe, no snooze state.

## Decision

**There are two aggregates.**

1. **`aggregator_order` — CLOUD-AUTHORITATIVE**, cloud→edge, **replace-not-merge**.
   It is an inbound *document* from an external system. The platform's own status
   changes — cancellation, rider assigned — land here.
2. **`order` — EDGE-AUTHORITATIVE**, edge→cloud, append-only, exactly as today.
   The edge creates one from the inbound document, linked by `external_order_id`,
   and from that moment it is a **local transaction** carrying every offline
   guarantee this product already makes.

**The state round-trip splits on the same line.** Accept / ready / picked-up
**originate at the till**, so they ride the existing edge→cloud→platform path on
the edge-authoritative `order`. Platform-originated changes arrive on the
cloud-authoritative document and are **surfaced to the till, never silently
applied** to the local order.

## Why not one aggregate

Making `order` cloud-authoritative when the channel is an aggregator and
edge-authoritative otherwise is **split authority on a single aggregate**, which
§50.1 and the contract rubric forbid outright: *no split-authority columns —
split the aggregate instead*.

It also fails on product grounds, and that is the stronger argument. A
delivery-heavy outlet could not modify, bill or close an aggregator order with
the line down. **That is not a variation on the product; it is the opposite of
it** — local-first operation is the differentiator, and it would be absent for
exactly the orders where it matters most.

## Consequence, stated plainly

**A new aggregator order cannot arrive while the uplink is down. One that has
already arrived is fully operable offline.**

This is the guarantee we publish. It is also true of every competitor: no system
receives an internet-originated order on a machine with no route to the internet.
Stating it in the ADR keeps a future session from "fixing" the first half and
silently trading away the second.

## What this draft deliberately does not decide

Escalated with the draft, to be settled before 0.7.0 is drawn:

- The table set behind `aggregator_order` — platform credential storage, menu and
  item mapping, webhook **dedupe key**, snooze state, and which of those are
  cloud-only (the `refresh_token` / `device_credential` precedent) versus mirrored.
- Whether the edge's `order` creation from an inbound document is automatic or
  operator-confirmed at the till.
- How a platform cancellation that arrives **after** the till has billed the local
  order is surfaced — a live question, not a schema one, and the shape most likely
  to produce a money defect.
- Retention of `aggregator_order` documents once their local `order` is closed.

## Rules that will bind every builder once approved

Recorded now so they are not rediscovered later:

1. **`aggregator_order` is replace-not-merge.** A cloud-authoritative document is
   replaced wholesale at its version, never field-merged with local state — the
   `GET /sync/config` precedent. A merge would make the edge a second writer.
2. **A platform status never writes `order.status`.** One writer, as ADR-014
   already requires for `kot.status`.
3. **The link is `external_order_id`, and it is tenant-scoped and unique per
   platform.** Global uniqueness across platforms is wrong: two platforms can and
   do issue the same id.
4. **An inbound document that cannot be mapped to menu items is recorded, not
   refused** — the `grn_gap` precedent from ADR-019. Refusing a delivery order
   that is already cooking is the outage, not the protection.

---

# Addendum, 2026-09-08 — the four open questions, decided

The draft listed four things it "deliberately does not decide" and required them
settled before 0.8.0 is drawn. Each is decided below **by default rather than by
escalation**, because none of them is a breaking change, an authority split, or
a public exposure. Where a default is arguable the argument is recorded, so a
later session can overturn it on evidence instead of rediscovering it.

## 1. The table set

| Table | Store | Direction | Why |
|---|---|---|---|
| `aggregator_order` | both | **cloud→edge**, replace-not-merge | The inbound document. Authority per the body of this ADR |
| `aggregator_order_line` | both | **child row, no direction** | Travels inside its parent's payload — the `invoice_line` / `grn_line` precedent. Not an aggregate, never given a sync direction |
| `aggregator_platform_credential` | **Postgres only** | **none, ever** | API keys and platform secrets. **The edge never talks to a platform**: it has no public address, which is the premise of this whole ADR, so an outlet has no use for a credential it cannot spend. The `refresh_token` / `device_credential` precedent, and the same reasoning — credential material does not travel to a machine that does not need it |
| `aggregator_item_map` | **Postgres only** | **none in stage 1** | Platform item id → `menu_item`. Resolution happens **at the cloud, when the document arrives**, so the edge receives an `aggregator_order` whose lines already name local menu items. Mirroring the map would mean two resolvers that can disagree |
| `aggregator_callback_receipt` | **Postgres only** | **none, ever** | The webhook dedupe record: `UNIQUE (tenant_id, platform, message_id)`. A platform retries; a duplicate callback must be idempotent. Edge-local equivalents already exist for the other direction (`sync_outbox_block`), and this is the inbound mirror of that idea |

**`aggregator_item_snooze` is NOT drawn in stage 1.** Snooze is M6 **C2**, which
is PARKED behind platform sandbox access, and the push path that would write it
does not exist. Drawing a table nothing writes is a column nothing reads with
extra steps. It arrives with the criterion.

## 2. Order creation from an inbound document: OPERATOR-CONFIRMED, not automatic

**Default taken: the till shows the inbound document and a human accepts it.**
Creation of the local `order` happens on that accept.

Three reasons, in order of weight:

1. **It matches the actual workflow.** Every aggregator platform has an accept /
   reject step; a restaurant that cannot refuse an order it has no ingredients
   for is not a product anyone will run.
2. **Rule 4 of this ADR makes automatic creation unsafe.** An inbound document
   that cannot be mapped to menu items is *recorded, not refused* — so automatic
   creation would put an order with unresolved lines into a kitchen. Recording a
   document a human then reads is the whole point of recording it.
3. **It keeps one writer.** The edge creates the local order, as it does for
   every other channel. Nothing about the aggregator path makes the cloud a
   creator of `order` rows.

**The cost, stated:** an order sitting unaccepted is a real operational failure
mode, and this decision creates it. Mitigation is a till surface that makes an
unaccepted document loud — the same shape as the sync-blocked banner — and that
is Phase C work, not a later idea.

## 3. A platform cancellation arriving AFTER the till has billed

**Default taken: recorded on the document, surfaced to the operator, and it
NEVER touches the invoice or the local order automatically.**

`invoice` is immutable with exactly one legal transition (`ISSUED → CANCELLED`,
contracts 0.5.0), and that transition is an operator action with an audit trail.
A platform message must not be able to void a bill: it is money, it is a legal
document, and the platform's view and the till's view can legitimately disagree
— exactly the ADR-019 situation where both numbers are right and reconciling
them silently is the defect.

So the cancellation lands on `aggregator_order`, the till shows it beside the
local order, and a human decides. **This is the case most likely to produce a
money defect** (the draft said so), which is precisely why it is manual.

## 4. Retention of `aggregator_order` documents

**Default taken: kept indefinitely in stage 1. No purge, no archive, no TTL.**

It is the inbound record of what an external system asked for, and the thing
anyone would reach for in a dispute about an order that was billed, refunded or
cancelled. Deletion semantics are also unresolved repository-wide — the
cloud→edge config path has no tombstone at all, filed at Phase B close — and
inventing a retention rule for one table while that is open would set a
precedent by accident.

Filed for revisit when volume justifies it, which is a real trigger and not a
polite deferral: a busy delivery outlet generates thousands of these a month.

## What this addendum does NOT decide, and will not be quietly assumed

- **Nothing about signing, the registry, or a public callback endpoint.** Those
  are M6.1. In M6 the callback receive path is built and reachable **locally
  only**.
- **Nothing about snooze**, per §1.
- **No live channel on any platform.** Stage 1 delivers the framework and two
  adapters against fakes. C8 is **SHAPE ONLY** and the acceptance file says so
  in those words.

---

# Addendum 2, 2026-09-08 — the cloud→edge down-path (M6 C1)

**Why it is in M6 and not M6.1.** M6 C1 requires a received aggregator order to
bill, print and close with the cloud provably unreachable. That is this ADR's
published guarantee, and it is unreachable unless the document gets to a till in
the first place — `aggregator_order` is the first non-config aggregate to travel
cloud→edge, and nothing carried it. It is also verifiable without a registry, a
signature or a public endpoint, so it belongs on this side of the cut. Deferring
it would have meant the framework never reached a till at all.

**Scope, deliberately narrow.** `aggregator_order` only, not a general
down-sync framework. A framework built for one caller is a framework shaped by
one caller, and the second aggregate to need this will have requirements this
one cannot see.

| Decision | Why |
|---|---|
| **Pull, not push** | The till has no address the cloud can reach — the premise of this whole ADR |
| **Keyset cursor on `(updated_at, id)`** | Documents are rewritten while an outlet pages (a status change), and an OFFSET walk silently skips or repeats rows. `updated_at` rather than `received_at` because a document whose status changed **must travel again** — ordered by arrival, a cancellation would never reach the till cooking the order |
| **Cursor is edge-local** (`sync_state.aggregator_pull_cursor`, SQLite only) | One outlet's record of how far IT has read. A mirrored cursor is a second opinion about what an outlet has seen — the `invoice_sequence` precedent |
| **Cursor advances only after a page is applied** | Moving ahead of what was written skips a document permanently and silently |
| **Idempotent by `id`, replace-not-merge on `document_version`** | The pull is at-least-once by construction: a drain that dies after writing and before advancing re-reads the page. Without idempotency every crash would double an outlet's delivery orders |
| **Rides the A5 periodic loop, inside the same database lock** | No second pump host. A document apply interleaving with an outbox pump on one SQLite connection is the fault that appears as a corrupt read once a month |
| **A failed pull keeps what the till has, logs, shows nothing** | Offline is normal here. A new aggregator order *cannot* arrive while the uplink is down — that is the guarantee, not a fault to alarm a cashier about |

## The rule, with no exception to maintain

**The edge holds `aggregator_order` as a READ-ONLY MIRROR. It carries no
edge-written column at all.** One writer on the edge side — the apply function
the pull calls — and nothing else may touch the table.

### The version of this that was wrong, and why it was wrong even though it worked

The first cut of the down-path gave the mirror two edge-written columns,
`accepted_at` and `local_order_id`, and protected them by omitting them from the
upsert's SET list so a later cloud document could not clear them. That guard
worked. It was tested, and the test was watched failing with the columns added
back.

**It was still split authority.** Two edge-written columns on a
cloud-authoritative aggregate are split authority however carefully the SET list
is maintained, and the contract rubric says what to do about that directly: *no
split-authority columns — split the aggregate instead.* A guard is a documented
obligation, and this repository's history is a list of documented obligations
that held right up until they didn't. It also grows: `printed_at` and
`closed_at` are the obvious next two, each arriving with the same reasoning and
the same guard.

### What replaced it: nothing, because acceptance was always derivable

**Accepting a document IS creating the local `order` for it.** So:

| Question | Answer, derived |
|---|---|
| Is this document accepted? | An `order` exists whose `external_order_id` matches |
| When was it accepted? | That order's `created_at` |
| Which local order is it? | That order's `id` |

No second table, no denormalised copy, and nothing that can drift from the fact
it describes. `repo::aggregator_order_acceptance` is the single place that join
lives, and the unaccepted queue a till reads is a `LEFT JOIN … WHERE o.id IS
NULL`. Contracts 0.8.0 migrations sqlite/postgres 0034 drop the columns from
both stores — the cloud too, because nothing writes them there either and a
column nothing writes is a column that does not exist.

### The test that keeps it true

`edge/database/tests/aggregator_mirror.rs` asserts against **the schema**, not
against a struct — a struct can drop a field while the column lingers, and the
column is what a future writer reaches for:

> `aggregator_order grew an edge-written column 'accepted_at'. It is
> cloud-authoritative: acceptance and every state that follows it belong on the
> local order, derived, not stored here`

Watched failing with the drop commented out. It names `printed_at` and
`closed_at` alongside the two that existed, so the next attempt fails on arrival
rather than after review.

**Acceptance now survives later cloud documents STRUCTURALLY rather than by a
guard**: a cloud document cannot touch an `order` row, so there is no path by
which it could be cleared.
