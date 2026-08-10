// Identity & RBAC contracts — Milestone 1 (ADR-011). Mirrors go/identity.go.
//
// SECURITY: the wire shape carries NO credential material. password_hash and
// pin_hash exist only in the database rows (postgres/0002, sqlite/0002) and
// must never appear in an API response, a log line, or an audit_event value.

import { z } from "zod";

// docs/spec/security-rbac.md §Roles — the 15 roles, tenant-seeded.
export const RoleCodeSchema = z.enum([
  "PLATFORM_SUPER_ADMIN",
  "ORGANISATION_OWNER",
  "BRAND_ADMIN",
  "REGIONAL_MANAGER",
  "OUTLET_MANAGER",
  "ACCOUNTANT",
  "INVENTORY_MANAGER",
  "PURCHASE_MANAGER",
  "CHEF",
  "KITCHEN_STAFF",
  "CAPTAIN",
  "WAITER",
  "CASHIER",
  "DELIVERY_STAFF",
  "AUDITOR",
]);
export type RoleCode = z.infer<typeof RoleCodeSchema>;

// docs/spec/security-rbac.md §Permissions. Only permissions whose owning
// context exists in Milestone 1 are enumerated; later milestones extend this
// list through the same orchestrator-serialized process (ADR-008).
export const PermissionSchema = z.enum([
  "order.create",
  "order.modify",
  "order.cancel",
  "order.void",
  "menu.manage",
  "table.manage",
  "outlet.manage",
  "user.manage",
]);
export type Permission = z.infer<typeof PermissionSchema>;

export const RoleSchema = z.object({
  id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  code: RoleCodeSchema,
  name: z.string(),
  permissions: z.array(PermissionSchema),
  schema_version: z.literal(1),
});
export type Role = z.infer<typeof RoleSchema>;

// A role held by a user, optionally narrowed to one outlet. outlet_id null =
// tenant-wide (postgres user_role.outlet_id IS NULL).
export const RoleAssignmentSchema = z.object({
  id: z.string().uuid(),
  role_id: z.string().uuid(),
  role_code: RoleCodeSchema,
  outlet_id: z.string().uuid().nullable(),
});
export type RoleAssignment = z.infer<typeof RoleAssignmentSchema>;

export const AppUserSchema = z.object({
  id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  email: z.string().email(),
  full_name: z.string(),
  is_active: z.boolean(),
  roles: z.array(RoleAssignmentSchema),
  config_version: z.number().int(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type AppUser = z.infer<typeof AppUserSchema>;

// What an authenticated session resolves to — the shape RBAC middleware and
// the POS both check permissions against. Identical online and offline.
export const AuthenticatedPrincipalSchema = z.object({
  user_id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  full_name: z.string(),
  permissions: z.array(PermissionSchema),
  authenticated_offline: z.boolean(), // true when verified against the edge user cache
  schema_version: z.literal(1),
});
export type AuthenticatedPrincipal = z.infer<typeof AuthenticatedPrincipalSchema>;

// EdgeUserCacheEntry — the ONE exception to the no-credential-material rule in
// this file's header, added at 0.3.1 (ADR-015). Every other type here, and the
// rubric line forbidding credential material on the wire, stays in force.
//
// WHY IT EXISTS: ADR-011 requires a cashier to log in with no internet, which
// is only possible if the edge holds verifiable credentials locally. So
// exactly one route ships them. Before 0.3.1 the Go mirror was deliberately
// absent and GET /sync/config returned an empty users array — meaning offline
// login worked only against dev-seeded data and never a real sync.
//
// WHAT IT MAY CARRY: Argon2id hashes and flattened permission claims. Nothing
// else. No refresh token, no token_hash, no session id, no bearer material of
// any kind — a stolen cache entry must not become a session anywhere. Both
// hashes here are VERIFIERS, not bearers: possessing one lets you check a
// password or PIN, never present it as proof of identity.
//
// WHERE IT MAY GO: the `users` array of GET /sync/config, and nowhere else.
// Not another route response, not an event payload, not a log line, not an
// audit value. Deliberately NOT an AggregateType — it never syncs up, and
// giving it a direction would invite a replay path that must not exist (the
// refresh_token precedent, 0.2.1). The edge stores it only in the
// encrypted-at-rest database (ADR-011).
export const EdgeUserCacheEntrySchema = z.object({
  id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  email: z.string().email(),
  full_name: z.string(),
  password_hash: z.string(), // Argon2id encoded string; never logged
  // Argon2id, null when no PIN is set. A PIN pad — not an email box — is the
  // primary offline login at a POS, so this is the field that actually carries
  // the shift, and it gets exactly the containment password_hash gets.
  pin_hash: z.string().nullable(),
  is_active: z.boolean(),
  // Role CLAIMS, pre-flattened. The edge has no role table by design: the
  // `roles` field was dropped from /sync/config at 0.2.2 for promising storage
  // that does not exist. Permissions arrive already resolved.
  permissions: z.array(PermissionSchema),
  config_version: z.number().int(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type EdgeUserCacheEntry = z.infer<typeof EdgeUserCacheEntrySchema>;

// Field names that must never be written into audit_event old_value/new_value
// or emitted on the wire. The audit helper in each runtime redacts these
// (ADR-011); the drift tests assert the list matches Go.
//
// token_hash added at 0.2.1 alongside the refresh_token table: a refresh-token
// row must never reach an audit_event value either.
export const AUDIT_REDACTED_FIELDS = ["password_hash", "pin_hash", "token_hash"] as const;

export const AuditEventSchema = z.object({
  id: z.string().uuid(),
  // Non-null, matching audit_event.tenant_id in postgres/0002. Corrected at
  // 0.2.1 — the 0.2.0 type omitted it and drifted from the table.
  tenant_id: z.string().uuid(),
  outlet_id: z.string().uuid().nullable(),
  actor_user_id: z.string().uuid().nullable(),
  device_id: z.string().uuid().nullable(),
  action: z.string(), // 'order.cancel', 'user.role.assign', ...
  entity_type: z.string(),
  entity_id: z.string().uuid().nullable(),
  old_value: z.record(z.unknown()).nullable(),
  new_value: z.record(z.unknown()).nullable(),
  reason: z.string().nullable(),
  occurred_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type AuditEvent = z.infer<typeof AuditEventSchema>;
