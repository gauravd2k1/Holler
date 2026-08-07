# ADR-008: Contracts-First Development

## Context
Holler is built by multiple parallel builder agents/sessions across Rust (edge/POS), Go (backend), and TypeScript (web/contracts) codebases. Without a single frozen source of truth for cross-boundary shapes, parallel implementations drift, causing integration bugs that surface late.

## Decision
`packages/contracts/` is the single source of truth for all cross-boundary shapes: TypeScript types + Zod schemas, generated/mirrored Go structs, OpenAPI spec, JSON Schema event payloads, the canonical order model, and SQLite/PostgreSQL migrations (with documented per-module ownership). Contract changes are serialized — only the orchestrator/architect session edits them, never a parallel builder agent. Every contract change increments a version and, if it changes semantics, is recorded in `docs/adr/`. CI includes a contract-drift check so Go/TypeScript/Rust representations must round-trip the same fixtures.

## Alternatives
- **Each module defines its own types, reconciled at integration time**: rejected — this is exactly the drift pattern contracts-first is meant to prevent; integration bugs would surface late and be expensive to trace.
- **Backend as sole source of truth, frontend/edge infer from API responses at runtime**: rejected — loses compile-time safety in Rust/TypeScript and makes offline-first edge development (which can't always hit the live API) harder to validate.

## Consequences
- Builder agents treat `packages/contracts/` as strictly read-only; see MILESTONE 0.5 (packages/contracts/ frozen for the vertical slice) and CLAUDE.md directory ownership.
- Adds process overhead (drift tests, serialized contract edits) in exchange for preventing an entire class of cross-language integration bugs.
- MILESTONE 0.5 exists specifically to produce and freeze the first contract set before any vertical-slice implementation begins.
