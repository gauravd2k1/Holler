# ADR-022 — Aggregator orders are two aggregates, not one

- **Status: PROPOSED (draft).** Escalated for approval **before a single table is
  drawn**. No schema, no `AggregateType` member, no migration accompanies this
  draft deliberately.
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
