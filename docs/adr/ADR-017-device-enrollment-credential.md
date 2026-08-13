# ADR-017 — Device enrollment credential

**Status:** Accepted (contract shape only — the mechanism is Milestone 3 track T1/T4)
**Date:** 2026-08-12
**Extends:** ADR-011 (edge credential cache), ADR-012 (host-based tenant resolution), ADR-015 (edge credential sync and LAN transport).

## Context

`docs/backlog-m2.md` carries device enrollment as a **hard trigger: blocks any pilot deployment**. Three holes are one missing mechanism:

1. **Edge sync worker** — `tenant_id` and `device_id` are supplied at construction with nothing to verify them against, so a mis-enrolled node silently mislabels every outbound envelope.
2. **`GET /sync/config`** — the one route carrying Argon2id password and PIN hashes is gated on an ordinary human bearer token with `user.manage`. An enrolled edge node and a logged-in browser session are indistinguishable to the backend. The frozen OpenAPI description for `EdgeUserCacheEntry` already claims delivery is "only over TLS, only to an enrolled edge node"; two thirds of that sentence is currently aspirational.
3. **KDS LAN port** — the handshake verifies only that a `device_id`/`outlet_id` pair matches a registered device row, with no TLS. Device ids are UUIDs, not secrets, and they travel in the WebSocket query string. On a flat restaurant LAN, anyone who captures one can drive `set_kot_status`: marking food SERVED when it never left the kitchen, or CANCELLED on a live ticket.

M3 is why this is now urgent rather than merely outstanding. Invoices, payments and cash-shift records are the first data where an unauthenticated device is a **financial** problem rather than an operational one. Building a billing surface on an unauthenticated edge means the enrollment work later has to re-secure a larger surface than it does today.

## Decision

### 1. A per-device enrolled credential, stored hashed, cloud-only

`device_credential` holds an Argon2id hash of a per-device token. It is **cloud-only**: no SQLite mirror, and deliberately **not** an `AggregateType` — the `refresh_token` precedent. Giving it a sync direction would ship credential material to the very device whose identity it establishes.

The plaintext token is returned **once**, at enrollment, and never again. It never appears in an `audit_event` old/new value; the audit helper's redact list gains `device_token_hash` alongside `password_hash`, `pin_hash` and `token_hash`.

### 2. Closing it must close all three holes together

A credential presented on the cloud sync path but not the LAN handshake leaves the KDS port exactly as open as it is today. The three holes are one mechanism, and the backlog entry's minimum close is a per-device credential on **both** paths plus network-segmentation guidance in outlet setup documentation.

### 3. When verification turns on, the token moves out of the query string

`lan.ts` reserves an optional `device_token` handshake parameter (ADR-015), so enrollment lands as a **behaviour change — server strictness — not a contract shape change**. That is why this is a minor bump rather than a 0.5.0.

But: query-string carriage is acceptable **only while the token is unverified and therefore carries no authority**. The moment the value becomes a secret, it must move to an `Authorization` header or a first-frame auth message before the snapshot. A secret in a query string is a secret in every proxy and access log on the path.

**Closing enrollment without also moving the parameter would convert a documented non-secret into an undocumented leaked secret** — strictly worse than today, because the leak would be invisible.

### 4. `app_user.config_version` must bump on credential change

Currently it bumps only on create and role change. So a password or PIN change would never reach the edge cache, and a cashier would keep authenticating offline with the old credential indefinitely — including a credential changed *because* it was compromised. In scope for the same track.

## Freezing scope (0.4.1, 2026-08-13)

The cloud half landed and passed its gate. Three routes exist in `backend/internal/outlet`: enroll, credential rotate, credential revoke. **Only `POST /devices/enroll` is frozen into `packages/contracts/openapi/openapi.yaml`.**

Enrollment is frozen because it has a consumer the moment a pilot install happens: a technician enrols the outlet's POS, and that request/response shape is the interface between install tooling and the backend. It is also the only response in the entire API that contains a credential token, which is worth pinning explicitly so the "returned exactly once, never readable again" property is part of the contract rather than an implementation habit.

Rotation and revocation stay **implemented but unfrozen**. They have no consumer yet — no admin UI calls them, and no pilot runbook describes rotating a credential in the field. Freezing a shape before anything consumes it risks freezing the wrong one, and the cost of that is high here: an unfreeze needs a version bump and an ADR, while an unfrozen internal route can simply change. They get frozen when the admin UI or the pilot runbook lands and tells us what the operator actually needs.

This is a deliberate asymmetry, not an oversight. Recorded so a later reader does not "complete" the set without a consumer to design against.

## Verification note (2026-08-13)

The cloud half passed its gate. Two properties were confirmed structurally rather than by assertion, and both are worth recording because they are easy to erode later:

- **No route reads a token back.** `VerifyToken` takes no tenant/outlet parameters, `deviceCredentialVerifyRow` is unexported so no external package can construct one, and the only two writers of the plaintext return it once by value. The guarantee lives in the type and package boundaries, not in a policy someone must remember.
- **Audit redaction is complete across packages.** `outlet.DeviceService` never redacts for itself — it calls the injected `auth.AuditRecorder`, whose single concrete implementation is the only writer of `AuditEvent` and always applies the redact list regardless of calling package. Today this is defence-in-depth (the device audit values contain no hash to begin with), which is exactly why it must not be quietly removed as unused.

## Consequences

- `GET /sync/config` stops accepting a human bearer token. Any existing caller relying on that is broken deliberately; it was the hole.
- An edge node that cannot present a valid credential fails to sync rather than degrading quietly. Whoever wires the sync worker must also treat an **empty `users` array as an error rather than an empty set** — an edge that receives zero cached credentials today gets no signal at all, only downstream login failures.
- Enrollment must be performable by a technician on a flaky connection at install time (ADR-013), so the flow cannot assume reliable connectivity mid-enrollment.

## Status note

This ADR fixes the **contract shape and the rules**. The mechanism — the enrollment route, credential verification on both paths, and the query-string move — is Milestone 3 tracks T1 and T4. Until those pass their gate, the three holes remain open and no pilot deployment is defensible.
