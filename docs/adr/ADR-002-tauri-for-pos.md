# ADR-002: Tauri for POS Desktop

## Context
The POS app must run on modest restaurant-grade Windows hardware, start in under 2 seconds, and keep UI interactions near-instantaneous, while still giving us a modern web-based UI stack for velocity.

## Decision
Build the POS desktop app with **Tauri** (Rust core + system webview) instead of Electron. Rust handles local DB access, printer abstraction, device interfaces, sync, LAN communication, background services, encryption, and filesystem operations. React/TypeScript (Vite, TanStack Query/Router, Zustand, React Hook Form, Zod) handles presentation only.

## Alternatives
- **Electron**: rejected — ships a bundled Chromium + Node runtime per app, materially larger memory/disk footprint and slower cold start on the target low-spec hardware profile (§3.8).
- **Native Win32/WPF app**: rejected — loses the shared web UI stack used across admin/KDS, increasing team cost without a performance win large enough to justify it.

## Consequences
- Rust is now a required skill/dependency for the POS team; device/printer/sync code lives in Rust, not JS.
- Smaller runtime footprint and faster startup, better matching the <2s launch and offline-resilience targets.
- Windows builds use the MSVC toolchain on the Windows side of the dev machine (§3.8), separate from the WSL2-hosted backend.
