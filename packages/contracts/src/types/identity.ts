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

// Field names that must never be written into audit_event old_value/new_value
// or emitted on the wire. The audit helper in each runtime redacts these
// (ADR-011); the drift tests assert the list matches Go.
export const AUDIT_REDACTED_FIELDS = ["password_hash", "pin_hash"] as const;

export const AuditEventSchema = z.object({
  id: z.string().uuid(),
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
