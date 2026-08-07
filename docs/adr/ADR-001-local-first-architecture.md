# ADR-001: Local-First Architecture

## Context
Indian restaurant venues face frequent, sometimes prolonged internet outages. Existing cloud-dependent POS systems fail or degrade badly during outages, which is unacceptable for order-taking and billing — a restaurant's core revenue path.

## Decision
Holler is architected local-first: an on-premise **Edge Node** (SQLite + local services) is the authoritative system of record for operational transactions (orders, KOTs, payments, shifts, stock movements) at each outlet. Cloud (PostgreSQL) is authoritative for catalog/config (menu, pricing, tax, users, roles) and syncs down to edge. Core restaurant operations — dine-in/takeaway/delivery order entry, KOT, KDS, modifiers, discounts, tax, billing, cash payments, offline card records, inventory deduction, shift ops, printing, cached customer lookup — must all function with zero internet connectivity.

## Alternatives
- **Cloud-dependent SaaS POS** (like many incumbents): rejected — a network blip stops billing, which is the core failure mode we're designing away.
- **Fully peer-to-peer/CRDT sync with no clear authority**: rejected as unnecessary complexity; see ADR-009 for the simpler authority split actually adopted.

## Consequences
- Requires a genuine local database + local sync engine + local realtime transport (see ADR-003, ADR-007, ADR-009), not just an offline cache.
- Adds engineering cost (edge/cloud consistency, resumable sync) in exchange for the product's central differentiator.
- All builder agents must treat "does this still work with the outlet offline?" as a first-class acceptance criterion for anything in the order-to-bill path.
