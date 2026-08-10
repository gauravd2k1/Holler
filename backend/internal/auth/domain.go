// Package auth implements the security & RBAC bounded context
// (docs/spec/security-rbac.md): users, roles, permissions, sessions, RBAC
// enforcement and the audit helper other contexts call to record sensitive
// actions.
//
// Wire/domain shapes are the contract types (github.com/holler/contracts),
// aliased below rather than duplicated: backend/go.mod carries a replace
// directive onto packages/contracts/go (added alongside the 0.2.1 contracts
// release), so this package no longer hand-mirrors identity.go.
package auth

import (
	contracts "github.com/holler/contracts"
)

// Permission is a granted capability string, e.g. "order.create".
type Permission = contracts.Permission

// Milestone 1 permission set — re-exported from contracts so callers in this
// package don't need to import it directly. Do not add permissions outside
// this list; the M1 contract does not define them.
const (
	PermissionOrderCreate  = contracts.PermissionOrderCreate
	PermissionOrderModify  = contracts.PermissionOrderModify
	PermissionOrderCancel  = contracts.PermissionOrderCancel
	PermissionOrderVoid    = contracts.PermissionOrderVoid
	PermissionMenuManage   = contracts.PermissionMenuManage
	PermissionTableManage  = contracts.PermissionTableManage
	PermissionOutletManage = contracts.PermissionOutletManage
	PermissionUserManage   = contracts.PermissionUserManage
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
// §Roles.
type RoleCode = contracts.RoleCode

const (
	RoleCodePlatformSuperAdmin = contracts.RoleCodePlatformSuperAdmin
	RoleCodeOrganisationOwner  = contracts.RoleCodeOrganisationOwner
	RoleCodeBrandAdmin         = contracts.RoleCodeBrandAdmin
	RoleCodeRegionalManager    = contracts.RoleCodeRegionalManager
	RoleCodeOutletManager      = contracts.RoleCodeOutletManager
	RoleCodeAccountant         = contracts.RoleCodeAccountant
	RoleCodeInventoryManager   = contracts.RoleCodeInventoryManager
	RoleCodePurchaseManager    = contracts.RoleCodePurchaseManager
	RoleCodeChef               = contracts.RoleCodeChef
	RoleCodeKitchenStaff       = contracts.RoleCodeKitchenStaff
	RoleCodeCaptain            = contracts.RoleCodeCaptain
	RoleCodeWaiter             = contracts.RoleCodeWaiter
	RoleCodeCashier            = contracts.RoleCodeCashier
	RoleCodeDeliveryStaff      = contracts.RoleCodeDeliveryStaff
	RoleCodeAuditor            = contracts.RoleCodeAuditor
)

// AuditRedactedFields must never appear in an AuditEvent OldValue/NewValue or
// on the wire (ADR-011). As of contracts 0.2.1 this also covers
// "token_hash" — a refresh_token row must never reach an audit_event value
// either.
var AuditRedactedFields = contracts.AuditRedactedFields

// Role is a role_permission-joined role row.
type Role = contracts.Role

// RoleAssignment ties a role to a user, optionally narrowed to one outlet.
// OutletID nil = tenant-wide.
type RoleAssignment = contracts.RoleAssignment

// User is an app_user row plus its role assignments. It never carries
// PasswordHash/PinHash out of the repository layer — those stay inside the
// repository's own row-scanning code (credentialRow) so a bug elsewhere in
// this package cannot accidentally serialize them.
type User = contracts.AppUser

// AuthenticatedPrincipal is what a session resolves to.
type AuthenticatedPrincipal = contracts.AuthenticatedPrincipal

// EdgeUserCacheEntry is the ONE named exception to this package's rule that
// password_hash/pin_hash never leave the repository layer (ADR-015). It
// exists solely to be returned by Service.ListEdgeUserCache, which feeds
// GET /sync/config's users array and nowhere else.
type EdgeUserCacheEntry = contracts.EdgeUserCacheEntry

// hasPermission reports whether p carries permission. It is a free function,
// not a method, because AuthenticatedPrincipal is a type alias onto
// contracts.AuthenticatedPrincipal and Go forbids attaching methods to a
// type defined in another module. See Principal below for the
// method-bearing adapter other bounded contexts can consume.
func hasPermission(p AuthenticatedPrincipal, permission string) bool {
	for _, perm := range p.Permissions {
		if string(perm) == permission {
			return true
		}
	}
	return false
}

// Principal adapts an AuthenticatedPrincipal to the minimal
// `HasPermission(string) bool` interface other bounded contexts (e.g.
// backend/internal/menu.Principal) depend on, so they can consume the real
// principal without importing this package's full surface.
type Principal struct {
	AuthenticatedPrincipal
}

// NewPrincipal wraps p for handoff to a HasPermission-shaped interface.
func NewPrincipal(p AuthenticatedPrincipal) Principal {
	return Principal{AuthenticatedPrincipal: p}
}

// HasPermission implements the Principal interface consumed by other
// bounded contexts.
func (p Principal) HasPermission(permission string) bool {
	return hasPermission(p.AuthenticatedPrincipal, permission)
}

// AuditEvent is a contracts.AuditEvent. OldValue/NewValue must never contain
// AuditRedactedFields keys by the time it reaches the repository —
// RecordAudit enforces that.
type AuditEvent = contracts.AuditEvent
