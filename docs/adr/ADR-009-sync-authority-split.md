# ADR-009: Sync Authority Split (Cloud = Catalog, Edge = Transactions)

## Context
Local-first architecture (ADR-001) requires a concrete, implementable conflict-resolution strategy between each outlet's edge node and Holler Cloud. General-purpose bidirectional merge (CRDTs, vector clocks, last-write-wins across the board) is powerful but adds significant complexity and subtle correctness risk — particularly dangerous for financial data.

## Decision
Adopt a strict **authority split**, documented in full in docs/spec/sync.md §50.1:
- **Cloud is authoritative for catalog/config**: menu, price books, tax profiles, users, roles, outlet settings. These sync **down** to edge, versioned; edge applies the latest authorized version — never merged, always replaced.
- **Edge is authoritative for operational transactions**: orders, KOTs, payments, shifts, stock movements. These sync **up** to cloud, **append-only** — never merged, always replayed.
- No CRDTs or bidirectional merge machinery are introduced. This split, plus the explicit per-aggregate conflict policy table in docs/spec/sync.md §51, is the entire conflict-resolution design.

## Alternatives
- **CRDTs / general bidirectional merge**: rejected — substantial implementation and reasoning complexity for a domain (financial transactions) where "automatically merged" is the wrong answer; corrections must be explicit (void/refund/adjustment, §2.5), not silently reconciled.
- **Last-write-wins everywhere**: rejected — acceptable for availability/config fields, unacceptable for financial transactions where it can silently drop revenue-relevant data.

## Consequences
- Every aggregate's conflict behavior must be explicitly documented (docs/spec/sync.md §51 table) — "do not redesign" is a standing constraint on future agents.
- Transaction replay means the cloud is an append-only ledger of what every edge node produced; it never rewrites edge-originated rows.
- Catalog versioning means an edge node briefly offline simply applies the next authorized version on reconnect — no merge conflict is possible by construction.
