package auth

import (
	"context"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/id"
)

// roleSeeds assigns each of the 15 roles of docs/spec/security-rbac.md
// §Roles a Milestone 1 permission set drawn only from AllM1Permissions.
// Later milestones (inventory, purchasing, payments, reporting...) will
// extend these sets once their permissions exist in the contract — this
// package must never invent a permission string ahead of that.
var roleSeeds = []struct {
	code        RoleCode
	name        string
	permissions []Permission
}{
	{RoleCodePlatformSuperAdmin, "Platform Super Admin", AllM1Permissions},
	{RoleCodeOrganisationOwner, "Organisation Owner", AllM1Permissions},
	{RoleCodeBrandAdmin, "Brand Admin", []Permission{
		PermissionMenuManage, PermissionTableManage, PermissionOutletManage,
		PermissionUserManage, PermissionOrderVoid,
	}},
	{RoleCodeRegionalManager, "Regional Manager", []Permission{
		PermissionOutletManage, PermissionTableManage, PermissionMenuManage, PermissionOrderVoid,
	}},
	{RoleCodeOutletManager, "Outlet Manager", []Permission{
		PermissionOrderCreate, PermissionOrderModify, PermissionOrderCancel,
		PermissionOrderVoid, PermissionMenuManage, PermissionTableManage, PermissionUserManage,
	}},
	// Accountant, Inventory Manager, Purchase Manager and Auditor have no
	// Milestone 1 permissions to grant: reporting/inventory/purchasing are on
	// the M1 excludes list and Auditor is read-only by design. The role rows
	// still exist so the full 15-role set is present per tenant from day one.
	{RoleCodeAccountant, "Accountant", nil},
	{RoleCodeInventoryManager, "Inventory Manager", nil},
	{RoleCodePurchaseManager, "Purchase Manager", nil},
	{RoleCodeChef, "Chef", []Permission{PermissionOrderModify}},
	{RoleCodeKitchenStaff, "Kitchen Staff", []Permission{PermissionOrderModify}},
	{RoleCodeCaptain, "Captain", []Permission{
		PermissionOrderCreate, PermissionOrderModify, PermissionTableManage,
	}},
	{RoleCodeWaiter, "Waiter", []Permission{PermissionOrderCreate, PermissionOrderModify}},
	{RoleCodeCashier, "Cashier", []Permission{
		PermissionOrderCreate, PermissionOrderModify, PermissionOrderCancel,
	}},
	{RoleCodeDeliveryStaff, "Delivery Staff", []Permission{PermissionOrderModify}},
	{RoleCodeAuditor, "Auditor", nil},
}

// RoleSeeder is what SeedTenantRoles needs from persistence. *Repository
// satisfies it.
type RoleSeeder interface {
	SeedRole(ctx context.Context, id, tenantID string, code RoleCode, name string, perms []Permission, now time.Time) error
}

// SeedTenantRoles idempotently creates the 15 roles of
// docs/spec/security-rbac.md for tenantID with their Milestone 1 permission
// sets. Call it once per tenant at creation time (or at backend startup for
// every known tenant); SeedRole no-ops for a role that already exists.
func SeedTenantRoles(ctx context.Context, seeder RoleSeeder, tenantID string) error {
	now := time.Now().UTC()
	for _, seed := range roleSeeds {
		if err := seeder.SeedRole(ctx, id.New(), tenantID, seed.code, seed.name, seed.permissions, now); err != nil {
			return fmt.Errorf("auth: seeding role %s: %w", seed.code, err)
		}
	}
	return nil
}
