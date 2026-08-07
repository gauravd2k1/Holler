# ADR-012 — Host-based tenant resolution, with X-Tenant-ID as a time-boxed interim

Status: Accepted (2026-08-07)
Relates to: ADR-006 (PostgreSQL multi-tenancy), ADR-011 (identity and RBAC contracts), docs/spec/security-rbac.md

## Context

`POST /auth/login` is the one endpoint that runs before any principal exists, so it cannot take tenant identity from an authenticated context — there isn't one yet. The Milestone 1 auth implementation resolves the tenant from a client-supplied `X-Tenant-ID` header.

The verification pass assessed this and found no user-enumeration leak today: the no-such-user path runs a dummy Argon2id verify and returns the identical error and status as a wrong-password attempt, so neither response content nor timing distinguishes them. The real exposure is different and forward-looking — an unauthenticated caller controls the value that scopes the lookup, so any future defence keyed on tenant alone (rate limit, lockout, anomaly counter) could be reset by rotating the header.

## Decision

**Host-based tenant resolution is the target.** Before Holler serves more than one tenant in production, the tenant is resolved server-side from the request host (`pune-fc.holler.app`, or a mapped custom domain), never from a client-supplied header. The host is attested by TLS SNI and certificate, so it is not freely forgeable by the caller, and it cannot be varied per-request to escape a counter.

**`X-Tenant-ID` remains for Milestone 1 only**, as an explicitly time-boxed interim, because host-based routing needs DNS, wildcard certificates and a reverse-proxy layer that the local WSL2 development stack does not have and that Milestone 1 does not otherwise require.

**Rate limiting lands now, not with the host change.** Login attempts are limited on the composite key IP + tenant. Keying on IP as well as tenant is the specific mitigation for the weakness above: rotating `X-Tenant-ID` no longer resets the attacker's budget, because the IP component of the key does not change. The limiter lives behind a small `platform/ratelimit` interface so the domain does not depend on a specific backing store; Redis is the initial implementation, since it is already in the stack, survives restart, and works across instances.

## Consequences

- Auth gains a rate-limited login path in Milestone 1. A limiter outage must fail closed for login, not open.
- The tenant-resolution seam is confined to one place in the auth context so that swapping header for host is a small, testable change rather than a diffuse one.
- When host-based resolution lands, `X-Tenant-ID` must be *rejected*, not merely ignored — an endpoint that silently accepts both leaves the forgeable path alive.
- Cross-tenant isolation tests keep asserting that tenant scoping comes from the resolved context and never from a request parameter or body, which holds under either mechanism.
