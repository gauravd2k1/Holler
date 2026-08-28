# ADR-015 — The edge credential-sync exception, and freezing the KDS LAN transport

**Status:** Accepted
**Date:** 2026-08-10
**Contracts version:** 0.3.0 → 0.3.1
**Amends:** ADR-008 (contracts frozen), ADR-011 (identity/RBAC, credential handling), ADR-014 (M2 kitchen contracts)

## Context

Two unrelated gaps surfaced while executing Milestone 2, both found by verification passes rather than by tests.

**1. `GET /sync/config` returned an empty `users` array.** The route was built, its other eight fields correct, but `users` could not be populated: `packages/contracts/go/identity.go` carried no `EdgeUserCacheEntry` mirror — deliberately, under a file header reading *"no struct here carries credential material"* — and `internal/auth` exported nothing returning a `password_hash`. The OpenAPI schema for the entry had existed since 0.2.2; only the language mirrors were missing.

The consequence is larger than Milestone 2. ADR-011 makes the edge's cached Argon2id hashes the mechanism by which offline login works at all, and **Milestone 1's acceptance criterion is that a cashier can create orders with the network disconnected.** With `users` always empty, a freshly-synced edge node caches zero credentials and can authenticate nobody. Offline login held only against dev-seeded data — never through any production path. It also failed silently: the edge receives a well-formed response and has no signal it got nothing.

**2. `lan.ts` froze message shapes but not transport.** The Rust server in `edge/device` and the TypeScript client in `apps/kds` were built concurrently against the same frozen contract, by different agents, and did not interoperate. The server took connection identity from handshake query params and answered 400 without them; the client connected to its configured URL verbatim and had no outlet identity at all.

Neither implementation violated the contract. That is the point: a contract that pins payloads and says nothing about the handshake does not pin the interface.

## Decision

### 1. `EdgeUserCacheEntry` — one named exception to the no-credential-material rule

Add the Go and TypeScript mirrors of the already-frozen OpenAPI schema. Eleven fields plus `schema_version`: `id`, `tenant_id`, `outlet_id`, `email`, `full_name`, `password_hash`, `pin_hash`, `is_active`, `permissions`, `config_version`, `updated_at`.

**The rule this suspends, and exactly how far.** ADR-011 and the contract review rubric state that credential material never appears on the wire, in a log, or in an audit value. That line stays in force everywhere except this one type on this one route. It is written as an exception rather than a softening of the rule so the rubric stays mechanically enforceable: any *other* type acquiring a hash field still fails review, and the drift suites now sweep every fixture except two named carriers rather than a hard-coded list, so a new fixture is covered automatically.

**Verifiers, not bearers.** Both hashes let a holder *check* a secret; neither can be *presented* as proof of identity. A stolen cache entry does not become a session. This is the property that makes the exception containable, and it is why no token, `token_hash`, session id, or refresh material may ever join this type — a drift test asserts their absence by name.

**`pin_hash` stays, and is arguably the more important field.** A PIN pad, not an email box, is the primary offline login at a point of sale: it is what a cashier actually uses at the start of a shift. It is Argon2id, verifier-not-bearer, and gets identical containment to `password_hash`. Excluding it would have left the most-used offline credential unable to sync while keeping the less-used one — and would have diverged the mirrors from the frozen OpenAPI schema on their first field.

**Not an `AggregateType`.** It never syncs up. Listing it would promise a sync direction and invite a replay path that must not exist — the same reasoning that keeps `refresh_token` (cloud-only) and `print_job` (edge-local) out of that enum. Negative tests in both languages assert it stays out.

**Storage.** The edge holds it only in the encrypted-at-rest database (ADR-011).

**Fixtures pin both nullable states.** Two fixtures in both language suites: one with an encoded `pin_hash`, one with `pin_hash: null`. Nullable handling is precisely where a mirror silently drops a field, and until now the correctness of nullable round-tripping in this codebase was a read-verified claim from Milestone 1, never an executed one. The null fixture additionally asserts the key is *present and null* rather than absent, because an omitted key round-trips to a different object.

**`schema_version` reconciled.** The frozen OpenAPI schema omitted it while every sibling wire type carries one. The mirrors would have drifted from the schema on their first field. Added to the schema rather than dropped from the mirrors, which would have left this the only unversioned shape on the wire.

### 2. The LAN transport is frozen; `device_id` is identity, not authentication

`lan.ts` now specifies endpoint (`/kds`), framing (one JSON text frame per message, no sub-protocol, no envelope, no handshake message beyond the socket opening), the handshake parameters, and the 400 rejection.

The distinction that matters: **`device_id` identifies a screen; it does not authenticate one.** It is a UUID, not a secret, and it travels in a query string that lands in proxy and access logs. The server today accepts any `device_id` matching a registered row, so anyone reaching the port with a captured id can drive ticket transitions. The contract states this as a known unclosed gap — tracked under Device enrollment, blocking any pilot deployment — rather than blessing the current behaviour by documenting it neutrally.

**`device_token` is reserved now so enrollment lands later without a major bump.** It is optional and unverified: clients may send it, servers may ignore it. When enrollment exists, only the server's strictness changes — the parameter is already part of the frozen handshake, so the transition is a behaviour change needing a minor bump and an ADR note, not 0.4.0 and not a client rewrite.

**When verification turns on, `device_token` must move out of the query string** — an `Authorization` header or a first-frame auth message. A secret in a query string is a secret in a log file. Query-string carriage is tolerable only while the value is worthless.

## Consequences

Populating `users` **makes a latent exposure real.** Today the array is empty, so the security gap is theoretical. Once the `auth` export lands, `/sync/config` ships Argon2id hashes — and there is no edge enrollment mechanism anywhere in the backend, so that route is gated on an ordinary human bearer token with `user.manage`. An enrolled edge node and a logged-in browser session are indistinguishable to the server.

The frozen schema's own description already claims delivery is *"only over TLS, only to an enrolled edge node."* Two thirds of that sentence is currently aspirational. **This ADR does not fix it, and the mirrors landing here do not make it true.** Device enrollment carries a hard trigger in `docs/backlog.md`: it blocks any pilot deployment, including a single outlet. Shipping credential sync to a real restaurant before enrollment exists would be a straightforward security failure, and naming it here is the point of the entry.

The `auth` export that populates the array is builder work, gated on a verifier condition that `password_hash` and `pin_hash` serialize in the `/sync/config` handler and nowhere else across `backend/` — asserted by grep, not by review.

Freezing the transport is not purely additive in effect: it retroactively made the KDS client wrong, which is why the client changed rather than the server. Identity from the connection rather than from a payload field matches ADR-014 §6, and a payload `device_id` a client can set to anything is not an identity.

## Alternatives considered

**Populate `users` only when an enrolled device asks.** Rejected for now, not on merit — it is the right end state, but it presumes an enrollment mechanism that does not exist. Building the gate before the thing it gates would have produced a fiction.

**Drop `pin_hash` to narrow the exception.** Rejected: it excludes the credential a cashier actually uses at a POS while keeping the one they rarely do, and diverges the mirrors from the frozen schema.

**Document the transport in a spec file instead of the contract.** Rejected: the two ends failed to interoperate *because* the transport was not in the contract. Recording it somewhere non-binding would repeat the failure with an extra document.

**Make `device_token` required immediately.** Rejected: nothing can issue one. A required parameter no client can populate breaks the LAN hop today to describe a security property that will not exist until enrollment ships.
