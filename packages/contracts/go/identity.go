// Identity & RBAC contracts — Milestone 1 (ADR-011). Mirrors
// src/types/identity.ts.
//
// SECURITY: no struct here carries credential material. PasswordHash/PinHash
// live only in database rows and must never be marshaled onto the wire, into a
// log line, or into an AuditEvent value map.
package contracts

import "time"

type RoleCode string

// docs/spec/security-rbac.md §Roles.
const (
	RoleCodePlatformSuperAdmin RoleCode = "PLATFORM_SUPER_ADMIN"
	RoleCodeOrganisationOwner  RoleCode = "ORGANISATION_OWNER"
	RoleCodeBrandAdmin         RoleCode = "BRAND_ADMIN"
	RoleCodeRegionalManager    RoleCode = "REGIONAL_MANAGER"
	RoleCodeOutletManager      RoleCode = "OUTLET_MANAGER"
	RoleCodeAccountant         RoleCode = "ACCOUNTANT"
	RoleCodeInventoryManager   RoleCode = "INVENTORY_MANAGER"
	RoleCodePurchaseManager    RoleCode = "PURCHASE_MANAGER"
	RoleCodeChef               RoleCode = "CHEF"
	RoleCodeKitchenStaff       RoleCode = "KITCHEN_STAFF"
	RoleCodeCaptain            RoleCode = "CAPTAIN"
	RoleCodeWaiter             RoleCode = "WAITER"
	RoleCodeCashier            RoleCode = "CASHIER"
	RoleCodeDeliveryStaff      RoleCode = "DELIVERY_STAFF"
	RoleCodeAuditor            RoleCode = "AUDITOR"
)

type Permission string

// Milestone 1 permission set — extended per milestone by the orchestrator.
const (
	PermissionOrderCreate  Permission = "order.create"
	PermissionOrderModify  Permission = "order.modify"
	PermissionOrderCancel  Permission = "order.cancel"
	PermissionOrderVoid    Permission = "order.void"
	PermissionMenuManage   Permission = "menu.manage"
	PermissionTableManage  Permission = "table.manage"
	PermissionOutletManage Permission = "outlet.manage"
	PermissionUserManage   Permission = "user.manage"
)

type Role struct {
	ID            string       `json:"id"`
	TenantID      string       `json:"tenant_id"`
	Code          RoleCode     `json:"code"`
	Name          string       `json:"name"`
	Permissions   []Permission `json:"permissions"`
	SchemaVersion int          `json:"schema_version"`
}

// RoleAssignment ties a role to a user, optionally narrowed to one outlet.
// OutletID nil = tenant-wide.
type RoleAssignment struct {
	ID       string   `json:"id"`
	RoleID   string   `json:"role_id"`
	RoleCode RoleCode `json:"role_code"`
	OutletID *string  `json:"outlet_id"`
}

type AppUser struct {
	ID            string           `json:"id"`
	TenantID      string           `json:"tenant_id"`
	Email         string           `json:"email"`
	FullName      string           `json:"full_name"`
	IsActive      bool             `json:"is_active"`
	Roles         []RoleAssignment `json:"roles"`
	ConfigVersion int              `json:"config_version"`
	CreatedAt     time.Time        `json:"created_at"`
	UpdatedAt     time.Time        `json:"updated_at"`
	SchemaVersion int              `json:"schema_version"`
}

// AuthenticatedPrincipal is what a session resolves to — identical whether the
// credential was verified against PostgreSQL or the edge user cache.
type AuthenticatedPrincipal struct {
	UserID               string       `json:"user_id"`
	TenantID             string       `json:"tenant_id"`
	OutletID             string       `json:"outlet_id"`
	FullName             string       `json:"full_name"`
	Permissions          []Permission `json:"permissions"`
	AuthenticatedOffline bool         `json:"authenticated_offline"`
	SchemaVersion        int          `json:"schema_version"`
}

// EdgeUserCacheEntry is the ONE exception to this file's no-credential-material
// header rule, added at 0.3.1 (ADR-015). Mirrors src/types/identity.ts.
//
// ADR-011 requires offline cashier login, which is only possible if the edge
// holds verifiable credentials locally, so exactly one route ships them. This
// mirror was deliberately absent until 0.3.1, which left /sync/config returning
// an empty users array — offline login worked only against dev-seeded data.
//
// May carry Argon2id hashes and flattened permission claims, nothing else: no
// refresh token, no TokenHash, no session id, no bearer material. Both hashes
// are VERIFIERS, not bearers — holding one lets you check a secret, never
// present it as proof of identity.
//
// Travels in the users array of GET /sync/config and nowhere else: no other
// route, no event payload, no log line, no audit value. Deliberately NOT an
// AggregateType — it never syncs up, and a direction would invite a replay path
// that must not exist (the refresh_token precedent). The edge stores it only in
// the encrypted-at-rest database.
type EdgeUserCacheEntry struct {
	ID           string `json:"id"`
	TenantID     string `json:"tenant_id"`
	OutletID     string `json:"outlet_id"`
	Email        string `json:"email"`
	FullName     string `json:"full_name"`
	PasswordHash string `json:"password_hash"` // Argon2id encoded string; never logged
	// Argon2id, nil when no PIN is set. A PIN pad — not an email box — is the
	// primary offline login at a POS, so this field carries the shift and gets
	// exactly the containment PasswordHash gets.
	PinHash  *string `json:"pin_hash"`
	IsActive bool    `json:"is_active"`
	// Role claims, pre-flattened. The edge has no role table by design — the
	// roles field was dropped from /sync/config at 0.2.2.
	Permissions   []Permission `json:"permissions"`
	ConfigVersion int          `json:"config_version"`
	UpdatedAt     time.Time    `json:"updated_at"`
	SchemaVersion int          `json:"schema_version"`
}

// AuditRedactedFields must never appear in AuditEvent OldValue/NewValue or on
// the wire. Mirrors AUDIT_REDACTED_FIELDS in src/types/identity.ts; the audit
// helper strips these keys before persisting (ADR-011).
//
// token_hash added at 0.2.1 alongside the refresh_token table: a refresh-token
// row must never reach an audit_event value either.
var AuditRedactedFields = []string{
	"password_hash",
	"pin_hash",
	"token_hash",
	// Added at 0.4.3 (ADR-017 amendment). The device credential column IS named
	// token_hash and was already matched above; this covers a qualified spelling
	// and lets internal/auth drop the local supplement it needed at 0.4.1 when
	// this list was frozen without it.
	"device_token_hash",
	// The edge-cached verifier (0.4.3), in the sweep for the same reason
	// password_hash is: credential material at rest on the shop floor.
	"credential_hash",
}

// EdgeDeviceCredential is the device credential hash shipped to an enrolled
// edge so a LAN handshake can be verified WITH THE UPLINK DOWN. Added at 0.4.3
// (ADR-017 amendment).
//
// The ADR-011 pattern applied to devices: /sync/config already ships Argon2id
// password and PIN hashes so a cashier can log in offline. A kitchen screen
// reconnecting during a WAN outage must likewise still authenticate, because
// ticket visibility is a core operation and core operations run without
// internet.
//
// The PLAINTEXT token never leaves the cloud. Only the hash syncs, only on
// /sync/config, only to an already-enrolled node, into a SQLite file encrypted
// at rest. Never logged, never in an audit value.
type EdgeDeviceCredential struct {
	CredentialID string `json:"credential_id"`
	DeviceID     string `json:"device_id"`
	TenantID     string `json:"tenant_id"`
	OutletID     string `json:"outlet_id"`
	// Argon2id encoded string over the token secret — a VERIFIER, never a
	// bearer token. Named CredentialHash rather than TokenHash deliberately:
	// the drift guard treats "token_hash" as bearer material, correctly, so the
	// field holding something you check against is named for that.
	CredentialHash string `json:"credential_hash"`
	// Carried so the LAN server can refuse a PRINTER_BRIDGE credential
	// presented by something claiming to be a KDS.
	DeviceKind string `json:"device_kind"`
	// Both nullable. A revoked or expired credential STILL SYNCS — the edge must
	// learn that it is dead, which it cannot do if the row vanishes while the
	// uplink is down. The edge rejects on these fields rather than inferring
	// liveness from absence.
	RevokedAt     *string `json:"revoked_at"`
	ExpiresAt     *string `json:"expires_at"`
	ConfigVersion int     `json:"config_version"`
	SchemaVersion int     `json:"schema_version"`
}

type AuditEvent struct {
	ID string `json:"id"`
	// TenantID is non-null, matching audit_event.tenant_id in postgres/0002.
	// Corrected at 0.2.1 — the 0.2.0 type omitted it and drifted from the table.
	TenantID      string                 `json:"tenant_id"`
	OutletID      *string                `json:"outlet_id"`
	ActorUserID   *string                `json:"actor_user_id"`
	DeviceID      *string                `json:"device_id"`
	Action        string                 `json:"action"`
	EntityType    string                 `json:"entity_type"`
	EntityID      *string                `json:"entity_id"`
	OldValue      map[string]interface{} `json:"old_value"`
	NewValue      map[string]interface{} `json:"new_value"`
	Reason        *string                `json:"reason"`
	OccurredAt    time.Time              `json:"occurred_at"`
	SchemaVersion int                    `json:"schema_version"`
}
