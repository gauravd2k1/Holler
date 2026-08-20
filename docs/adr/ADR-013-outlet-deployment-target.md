# ADR-013 — Outlet deployment target: bare Windows 10, no WSL, no Docker

Status: Accepted (2026-08-07)
Relates to: ADR-001 (local-first), ADR-002 (Tauri for POS), ADR-003 (SQLite WAL), ADR-011 (encryption at rest)

## Context

Every environment note written so far — CLAUDE.md §Dev environment, README, `docker-compose.yml`, SYSTEM_ARCHITECTURE — describes WSL2 as though it were part of Holler. It is not. It is one developer's convenience for running PostgreSQL, Redis, NATS and the Go backend on a single laptop.

Holler's actual deployment target is a restaurant counter machine. Those are frequently old, minimally specified, and running a stock Windows 10 install that the owner will not upgrade, virtualise or extend. WSL2 needs virtualisation enabled in firmware, a supported Windows build, and several gigabytes of RAM to be useful. Requiring it at an outlet would be an absurd tax on a system whose entire premise (ADR-001) is that a cashier keeps working when everything else fails.

The distinction was never written down, so it was one careless sentence away from becoming a real requirement.

## Decision

**No outlet machine runs WSL, Docker, PostgreSQL, Redis or NATS. Ever.**

The outlet runs exactly one thing: the Holler POS, a native Windows executable. Its local state is a single SQLite file, statically linked into the binary (`rusqlite` with `bundled`), so there is no database server, no service to install, and no system SQLite dependency. Sync to the cloud is an outbound HTTPS client inside that same process. When there is no internet, the POS is fully functional and the outbox simply accumulates (ADR-001, §50.1).

**Baseline target: Windows 10, 64-bit, 4 GB RAM, spinning disk.** If it runs on that, it runs everywhere we care about. Two platform dependencies follow from Tauri (ADR-002) and must be handled by the installer rather than assumed:

- **WebView2 Runtime.** Windows 11 ships it; many Windows 10 machines do not have it. The installer must not assume a download is possible — a restaurant being installed on a flaky connection is the normal case, not the edge case. The bundle therefore embeds the runtime rather than fetching it at install time, at the cost of a larger installer. Size is a one-time cost; a failed install on-site is a callout.
- **Visual C++ runtime.** The MSVC Rust target links against it. The installer must carry whatever the binary needs so a bare machine does not fail with a missing-DLL dialog.

**WSL2 is a developer convenience and nothing more.** It is one way to run the cloud stack locally; Docker Desktop on Hyper-V, a remote dev database, or a natively built Go backend are all equally valid. No document may state or imply that Holler requires it.

## Consequences

- `make dev` and `docker-compose.yml` are development tooling for the **cloud** side. Neither is ever run at an outlet.
- The POS build must be verified on a clean Windows 10 VM with no developer tooling, no WebView2 preinstalled, and no internet during install. Until that has been done on real hardware, the Windows 10 claim is a design intent, not a verified fact — this ADR is the record of the intent, not evidence of the test.
- The encryption-at-rest design (ADR-011) must not acquire a dependency that assumes a developer toolchain. The current AES-256-GCM approach is pure Rust and satisfies this; a future move to SQLCipher must keep the outlet install free of OpenSSL/Perl runtime requirements even though building it needs them.
- Modest hardware sharpens two existing gaps: the POS decrypts its database to a working file on open, which costs disk I/O on a spinning disk, and the in-session plaintext window is longer on slow hardware. Both are already recorded.
- KDS is a LAN-first PWA (Milestone 2) and must not assume a server process on the counter machine beyond the POS itself.

---

## Addendum — how the two runtime dependencies were actually closed (2026-08-20)

The Decision above required the installer to carry both runtimes. Only one of them could be done that way.

### WebView2 — embedded, as specified

`bundle.windows.webviewInstallMode = { "type": "offlineInstaller", "silent": true }`. The runtime is packaged into the setup executable at build time. Evidence: the NSIS artefact is 209 MB, consistent with the ~127 MB offline package being embedded rather than fetched. Install-time needs no network, which is what this ADR requires.

**Build-time still needs one.** The bundler downloads the WebView2 package from `go.microsoft.com` while building, so an air-gapped *build machine* does not work. That is a different machine from the outlet and is not a violation of this ADR — recorded so nobody discovers it as a surprise.

### VC++ runtime — dependency removed rather than embedded, and this is a risk with a fallback

Tauri's bundler has **no** configuration for the Visual C++ redistributable; confirmed by reading `tauri-utils-2.9.3/src/config.rs`, where no such field exists. So the dependency is eliminated instead: `apps/pos/src-tauri/.cargo/config.toml` sets `-C target-feature=+crt-static` for the MSVC targets, statically linking the CRT.

**Why this is safe rather than merely convenient.** The dangerous failure mode for a static CRT is a *mixed* one: some C dependency left on the dynamic runtime, allocating on one heap and freeing on another. That does not fail at startup — it produces rare, unreproducible corruption in production, which is the worst class of defect this product could ship. `dumpbin /DEPENDENTS` on the release binary shows **no `vcruntime140.dll` and no `msvcp140.dll`** import, only `api-ms-win-crt-*` forwarders resolving to the UCRT, an OS component on Windows 10. The bundled SQLite compiled through `cc` picks the flag up automatically via `CARGO_CFG_TARGET_FEATURE`, so nothing in the tree is on a mixed CRT. The absence of that import is the evidence that matters.

**RISK, recorded rather than closed:** WebView2 initialisation under a static CRT is proven only by a windowed smoke test, not by analysis. The release binary is confirmed to load and execute (it reaches its own provisioning guard and aborts there — `panic=abort`, `0xC0000409`, not a loader failure at `0xC0000135`), but that is before any window exists.

**NAMED FALLBACK, if a static CRT ever misbehaves:** bundle `vc_redist.x64.exe` alongside the installer and run it silently from an NSIS install hook. This is cheap, well-trodden, and reverses the decision without touching application code — which is why a smoke test is the right level of rigour here and a dedicated verification agent is not.

### NSIS is the only supported installer

`bundle.targets` is `["nsis"]`, not `"all"`. The MSI/WiX target was never verified end to end — the WiX toolchain download timed out twice — and **two half-verified installers are worse than one verified one**. If MSI is ever required, it returns with its own verification, tracked in `docs/backlog-m2.md`.
