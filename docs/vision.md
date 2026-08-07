# HOLLER — Product Vision

Read by humans and the orchestrator when planning. Not loaded by builder agents.

## What Holler is
A state-of-the-art Restaurant Operating System for India — POS, table management, order/KOT/KDS, aggregator integration, inventory & recipe costing, procurement, central kitchen, payments, GST invoicing, settlement reconciliation, CRM, loyalty, online/QR ordering, reservations, staff management, multi-outlet management, analytics, menu engineering, accounting exports, and (later) AI-assisted operations. Not merely a billing app — an extensible Restaurant Commerce and Operations Platform.

Competes with Petpooja, Restroworks/Posist, UrbanPiper, DineOpen, Bhojan Setu, DotPe, GoFrugal, SlickPOS, TMBill — without cloning any of them.

## Core philosophy
> HOLLER MUST CONTINUE RUNNING EVEN WHEN THE INTERNET DOES NOT.

Local-first, not cloud-dependent. Cloud enhances; it is never required for a restaurant to take an order, fire a KOT, or close a bill.

## Primary principles
1. **Local-first** — dine-in/takeaway/delivery orders, KOT, KDS, modifiers, discounts, tax, billing, cash/manual card, inventory deduction, shifts, printing, and basic customer lookup all work offline.
2. **Extremely fast** — add-to-cart <50ms, KOT creation <100ms, LAN POS→KDS <250ms, invoice creation <300ms. Never a cloud round trip in the critical order-entry path.
3. **Zero lost orders** — orders/KOTs/payments/refunds/aggregator events/inventory transactions are durable, audited records.
4. **Idempotency** — every externally-initiated transaction (webhooks, settlement imports, menu sync) tolerates duplicate delivery.
5. **Immutable financial history** — corrections via cancellation/void/credit-note/refund/adjustment/reversal, never destructive edits.
6. **Contracts first** — shared interfaces frozen before parallel implementation begins.

## Competitive positioning
Holler's edge: exceptional speed, true local-first operation, no lost orders, restaurant-LAN resilience, sophisticated ingredient-level inventory, transparent reconciliation, open integration architecture, explainable AI analytics, modern API-first design, lower operational complexity.

## Non-functional priorities
Reliability > animations. Correctness > cleverness. Data integrity > convenience. Local operation > cloud dependency. Deterministic accounting > AI. Extensibility > provider coupling. Fast workflows > decorative UI. Auditability > destructive editing.

See `HOLLER_MASTER_PROMPT.md` §1, §2, §88, §93 for full source text; this file is the living, evolving copy.
