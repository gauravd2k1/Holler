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
  // Milestone 4 additions (ADR-018). RoleCode INVENTORY_MANAGER has existed
  // since Milestone 1 and mapped to no permissions at all.
  "inventory.manage",
  "inventory.count",
  "recipe.manage",
  // Rides along, and lands WITH its enforced check on the GSTIN write path.
  // backend/internal/compliance gated those writes on outlet.manage, so
  // whoever could rename a table could set the GSTIN printed on every invoice.
  // A permission defined and never checked is a documented obligation dressed
  // as structural enforcement.
  //
  // wastage.approve is deliberately NOT here: the approval workflow moves to
  // M5 with the append-only approval row that enforces it, because a mutable
  // approval flag on an append-only row is a contradiction. Wastage RECORDING
  // ships in M4 under inventory.manage.
  "billing.manage",
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

// Added at 0.4.3 (ADR-017 amendment). The device credential hash, shipped to
// an enrolled edge so a LAN handshake can be verified WITH THE UPLINK DOWN.
//
// This is the ADR-011 pattern applied to devices, not a new idea: /sync/config
// already ships Argon2id password and PIN hashes so a cashier can log in
// offline, for exactly the same reason. A kitchen screen that reconnects
// during a WAN outage — a browser reload, a tablet waking, a router blip — must
// still be able to authenticate, because ticket visibility is a core operation
// and CLAUDE.md's premise is that core operations run without internet.
//
// The PLAINTEXT token still never leaves the cloud. Only the Argon2id hash
// syncs, it travels only on /sync/config to an already-enrolled node, and the
// edge SQLite file holding it is encrypted at rest (ADR-011). Never logged,
// never in an audit value — token_hash and device_token_hash are both in
// AUDIT_REDACTED_FIELDS above.
export const EdgeDeviceCredentialSchema = z.object({
  credential_id: z.string().uuid(),
  device_id: z.string().uuid(),
  tenant_id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  // Argon2id encoded string over the token secret — a VERIFIER, never a
  // bearer token. Named credential_hash rather than token_hash deliberately:
  // the drift guard treats "token_hash" as bearer material, and it is right to,
  // so the field that holds something you check against says so.
  credential_hash: z.string(),
  // A device kind is carried so the LAN server can refuse, say, a PRINTER_BRIDGE
  // credential presented by something claiming to be a KDS.
  device_kind: z.enum(["POS", "KDS", "WAITER", "PRINTER_BRIDGE"]),
  // Both nullable. A revoked or expired credential still SYNCS — the edge must
  // learn that it is dead, which it cannot do if the row simply vanishes while
  // the uplink is down. The edge rejects on these fields; it does not infer
  // liveness from absence.
  revoked_at: z.string().datetime().nullable(),
  expires_at: z.string().datetime().nullable(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type EdgeDeviceCredential = z.infer<typeof EdgeDeviceCredentialSchema>;

// Field names that must never be written into audit_event old_value/new_value
// or emitted on the wire. The audit helper in each runtime redacts these
// (ADR-011); the drift tests assert the list matches Go.
//
// token_hash added at 0.2.1 alongside the refresh_token table: a refresh-token
// row must never reach an audit_event value either.
export const AUDIT_REDACTED_FIELDS = [
  "password_hash",
  "pin_hash",
  "token_hash",
  // Added at 0.4.3 (ADR-017 amendment). The device credential's column IS
  // named token_hash, so it was already matched by the entry above — this is
  // belt-and-braces for any writer that spells the field with its qualifier,
  // and it lets the auth package drop the local supplement it had to add at
  // 0.4.1 when this list was still frozen without it.
  "device_token_hash",
  // The edge-cached verifier (0.4.3). Belongs in the sweep for the same reason
  // password_hash does: it is credential material at rest on the shop floor.
  "credential_hash",
] as const;

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
