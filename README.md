# Holler

A local-first Restaurant Operating System for India. See `docs/vision.md` for product vision and `CLAUDE.md` for the agent working context.

> HOLLER MUST CONTINUE RUNNING EVEN WHEN THE INTERNET DOES NOT.

## Status
**Milestone 0 — Foundation.** Repository scaffolding, architecture/domain docs, ADRs, and empty app/service shells only. No business logic yet — see `CLAUDE.md` for the current milestone's exclusion list.

## Repository layout
```
holler/
├── CLAUDE.md              # agent working context (read this first)
├── apps/                  # pos (Tauri), admin, kds (PWA), waiter (Flutter), customer-ordering
├── edge/                  # local edge node services (Rust): sync, printer, device, database
├── backend/               # Go modular monolith cloud backend
├── packages/contracts/    # source of truth for cross-boundary shapes (frozen after Milestone 0.5)
├── docs/                  # vision, spec/, architecture/, domain/, adr/
├── deployments/           # docker, kubernetes, terraform
└── tests/                 # integration, e2e, load
```

## Development
Requires: Docker Desktop with WSL2 backend, Go 1.22+, Node 20+, pnpm, Rust toolchain (MSVC on Windows).

```
make dev    # bring up Postgres/Redis/NATS + backend inside WSL2
make test   # unit + integration tests
```

See `docs/architecture/SYSTEM_ARCHITECTURE.md` for system design and `docs/adr/` for key decisions.
