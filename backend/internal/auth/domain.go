// Package auth implements the security & RBAC bounded context
// (docs/spec/security-rbac.md): users, roles, permissions, sessions, RBAC
// enforcement and the audit helper other contexts call to record sensitive
// actions.
//
// This package mirrors the wire shapes of packages/contracts/go/identity.go
// (source of truth) without importing it directly: the backend Go module has
// no module linkage to packages/contracts/go (no go.work / replace
// directive), matching the pattern already used by backend/internal/menu.
// Field names, JSON tags and the Permission/RoleCode string literals below
// must stay byte-for-byte identical to identity.go — contract drift here is a
// bug in this package, not a reason to diverge.
package auth

import "time"

// Permission is a granted capability string, e.g. "order.create". Mirrors
// contracts.Permission.
type Permission string

// Milestone 1 permission set — mirrors the Permission constants in
// packages/contracts/go/identity.go verbatim. Do not add permissions outside
// this list; the M1 contract does not define them.
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

// AllM1Permissions lists every permission this milestone's contract defines,
// used to validate seed data never references an unknown permission.
var AllM1Permissions = []Permission{
	PermissionOrderCreate,
	PermissionOrderModify,
	PermissionOrderCancel,
	PermissionOrderVoid,
	PermissionMenuManage,
	PermissionTableManage,
	PermissionOutletManage,
	PermissionUserManage,
}

// RoleCode identifies one of the 15 roles of docs/spec/security-rbac.md
// §Roles. Mirrors contracts.RoleCode.
type RoleCode string

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

// AuditRedactedFields must never appear in an AuditEvent OldValue/NewValue or
// on the wire. Mirrors contracts.AuditRedactedFields (ADR-011).
var AuditRedactedFields = []string{"password_hash", "pin_hash"}

// Role is a role_permission-joined role row.
type Role struct {
	ID          string
	TenantID    string
	Code        RoleCode
	Name        string
	Permissions []Permission
}

// RoleAssignment ties a role to a user, optionally narrowed to one outlet.
// OutletID nil = tenant-wide.
type RoleAssignment struct {
	ID       string
	RoleID   string
	RoleCode RoleCode
	OutletID *string
}

// User is an app_user row plus its role assignments. It never carries
// PasswordHash/PinHash out of the repository layer — those stay inside the
// repository's own row-scanning code so a bug elsewhere in this package
// cannot accidentally serialize them.
type User struct {
	ID        string
	TenantID  string
	Email     string
	FullName  string
	IsActive  bool
	Roles     []RoleAssignment
	CreatedAt time.Time
	UpdatedAt time.Time
}

// AuthenticatedPrincipal is what a session resolves to. Mirrors
// contracts.AuthenticatedPrincipal.
type AuthenticatedPrincipal struct {
	UserID               string       `json:"user_id"`
	TenantID             string       `json:"tenant_id"`
	OutletID             string       `json:"outlet_id"`
	FullName             string       `json:"full_name"`
	Permissions          []Permission `json:"permissions"`
	AuthenticatedOffline bool         `json:"authenticated_offline"`
	SchemaVersion        int          `json:"schema_version"`
}

// HasPermission implements the minimal Principal interface other bounded
// contexts (e.g. backend/internal/menu) depend on, so they can consume the
// real principal without importing this package's full surface.
func (p AuthenticatedPrincipal) HasPermission(permission string) bool {
	for _, perm := range p.Permissions {
		if string(perm) == permission {
			return true
		}
	}
	return false
}

// AuditEvent mirrors contracts.AuditEvent. OldValue/NewValue must never
// contain AuditRedactedFields keys by the time it reaches the repository —
// RecordAudit enforces that.
type AuditEvent struct {
	ID          string
	TenantID    string
	OutletID    *string
	ActorUserID *string
	DeviceID    *string
	Action      string
	EntityType  string
	EntityID    *string
	OldValue    map[string]interface{}
	NewValue    map[string]interface{}
	Reason      *string
	OccurredAt  time.Time
}
