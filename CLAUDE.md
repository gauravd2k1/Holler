# HOLLER — Agent Working Context

Restaurant Operating System for India. Local-first: core ops run without internet.
Full vision: `docs/vision.md`. Full spec source: `HOLLER_MASTER_PROMPT.md` (orchestrator/humans only — builder agents do not load it).

## Tech stack
- POS desktop: Tauri + React + TypeScript + Rust + SQLite (WAL)
- Cloud backend: Go, PostgreSQL, Redis, NATS JetStream (modular monolith)
- Web admin: React, TypeScript, Vite, TanStack Query/Router
- KDS: PWA (web), LAN-first
- Waiter app: Flutter (Android-first) — decided, see ADR-010
- Contracts: `packages/contracts/` — TS+Zod, Go structs, OpenAPI, SQLite/Postgres migrations. Read-only for builder agents.

## Dev environment
- Windows laptop (i7-9750H, 16GB, GTX1050). Docker services (Postgres/Redis/NATS/Go backend) run in **WSL2**. Tauri/Rust Windows builds run on Windows (MSVC). Cap concurrent agent sessions at 3.
- `make dev` brings up the stack. Frontend runs natively for HMR.

## Money / time / identifiers
- Money: INR stored as integer paise (₹125.50 = 12550). Never floating point for money.
- Time: UTC storage; outlet timezone stored separately, rendered local. Business day may cross midnight.
- IDs: UUIDv7/ULID internally. Human-facing numbers are short (Order #A184, Invoice FY26/PNQ/001423). Never expose sequential PKs as security identifiers.

## Coding rules
- Strict typing, no `any`. Business logic outside UI components; DB logic outside HTTP handlers.
- Provider-specific code (aggregators, payments, printers) behind interfaces — never leak into core domain.
- No magic numbers, no hard-coded tax rates/restaurant IDs/URLs, no secrets committed.
- Contracts (`packages/contracts/`) are edited only by the orchestrator/architect session — never by a builder agent.
- Never `// TODO implement later` for current-milestone or excluded-list work.

## Directory ownership
- `apps/pos` — POS Tauri app. `apps/admin` — web admin. `apps/kds` — kitchen display PWA. `apps/waiter` — Flutter app.
- `edge/` — local edge node services (sync, printer, device, database) — Rust.
- `backend/internal/<context>` — one bounded context per directory (auth, tenant, outlet, menu, ordering, kitchen, inventory, procurement, payments, aggregators, compliance, reporting, crm).
- `packages/contracts` — cross-boundary source of truth (read-only to builders). `packages/ui`, `packages/validation`, `packages/generated`.
- `docs/spec/<context>.md` — one spec per bounded context; an agent loads only CLAUDE.md + its assigned spec file(s) + `packages/contracts/`.

## Test/build commands
- `make dev` — start local stack (WSL2 Docker Compose + backend).
- `make test` — unit + integration tests.
- Backend: `go test ./...` inside `backend/`.
- POS: `pnpm test` / `pnpm tauri dev` inside `apps/pos/`.
- CI: lint, format, unit, integration, contract-drift check, build, security scan.

## Current milestone: MILESTONE 0 — Foundation
Scope: repo scaffolding, architecture/domain docs, ADR-001..010, CI skeleton, Docker Compose (WSL2), Postgres/Redis/NATS config, Go backend health service, POS shell, KDS shell, spec decomposition (this file + docs/spec/ + docs/vision.md).

**EXCLUDES (must not exist yet):** any business logic, any UI beyond empty shells, aggregators, payments, inventory, CRM, loyalty, AI.

Next: MILESTONE 0.5 — freeze `packages/contracts/` for the vertical slice before any slice implementation begins.

## Response rules for agents
Inspect repo first, output a concise plan, then edit real files. If a task touches >15 files, stop and present the plan instead of proceeding. Report per milestone: Implemented / Verified / Performance / Remaining / Next.
