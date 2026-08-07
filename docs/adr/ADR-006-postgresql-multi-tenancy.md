# ADR-006: PostgreSQL Multi-Tenancy Model

## Context
Holler Cloud serves many organisations/brands/outlets from shared infrastructure. Tenant data must never leak across organisations, and the schema must support the Organisation → Brand → Outlet hierarchy (docs/spec/multi-outlet.md) without duplicating structure per tenant.

## Decision
Use a **shared PostgreSQL database with tenant_id-scoped tables** (row-level multi-tenancy), enforced at the application/query layer with automated cross-tenant access tests (docs/spec/security-rbac.md), rather than database-per-tenant or schema-per-tenant. All tenant-owned tables carry `tenant_id` (and `outlet_id` where applicable) as a mandatory, indexed column.

## Alternatives
- **Database-per-tenant**: rejected — operationally heavy (migrations × N tenants, connection pooling complexity) for a product targeting many small/medium restaurant tenants.
- **Schema-per-tenant**: rejected — migration fan-out and connection/catalog overhead still scale with tenant count; row-level scoping with strong query discipline is simpler to operate and secure.

## Consequences
- Every query touching a tenant-owned table must be scoped by `tenant_id`; this is enforced via repository-layer conventions and covered by dedicated cross-tenant isolation tests (docs/spec/security-rbac.md §Tenant isolation).
- Backups/PITR and migrations apply uniformly across all tenants, simplifying operations (§59, §78).
- Revisit only if a specific tenant's compliance/isolation requirements demand physical separation.
