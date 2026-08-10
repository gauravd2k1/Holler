<p align="center">
  <img src="imgs/holler_no_bg.png" alt="Holler" width="180">
</p>

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
├── imgs/                  # brand assets (source of truth for logo/icons)
└── tests/                 # integration, e2e, load
```

## Brand assets
`imgs/` holds the source artwork: `holler_no_bg.png` (transparent logo),
`Holler_best icon_copy.png` (full-resolution icon), and the generated favicon /
touch-icon set. Treat these as the source of truth — generate app icons and
web favicons from them rather than adding new one-off copies elsewhere.

## Deployment target
Outlet machines run **bare Windows 10 (64-bit, 4GB RAM)** with no WSL, no Docker and no database server. The POS is a single native executable over a statically-linked SQLite file, and works fully offline. See `docs/adr/ADR-013-outlet-deployment-target.md` — the dev tooling below is for the **cloud** side and never runs at a restaurant.

## Development
Requires: Go 1.22+, Node 20+, pnpm, Rust toolchain (MSVC on Windows), and Docker for the cloud services.

Docker can be hosted however you prefer — WSL2, Hyper-V, or a remote/managed Postgres. WSL2 is a convenience, not a requirement.

```
make dev    # bring up Postgres/Redis/NATS + backend (cloud stack, local dev only)
make test   # unit + integration tests
```

See `docs/architecture/SYSTEM_ARCHITECTURE.md` for system design and `docs/adr/` for key decisions.
