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
	PermissionOrderCreate Permission = "order.create"
	PermissionOrderModify Permission = "order.modify"
	PermissionOrderCancel Permission = "order.cancel"
	PermissionOrderVoid   Permission = "order.void"
	PermissionMenuManage  Permission = "menu.manage"
	PermissionTableManage Permission = "table.manage"
	PermissionOutletManage Permission = "outlet.manage"
	PermissionUserManage  Permission = "user.manage"
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

// AuditRedactedFields must never appear in AuditEvent OldValue/NewValue or on
// the wire. Mirrors AUDIT_REDACTED_FIELDS in src/types/identity.ts; the audit
// helper strips these keys before persisting (ADR-011).
var AuditRedactedFields = []string{"password_hash", "pin_hash"}

type AuditEvent struct {
	ID            string                 `json:"id"`
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
