# ADR-003: SQLite in WAL Mode for the Edge Database

## Context
The Edge Node needs a local database that supports concurrent reads (POS, KDS, sync worker) with low-latency writes, survives power loss without corruption, and requires no separate server process on restaurant hardware.

## Decision
Use **SQLite in WAL (Write-Ahead Logging) mode** as the edge/local database, accessed only through the edge/database service layer — never exposed directly to UI code.

## Alternatives
- **SQLite default rollback-journal mode**: rejected — serializes readers and writers more aggressively, risking UI stalls during concurrent KDS/POS/sync access.
- **Embedded Postgres-compatible engine**: rejected — heavier footprint, no meaningful benefit for a single-outlet embedded workload.
- **A full local server process (e.g. Postgres in a local container)**: rejected — resource-heavy on modest hardware and adds an operational dependency the local-first design is trying to avoid.

## Consequences
- WAL mode allows concurrent readers alongside a writer, matching the multi-consumer local access pattern (POS terminals, KDS, sync worker).
- Local database access is fully encapsulated in `edge/database/`; other edge services and UI never touch the SQLite file directly.
- Local↔cloud consistency is handled entirely by the explicit sync protocol (ADR-009), not by SQLite itself.
