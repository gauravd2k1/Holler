# ADR-017 — Device enrollment credential

**Status:** Accepted (contract shape only — the mechanism is Milestone 3 track T1/T4)
**Date:** 2026-08-12
**Extends:** ADR-011 (edge credential cache), ADR-012 (host-based tenant resolution), ADR-015 (edge credential sync and LAN transport).

## Context

`docs/backlog.md` carries device enrollment as a **hard trigger: blocks any pilot deployment**. Three holes are one missing mechanism:

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

## Amendment — 0.4.3: the credential hash syncs to an enrolled edge (2026-08-13)

**This amends §1.** That section said `device_credential` is cloud-only, with no SQLite mirror. That is now too strict, and the reason is instructive.

The first edge implementation verified every new LAN connection by calling the cloud (`CloudConfigOracleVerifier`). Its verification gate ruled that a **blocker**: a browser reload, a tablet waking, or a router blip during a WAN outage left the kitchen screen unable to re-authenticate and receiving no tickets until connectivity returned. CLAUDE.md's premise is that core operations run without internet, and `docs/spec/kitchen.md` makes the KDS LAN-first. Ticket visibility is a core operation.

So the mechanism as first built traded an **unauthenticated-but-available** KDS for an **authenticated-but-unavailable-offline** one — worse at precisely the moment local-first exists to protect. Closing a security hole by breaking the product's central guarantee is not closing it.

### The decision

The device credential's **Argon2id hash** now syncs to an enrolled edge on `GET /sync/config` and LAN handshakes are verified locally.

This is not a new idea; it is **the ADR-011 pattern applied to devices**. `/sync/config` already ships password and PIN hashes so a cashier can log in offline, for exactly the same reason and with exactly the same containment. Devices now get the same treatment:

- The **plaintext token still never leaves the cloud.** Only the verifier syncs.
- It travels only on `/sync/config`, only over TLS, only to an already-enrolled node.
- The edge SQLite file holding it is **encrypted at rest** (ADR-011) — never copy it or its backups anywhere unencrypted.
- The field is named **`credential_hash`, not `token_hash`**, because it holds something you *check a presented token against*, never a bearer token you could replay. The contract drift guard treats `token_hash` as bearer material and is right to; the naming now matches the semantics rather than fighting the guard.
- `credential_hash` and `device_token_hash` are both in `AUDIT_REDACTED_FIELDS`, so neither can reach an audit value or a log line.
- `edge_device_credential.json` is registered as a deliberate credential-bearing fixture in both drift sweeps — which required this ADR, exactly as those guards demand.

**A revoked or expired credential still syncs.** The edge must be able to learn that a credential is dead, and it cannot learn that from a row's absence while the uplink is down: absence is indistinguishable from "not yet synced". Rejection is decided by `revoked_at`/`expires_at`, never by whether a row exists.

### Explicitly rejected: verify-online-then-cache-with-a-TTL

Caching a successful verification for a window was considered and **rejected**. It leaves a cold-start hole: a screen that has never connected while online cannot join the LAN at all. That is precisely the offline-first failure this architecture exists to prevent — the outlet whose uplink is down on the morning a new kitchen tablet arrives is exactly the outlet that most needs the tablet to work. A mechanism that works only for devices lucky enough to have been online before is not an offline-capable mechanism.

## Freezing scope (0.4.1, 2026-08-13)

The cloud half landed and passed its gate. Three routes exist in `backend/internal/outlet`: enroll, credential rotate, credential revoke. **Only `POST /devices/enroll` is frozen into `packages/contracts/openapi/openapi.yaml`.**

Enrollment is frozen because it has a consumer the moment a pilot install happens: a technician enrols the outlet's POS, and that request/response shape is the interface between install tooling and the backend. It is also the only response in the entire API that contains a credential token, which is worth pinning explicitly so the "returned exactly once, never readable again" property is part of the contract rather than an implementation habit.

Rotation and revocation stay **implemented but unfrozen**. They have no consumer yet — no admin UI calls them, and no pilot runbook describes rotating a credential in the field. Freezing a shape before anything consumes it risks freezing the wrong one, and the cost of that is high here: an unfreeze needs a version bump and an ADR, while an unfrozen internal route can simply change. They get frozen when the admin UI or the pilot runbook lands and tells us what the operator actually needs.

This is a deliberate asymmetry, not an oversight. Recorded so a later reader does not "complete" the set without a consumer to design against.

## Verification note (2026-08-13)

The cloud half passed its gate. Two properties were confirmed structurally rather than by assertion, and both are worth recording because they are easy to erode later:

- **No route reads a token back.** `VerifyToken` takes no tenant/outlet parameters, `deviceCredentialVerifyRow` is unexported so no external package can construct one, and the only two writers of the plaintext return it once by value. The guarantee lives in the type and package boundaries, not in a policy someone must remember.
- **Audit redaction is complete across packages.** `outlet.DeviceService` never redacts for itself — it calls the injected `auth.AuditRecorder`, whose single concrete implementation is the only writer of `AuditEvent` and always applies the redact list regardless of calling package. Today this is defence-in-depth (the device audit values contain no hash to begin with), which is exactly why it must not be quietly removed as unused.

## Amendment — 0.4.3: the device credential gates edge→cloud ingest (2026-08-13)

**This extends §2.** The first implementation gated `GET /sync/config` on the device credential and stopped there. The T4 verification gate found the consequence by tracing further than either side's own gate had: the **ingest routes** — order, table_session, kot — remained behind `auth.Authenticate`, which verifies HMAC-signed *human* JWTs. A device credential (`<credential_id>.<secret>`) cannot satisfy that signature check.

So a correctly enrolled edge node could pull config and then have every envelope push rejected. Hole 1 was closed in form and not in effect: the worker would present a credential the ingest path could not evaluate.

**Decision: `DeviceAuthenticate` gates the ingest routes, and tenant/outlet resolve from the credential row.**

Ingest is edge→cloud replay by definition — the caller is always an enrolled device, never a browser — so accepting a human JWT there was never meaningful, and accepting *either* would reinstate exactly the ambiguity this ADR exists to remove, on the path that now carries money.

This also closes the remaining half of hole 1. §1 named the defect as a mis-enrolled node silently mislabelling every outbound envelope; verifying `outlet_id` against `/sync/config` narrowed that to outlet level but left `tenant_id` locally supplied and unverifiable. With ingest resolving tenant and outlet **from the credential**, an envelope's claimed `tenant_id` is checked against what the credential actually resolves to rather than trusted. A wrong tenant with a right outlet is no longer representable.

## Consequences

- `GET /sync/config` stops accepting a human bearer token. Any existing caller relying on that is broken deliberately; it was the hole.
- An edge node that cannot present a valid credential fails to sync rather than degrading quietly. Whoever wires the sync worker must also treat an **empty `users` array as an error rather than an empty set** — an edge that receives zero cached credentials today gets no signal at all, only downstream login failures.
- Enrollment must be performable by a technician on a flaky connection at install time (ADR-013), so the flow cannot assume reliable connectivity mid-enrollment.

## Status note

This ADR fixes the **contract shape and the rules**. The mechanism — the enrollment route, credential verification on both paths, and the query-string move — is Milestone 3 tracks T1 and T4. Until those pass their gate, the three holes remain open and no pilot deployment is defensible.

## Addendum — 0.4.5: per-row `config_version` on `device_credential` (2026-08-15)

`device_credential` was the only config-bearing table in the contract without its own `config_version`. `station`, `printer`, `menu_item_station` and `restaurant_table` all declare `config_version INTEGER NOT NULL` and are written with the outlet's freshly bumped value; `device_credential` had none, so `DeviceService.ListEdgeDeviceCredentials` substituted the **outlet's** current version into every row it returned.

That was correctness-preserving and coarse. `since_version` filtering still held — an edge at the outlet's current version received nothing, one behind received everything — but it was outlet-granular where every sibling is row-granular. An unrelated config change elsewhere in the outlet (a renamed table, a new station) re-sent the entire credential collection, Argon2id hashes included, to every enrolled node.

`postgres/0010_device_credential_config_version.sql` adds the column, backfills each row from its outlet's current version, sets `NOT NULL`, and indexes `(outlet_id, config_version)`.

**The wire type does not change.** `EdgeDeviceCredential` has declared `config_version` since 0.4.3; only the *source* of the value changes. TS and Go mirrors are untouched and the drift tests are unaffected — which is why this is a schema-only bump.

**Two consequences recorded rather than discovered later:**

- **The write order inverts.** The credential row must carry the value the outlet is bumped *to*, so `BumpOutletConfigVersion` must run first and `InsertCredential` second, with the returned version. `RevokeActiveCredential` must **also** stamp the new version alongside `revoked_at` — a revocation that does not advance its own row's version would never reach the edge, and that is the more dangerous half: the edge would keep honouring a credential the cloud has revoked. This is only safe because the T13 retry (`eef7464`) put all three of enroll/rotate/revoke inside one `WithTx`. Before that commit this migration would have introduced a race rather than removed one.
- **One extra full send.** Any edge whose watermark sits below its outlet's current version re-receives every credential once on its first pull after the migration. Correct — it would have received them all anyway under the old filter — and it happens exactly once.

A credential whose `outlet_id` does not resolve is left NULL by the backfill and fails the `SET NOT NULL` loudly. That is intended: a credential with no resolvable outlet is a data defect worth stopping a migration for, not something to paper over with a default.

### Self-review against the rubric

| Check | Finding |
|---|---|
| App-generated UUIDv7, no DB-side defaults | N/A — no new identifiers. |
| No nullable columns in primary keys | Clean — not part of any PK; nullable only transiently during backfill. |
| Single authority per §50.1 | Clean. `device_credential` stays cloud-only and gains no sync direction; `config_version` is cloud-assigned and edge-read. |
| No credential material in audit/logs/wire | Clean — unchanged. The hash still travels only on `/sync/config` to an enrolled node. |
| Tenant-scoped uniqueness | N/A — no new uniqueness constraint. |
| Additive + version bump + ADR | Additive. 0.4.4 → 0.4.5, this addendum. |
