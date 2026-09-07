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
