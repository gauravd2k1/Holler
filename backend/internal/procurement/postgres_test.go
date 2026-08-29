package procurement

// Postgres-backed tests for the procurement repository.
//
// WHY THIS FILE EXISTS AT ALL. Until it was written, procurement was the ONLY
// bounded context in this backend with no Postgres-backed test: 984 lines of
// repository.go had never executed one SQL statement against a real server. It
// was covered instead by a static cross-check that every INSERT's column names
// and placeholder arity matched the shipped DDL — which proves the STRINGS are
// consistent with each other and proves nothing about whether Postgres accepts
// them. A type mismatch, a NOT NULL, a CHECK, a trigger or an FK is invisible
// to that check and fatal at runtime. This project has been bitten by exactly
// that substitution before.
//
// Every test here runs the real migrations (packages/contracts/postgres/*.sql)
// and talks to a live server through testdb.RequireDatabaseURL.

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/inventory"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/testdb"
	"github.com/holler/backend/internal/tenant"
	contracts "github.com/holler/contracts"
)

func setupPool(t *testing.T) postgres.Pool {
	t.Helper()
	dbURL := testdb.RequireDatabaseURL(t)

	ctx := context.Background()
	pool, err := postgres.Open(ctx, dbURL)
	if err != nil {
		t.Fatalf("postgres.Open: %v", err)
	}
	t.Cleanup(pool.Close)

	contractsDir, err := filepath.Abs(filepath.Join("..", "..", "..", "packages", "contracts", "postgres"))
	if err != nil {
		t.Fatalf("resolving contracts dir: %v", err)
	}
	if err := postgres.Migrate(ctx, pool, contractsDir); err != nil {
		t.Fatalf("postgres.Migrate: %v", err)
	}
	return pool
}

// pgFixture is one tenant/brand/outlet with a user, a second outlet in the
// same tenant (for the transfer destination) and two inventory items of
// DIFFERENT dimensions — the second one exists so the quantity_dimension
// mismatch can be provoked against a REAL inventory_item row rather than a
// stub, which is the only way that guard is worth anything.
type pgFixture struct {
	tenantID      string
	outletID      string
	otherOutletID string
	userID        string
	massItemID    string
	countItemID   string
}

func newPgFixture(t *testing.T, pool postgres.Pool, label string) pgFixture {
	t.Helper()
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))

	org, err := tenantSvc.CreateOrganisation(ctx, label+" Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, label+" Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, label+" Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}
	other, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, label+" Outlet 2", "")
	if err != nil {
		t.Fatalf("CreateOutlet (destination): %v", err)
	}

	userID := id.New()
	if _, err := pool.Exec(ctx, `
		INSERT INTO app_user (id, tenant_id, email, full_name, password_hash, is_active, config_version, created_at, updated_at)
		VALUES ($1, $2, $3, $4, 'unused', TRUE, 0, now(), now())`,
		userID, org.ID, userID+"@example.com", label+" Buyer"); err != nil {
		t.Fatalf("inserting app_user fixture: %v", err)
	}

	invSvc := inventory.NewService(inventory.NewRepository(pool))
	massItem, _, err := invSvc.CreateInventoryItem(ctx, org.ID, inventory.NewInventoryItemInput{
		ID: id.New(), OutletID: out.ID, SKU: label + "-RICE", Name: "Rice",
		Dimension: contracts.DimensionMass, IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreateInventoryItem (MASS): %v", err)
	}
	countItem, _, err := invSvc.CreateInventoryItem(ctx, org.ID, inventory.NewInventoryItemInput{
		ID: id.New(), OutletID: out.ID, SKU: label + "-EGGS", Name: "Eggs",
		Dimension: contracts.DimensionCount, IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreateInventoryItem (COUNT): %v", err)
	}

	return pgFixture{
		tenantID: org.ID, outletID: out.ID, otherOutletID: other.ID, userID: userID,
		massItemID: massItem.ID, countItemID: countItem.ID,
	}
}

// grantApprovalRole creates a real role carrying procurement.approve with the
// given ceiling and assigns it to the fixture's user, so PoApprovalLimitForUser
// and RolesAbleToApprove run their real joins over role / role_permission /
// user_role rather than a fake's map lookup.
//
// limitPaise nil leaves po_approval_limit_paise NULL — the case the whole
// approval design turns on: NULL means "may not approve any amount", never
// "unlimited".
func grantApprovalRole(t *testing.T, pool postgres.Pool, fx pgFixture, code, name string, limitPaise *int64) string {
	t.Helper()
	ctx := context.Background()
	roleID := id.New()
	if _, err := pool.Exec(ctx,
		`INSERT INTO role (id, tenant_id, code, name, po_approval_limit_paise, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5, now(), now())`,
		roleID, fx.tenantID, code, name, limitPaise); err != nil {
		t.Fatalf("inserting role fixture: %v", err)
	}
	if _, err := pool.Exec(ctx,
		`INSERT INTO role_permission (role_id, permission) VALUES ($1, $2)`,
		roleID, string(PermissionApprove)); err != nil {
		t.Fatalf("inserting role_permission fixture: %v", err)
	}
	if _, err := pool.Exec(ctx,
		`INSERT INTO user_role (id, user_id, role_id, outlet_id, created_at) VALUES ($1, $2, $3, NULL, now())`,
		id.New(), fx.userID, roleID); err != nil {
		t.Fatalf("inserting user_role fixture: %v", err)
	}
	return roleID
}

func newPgService(pool postgres.Pool) *Service {
	return NewService(NewRepository(pool))
}

func pgPrincipal(fx pgFixture, perms ...contracts.Permission) auth.AuthenticatedPrincipal {
	return auth.AuthenticatedPrincipal{
		UserID: fx.userID, TenantID: fx.tenantID, OutletID: fx.outletID, Permissions: perms,
	}
}

func strPtr(s string) *string { return &s }

// idSuffix takes the TAIL of an app-generated id, never the head.
//
// id.New() is UUIDv7: the leading bits are a millisecond timestamp, so the
// first eight characters are IDENTICAL for every id minted within about a
// minute of each other. Building a human-facing code out of a prefix therefore
// produces collisions against UNIQUE (outlet_id, po_number) that look like
// application bugs — this test suite hit exactly that on its first live run.
// The tail is the random half.
func idSuffix(id string) string {
	clean := ""
	for _, r := range id {
		if r != '-' {
			clean += string(r)
		}
	}
	return clean[len(clean)-10:]
}

func seedPgSupplier(t *testing.T, svc *Service, fx pgFixture) string {
	t.Helper()
	supplierID := id.New()
	if _, _, err := svc.CreateSupplier(context.Background(), fx.tenantID, NewSupplierInput{
		Supplier: Supplier{ID: supplierID, OutletID: fx.outletID, Code: "S-" + idSuffix(supplierID), Name: "Supplier", IsActive: true},
	}); err != nil {
		t.Fatalf("seeding supplier: %v", err)
	}
	return supplierID
}

func countRows(t *testing.T, pool postgres.Pool, query string, args ...any) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), query, args...).Scan(&n); err != nil {
		t.Fatalf("counting rows (%s): %v", query, err)
	}
	return n
}

// --- supplier + supplier_item against the real schema -----------------------

// TestPostgres_SupplierRoundTripAndDimensionRejection exercises UpsertSupplier,
// its wholesale child replace, SuppliersSince / SupplierItemsSince, and the
// quantity_dimension guard against a REAL COUNT-dimensioned inventory_item row
// — the only place that comparison means anything.
func TestPostgres_SupplierRoundTripAndDimensionRejection(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "SupplierRT")
	svc := newPgService(pool)

	supplierID := id.New()
	price := int64(120000)
	stored, items, err := svc.CreateSupplier(ctx, fx.tenantID, NewSupplierInput{
		Supplier: Supplier{
			ID: supplierID, OutletID: fx.outletID, Code: "ACME", Name: "Acme Foods",
			Gstin: strPtr("27AAPFU0939F1ZV"), Phone: strPtr("+919000000000"),
			Email: strPtr("orders@acme.example"), Address: strPtr("12 Market Road"),
			PaymentTermsDays: 30, IsActive: true,
		},
		Items: []SupplierItem{{
			ID: id.New(), InventoryItemID: fx.massItemID, PurchaseUnit: "50kg sack",
			PackSizeMicro: 50_000_000, QuantityDimension: DimensionMass,
			LastPricePaise: &price, IsPreferred: true,
		}},
	})
	if err != nil {
		t.Fatalf("CreateSupplier: %v", err)
	}
	if len(items) != 1 {
		t.Fatalf("want one supplier_item, got %d", len(items))
	}

	// FIXTURES ACTUALLY INSERTED, asserted before anything is claimed about
	// them: a rejected INSERT leaves zero rows and every later assertion then
	// passes trivially on absent data.
	if n := countRows(t, pool, `SELECT count(*) FROM supplier WHERE id = $1`, supplierID); n != 1 {
		t.Fatalf("supplier fixture did not insert: %d rows", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM supplier_item WHERE supplier_id = $1`, supplierID); n != 1 {
		t.Fatalf("supplier_item fixture did not insert: %d rows", n)
	}

	bundle, err := svc.SyncConfigBundle(ctx, fx.tenantID, fx.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle: %v", err)
	}
	if len(bundle.Suppliers) != 1 || len(bundle.SupplierItems) != 1 {
		t.Fatalf("supplier/supplier_item missing from the config bundle: %+v", bundle)
	}
	got := bundle.Suppliers[0]
	if got.Code != "ACME" || got.Name != "Acme Foods" || got.PaymentTermsDays != 30 || !got.IsActive {
		t.Errorf("supplier did not round-trip: %+v", got)
	}
	if got.Gstin == nil || *got.Gstin != "27AAPFU0939F1ZV" {
		t.Errorf("gstin did not round-trip: %v", got.Gstin)
	}
	if got.Address == nil || *got.Address != "12 Market Road" {
		t.Errorf("address did not round-trip: %v", got.Address)
	}
	if got.Phone == nil || got.Email == nil {
		t.Errorf("phone/email did not round-trip: %v / %v", got.Phone, got.Email)
	}
	gotItem := bundle.SupplierItems[0]
	if gotItem.PackSizeMicro != 50_000_000 || gotItem.QuantityDimension != DimensionMass || !gotItem.IsPreferred {
		t.Errorf("supplier_item did not round-trip: %+v", gotItem)
	}
	if gotItem.LastPricePaise == nil || *gotItem.LastPricePaise != price {
		t.Errorf("last_price_paise did not round-trip: %v", gotItem.LastPricePaise)
	}

	// since_version withholds what the caller already has.
	empty, err := svc.SyncConfigBundle(ctx, fx.tenantID, fx.outletID, int(stored.ConfigVersion))
	if err != nil {
		t.Fatalf("SyncConfigBundle(since=current): %v", err)
	}
	if len(empty.Suppliers) != 0 || len(empty.SupplierItems) != 0 {
		t.Errorf("since_version must withhold already-delivered rows: %+v", empty)
	}

	// The child list is REPLACED wholesale, not merged.
	if _, items2, err := svc.CreateSupplier(ctx, fx.tenantID, NewSupplierInput{
		Supplier: Supplier{ID: supplierID, OutletID: fx.outletID, Code: "ACME", Name: "Acme Foods", IsActive: true},
		Items:    []SupplierItem{},
	}); err != nil || len(items2) != 0 {
		t.Fatalf("re-upsert with an empty price list: items=%d err=%v", len(items2), err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM supplier_item WHERE supplier_id = $1`, supplierID); n != 0 {
		t.Errorf("supplier_item children must be replaced wholesale, %d rows survived", n)
	}

	// THE GUARD, against a real COUNT-dimensioned inventory_item.
	_, _, err = svc.CreateSupplier(ctx, fx.tenantID, NewSupplierInput{
		Supplier: Supplier{ID: id.New(), OutletID: fx.outletID, Code: "MISMATCH", Name: "Mismatch", IsActive: true},
		Items: []SupplierItem{{
			ID: id.New(), InventoryItemID: fx.countItemID, PurchaseUnit: "tray",
			PackSizeMicro: 30_000_000, QuantityDimension: DimensionMass,
		}},
	})
	if !errors.Is(err, ErrDimensionMismatch) {
		t.Fatalf("want ErrDimensionMismatch against a real COUNT item, got %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM supplier WHERE code = 'MISMATCH' AND outlet_id = $1`, fx.outletID); n != 0 {
		t.Errorf("a rejected supplier must not be partially written: %d rows", n)
	}
}

// --- purchase order: create, approve, and the two gates ---------------------

func seedPgPurchaseOrder(t *testing.T, svc *Service, fx pgFixture, supplierID string, totalPaise int64) PurchaseOrder {
	t.Helper()
	poID := id.New()
	po, err := svc.CreatePurchaseOrder(context.Background(), fx.tenantID, NewPurchaseOrderInput{
		PurchaseOrder: PurchaseOrder{
			ID: poID, OutletID: fx.outletID, SupplierID: supplierID,
			PoNumber: "PO-" + idSuffix(poID), Status: PurchaseOrderStatusPendingApproval,
			ExpectedDate: strPtr("2026-09-05"), Notes: strPtr("standing weekly order"),
			TotalPaise: totalPaise,
			Lines: []PurchaseOrderLine{{
				ID: id.New(), InventoryItemID: fx.massItemID, LineNumber: 1, PurchaseUnit: "50kg sack",
				OrderedQuantityMicro: 100_000_000, QuantityDimension: DimensionMass,
				UnitPricePaise: 120000, LineTotalPaise: totalPaise,
			}},
		},
	})
	if err != nil {
		t.Fatalf("CreatePurchaseOrder: %v", err)
	}
	return po
}

// TestPostgres_PurchaseOrderApprovalWritesBothFieldsTogether drives approval
// through the real role / role_permission / user_role joins and the real
// purchase_order CHECK constraints — including
// purchase_order_approval_is_whole, which is what the database would use to
// reject a half-written approval if this code ever attempted one.
func TestPostgres_PurchaseOrderApprovalWritesBothFieldsTogether(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "POApprove")
	svc := newPgService(pool)

	supplierID := seedPgSupplier(t, svc, fx)
	const total int64 = 5_000_00
	po := seedPgPurchaseOrder(t, svc, fx, supplierID, total)

	if n := countRows(t, pool, `SELECT count(*) FROM purchase_order WHERE id = $1`, po.ID); n != 1 {
		t.Fatalf("purchase_order fixture did not insert: %d rows", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM purchase_order_line WHERE purchase_order_id = $1`, po.ID); n != 1 {
		t.Fatalf("purchase_order_line fixture did not insert: %d rows", n)
	}

	grantApprovalRole(t, pool, fx, "PG_BUYER_"+idSuffix(po.ID), "Purchasing Manager", ptrInt64(10_000_00))

	approved, err := svc.ApprovePurchaseOrder(ctx, pgPrincipal(fx, PermissionApprove), po.ID)
	if err != nil {
		t.Fatalf("ApprovePurchaseOrder: %v", err)
	}
	if approved.Status != PurchaseOrderStatusApproved {
		t.Errorf("want APPROVED, got %s", approved.Status)
	}

	// Read the columns straight out of Postgres, not off the returned struct.
	var approver *string
	var approvedAt *time.Time
	var status string
	if err := pool.QueryRow(ctx,
		`SELECT status, approved_by_user_id, approved_at FROM purchase_order WHERE id = $1`, po.ID,
	).Scan(&status, &approver, &approvedAt); err != nil {
		t.Fatalf("reading back the approval: %v", err)
	}
	if status != string(PurchaseOrderStatusApproved) {
		t.Errorf("stored status: want APPROVED, got %s", status)
	}
	if approver == nil || approvedAt == nil {
		t.Fatalf("BOTH approval columns must be written together, got %v / %v", approver, approvedAt)
	}
	if *approver != fx.userID {
		t.Errorf("want approver %s, got %s", fx.userID, *approver)
	}

	reread, err := svc.GetPurchaseOrder(ctx, fx.tenantID, po.ID)
	if err != nil {
		t.Fatalf("GetPurchaseOrder: %v", err)
	}
	if len(reread.Lines) != 1 || reread.Lines[0].OrderedQuantityMicro != 100_000_000 {
		t.Fatalf("purchase_order_line did not round-trip: %+v", reread.Lines)
	}
	if reread.Lines[0].QuantityDimension != DimensionMass || reread.Lines[0].UnitPricePaise != 120000 {
		t.Errorf("purchase_order_line fields did not round-trip: %+v", reread.Lines[0])
	}
	if reread.ExpectedDate == nil || *reread.ExpectedDate != "2026-09-05" {
		t.Errorf("expected_date did not round-trip through DATE: %v", reread.ExpectedDate)
	}
	if reread.Notes == nil || *reread.Notes != "standing weekly order" {
		t.Errorf("notes did not round-trip: %v", reread.Notes)
	}
	if reread.ApprovedByUserID == nil || reread.ApprovedAt == nil {
		t.Errorf("re-read must carry both approval fields: %+v", reread)
	}

	// An amend must not silently clear an approval that already happened.
	if _, err := svc.CreatePurchaseOrder(ctx, fx.tenantID, NewPurchaseOrderInput{
		PurchaseOrder: PurchaseOrder{
			ID: po.ID, OutletID: fx.outletID, SupplierID: supplierID, PoNumber: po.PoNumber,
			Status: PurchaseOrderStatusPendingApproval, TotalPaise: total,
		},
	}); err != nil {
		t.Fatalf("amending an approved order: %v", err)
	}
	if err := pool.QueryRow(ctx,
		`SELECT approved_by_user_id FROM purchase_order WHERE id = $1`, po.ID).Scan(&approver); err != nil {
		t.Fatalf("re-reading after amend: %v", err)
	}
	if approver == nil {
		t.Error("an amend must not clear approved_by_user_id — the create route does not write that column at all")
	}
}

// TestPostgres_ApprovalIsRefusedOverTheRoleCeiling covers both halves of gate
// 2 against real role rows: a NULL ceiling refuses ANY amount, and a real
// ceiling refuses anything above it while naming a role that could approve
// instead. RolesAbleToApprove is tenant-wide and PoApprovalLimitForUser is
// user-scoped; the fixture keeps them deliberately distinct so a query that
// confused the two would fail here.
func TestPostgres_ApprovalIsRefusedOverTheRoleCeiling(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "POCeiling")
	svc := newPgService(pool)
	supplierID := seedPgSupplier(t, svc, fx)

	// A tenant-mate role that CAN approve this spend, so the §64 "ask someone
	// else" half has a real row behind it. Then strip it off OUR user.
	// The ceiling here must genuinely COVER the largest order this test
	// raises (250_000_00 paise), or RolesAbleToApprove is right to return
	// nothing and the assertion below would be testing the fixture's
	// arithmetic rather than the query. It caught exactly that on the first
	// live run, at 100_000_00.
	grantApprovalRole(t, pool, fx, "PG_FINDIR_"+idSuffix(fx.userID), "Finance Director", ptrInt64(1_000_000_00))
	if _, err := pool.Exec(ctx, `DELETE FROM user_role WHERE user_id = $1`, fx.userID); err != nil {
		t.Fatalf("clearing user_role: %v", err)
	}

	t.Run("null ceiling refuses any amount", func(t *testing.T) {
		po := seedPgPurchaseOrder(t, svc, fx, supplierID, 1)
		grantApprovalRole(t, pool, fx, "PG_NULLLIMIT_"+idSuffix(po.ID), "Unconfigured Approver", nil)
		t.Cleanup(func() { pool.Exec(ctx, `DELETE FROM user_role WHERE user_id = $1`, fx.userID) })

		_, err := svc.ApprovePurchaseOrder(ctx, pgPrincipal(fx, PermissionApprove), po.ID)
		var refusal *ApprovalRefusal
		if !errors.As(err, &refusal) {
			t.Fatalf("want *ApprovalRefusal, got %v", err)
		}
		if refusal.LimitPaise != nil {
			t.Errorf("a NULL po_approval_limit_paise must stay nil, got %d", *refusal.LimitPaise)
		}
		if len(refusal.Alternatives) == 0 {
			t.Errorf("§64: a real Finance Director row exists and must be named: %+v", refusal)
		}
		var approver *string
		if err := pool.QueryRow(ctx, `SELECT approved_by_user_id FROM purchase_order WHERE id = $1`, po.ID).Scan(&approver); err != nil {
			t.Fatalf("reading back: %v", err)
		}
		if approver != nil {
			t.Error("a refused approval must leave the row untouched")
		}
	})

	t.Run("real ceiling refuses what exceeds it", func(t *testing.T) {
		const total int64 = 250_000_00
		po := seedPgPurchaseOrder(t, svc, fx, supplierID, total)
		grantApprovalRole(t, pool, fx, "PG_SMALL_"+idSuffix(po.ID), "Junior Buyer", ptrInt64(50_000_00))
		t.Cleanup(func() { pool.Exec(ctx, `DELETE FROM user_role WHERE user_id = $1`, fx.userID) })

		_, err := svc.ApprovePurchaseOrder(ctx, pgPrincipal(fx, PermissionApprove), po.ID)
		var refusal *ApprovalRefusal
		if !errors.As(err, &refusal) {
			t.Fatalf("want *ApprovalRefusal, got %v", err)
		}
		if refusal.Code != approvalRefusalCodeOverLimit {
			t.Errorf("want %q, got %q", approvalRefusalCodeOverLimit, refusal.Code)
		}
		if refusal.LimitPaise == nil || *refusal.LimitPaise != 50_000_00 {
			t.Fatalf("want the real ceiling read from role, got %v", refusal.LimitPaise)
		}
		if refusal.TotalPaise != total {
			t.Errorf("want total %d, got %d", total, refusal.TotalPaise)
		}
		found := false
		for _, r := range refusal.Alternatives {
			if r == "Finance Director" {
				found = true
			}
		}
		if !found {
			t.Errorf("§64: RolesAbleToApprove must name the Finance Director row, got %v", refusal.Alternatives)
		}
	})
}

// --- cross-tenant isolation -------------------------------------------------

// TestPostgres_CrossTenantProcurementIsIsolated is the dedicated isolation
// test every sibling context has. Tenant B must not reach tenant A's purchase
// order or config, and the refusal must be NOT FOUND rather than forbidden —
// a 403 confirms the id exists.
func TestPostgres_CrossTenantProcurementIsIsolated(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fxA := newPgFixture(t, pool, "IsolationA")
	fxB := newPgFixture(t, pool, "IsolationB")
	svc := newPgService(pool)

	supplierA := seedPgSupplier(t, svc, fxA)
	poA := seedPgPurchaseOrder(t, svc, fxA, supplierA, 1000)
	if n := countRows(t, pool, `SELECT count(*) FROM purchase_order WHERE id = $1`, poA.ID); n != 1 {
		t.Fatalf("tenant A purchase order did not insert")
	}

	if _, err := svc.GetPurchaseOrder(ctx, fxB.tenantID, poA.ID); !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("tenant B must not read tenant A's purchase order: got %v, want ErrNotFound", err)
	}
	if _, err := svc.PurchaseOrderReceiptProgress(ctx, fxB.tenantID, poA.ID); !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("tenant B must not derive progress for tenant A's order: got %v", err)
	}
	if _, err := svc.SyncConfigBundle(ctx, fxB.tenantID, fxA.outletID, 0); !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("tenant B must not pull tenant A's outlet config: got %v", err)
	}

	// And tenant B's own bundle is genuinely empty of A's rows rather than
	// merely unasserted.
	bundleB, err := svc.SyncConfigBundle(ctx, fxB.tenantID, fxB.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle for tenant B: %v", err)
	}
	for _, s := range bundleB.Suppliers {
		if s.ID == supplierA {
			t.Fatalf("tenant A's supplier leaked into tenant B's bundle")
		}
	}
	for _, po := range bundleB.PurchaseOrders {
		if po.ID == poA.ID {
			t.Fatalf("tenant A's purchase order leaked into tenant B's bundle")
		}
	}
}

// --- goods receipt ingest ---------------------------------------------------

func pgEnvelope(fx pgFixture, aggregate contracts.AggregateType, recordID string) contracts.SyncEnvelope {
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      fx.tenantID,
		OutletID:      fx.outletID,
		DeviceID:      id.New(),
		AggregateType: aggregate,
		Direction:     contracts.AggregateAuthority[aggregate],
		Version:       1,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

// TestPostgres_GrnNeverBlocksOnAPurchaseOrder is ADR-019 §1 against the REAL
// DDL, which is where the nullability actually lives. A receipt with
// purchase_order_id NULL, supplier_id NULL and every line's
// purchase_order_line_id NULL is STORED. A NOT NULL, a CHECK or an FK that
// crept into the schema or the INSERT would fail here and nowhere else — the
// fake repository in service_test.go cannot see a database constraint at all.
func TestPostgres_GrnNeverBlocksOnAPurchaseOrder(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "GrnNoPO")
	svc := newPgService(pool)

	grnID := id.New()
	grn := GoodsReceiptNote{
		ID: grnID, OutletID: fx.outletID,
		PurchaseOrderID: nil, SupplierID: nil, // THE POINT OF THIS TEST
		GrnNumber: "GRN-" + idSuffix(grnID), DeliveryNoteRef: nil,
		ReceivedAt: "2026-08-29T10:00:00Z", ReceivedByUserID: fx.userID,
		BusinessDate: "2026-08-29", Notes: nil,
		Lines: []GrnLine{{
			ID: id.New(), InventoryItemID: fx.massItemID, LineNumber: 1,
			PurchaseOrderLineID:  nil, // AND THIS
			EnteredPurchaseUnit:  "50kg sack",
			EnteredQuantityMicro: 2_000_000, QuantityDimension: DimensionMass,
			BaseQuantityMicro: 100_000_000, PackSizeMicroApplied: 50_000_000,
			UnitCostPaise: 240, LineTotalPaise: 240000,
		}},
	}

	stored, err := svc.IngestGoodsReceiptNote(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeGoodsReceiptNote, grnID), grn)
	if err != nil {
		t.Fatalf("a receipt with no PO and no supplier must be ACCEPTED: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM goods_receipt_note WHERE id = $1`, grnID); n != 1 {
		t.Fatalf("the receipt did not insert: %d rows", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM grn_line WHERE grn_id = $1`, grnID); n != 1 {
		t.Fatalf("the grn_line did not insert: %d rows", n)
	}
	if stored.PurchaseOrderID != nil || stored.SupplierID != nil {
		t.Error("nothing may invent a PO or supplier link the edge did not send")
	}

	// The nulls are nulls in the database, not empty strings.
	var poID, supplierID, lineRef *string
	if err := pool.QueryRow(ctx,
		`SELECT g.purchase_order_id, g.supplier_id, l.purchase_order_line_id
		 FROM goods_receipt_note g JOIN grn_line l ON l.grn_id = g.id WHERE g.id = $1`, grnID,
	).Scan(&poID, &supplierID, &lineRef); err != nil {
		t.Fatalf("reading back the nulls: %v", err)
	}
	if poID != nil || supplierID != nil || lineRef != nil {
		t.Errorf("want three NULLs in Postgres, got %v / %v / %v", poID, supplierID, lineRef)
	}

	// Idempotent replay against a real UNIQUE (outlet_id, grn_number).
	if _, err := svc.IngestGoodsReceiptNote(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeGoodsReceiptNote, grnID), grn); err != nil {
		t.Fatalf("a repeated replay is an ordinary retry, not a fault: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM goods_receipt_note WHERE id = $1`, grnID); n != 1 {
		t.Fatalf("a retry must not duplicate the row: %d rows", n)
	}
}

// TestPostgres_GrnReadsBackIdenticallyWithEveryProvenanceFieldPopulated is M5
// acceptance criterion 6's cloud half, written the way contracts 0.5.9 says it
// must be.
//
// THE FIXTURE POPULATES EVERY NULLABLE PROVENANCE FIELD — purchase_order_id,
// supplier_id, delivery_note_ref, notes, and on the line batch_code,
// expiry_date and purchase_order_line_id — because a NULL round-trips through
// a column the INSERT never mentions PERFECTLY. That is exactly how
// source_stock_count_id was dropped in silence for four versions while a
// null-heavy fixture reported green. Every field is asserted individually
// rather than by a struct compare, so a loss names the field it lost.
func TestPostgres_GrnReadsBackIdenticallyWithEveryProvenanceFieldPopulated(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "GrnFidelity")
	svc := newPgService(pool)

	supplierID := seedPgSupplier(t, svc, fx)
	po := seedPgPurchaseOrder(t, svc, fx, supplierID, 240000)
	poLineID := po.Lines[0].ID

	grnID := id.New()
	grn := GoodsReceiptNote{
		ID: grnID, OutletID: fx.outletID,
		PurchaseOrderID: &po.ID,
		SupplierID:      &supplierID,
		GrnNumber:       "GRN-" + idSuffix(grnID),
		DeliveryNoteRef: strPtr("DN/2026/44817"),
		ReceivedAt:      "2026-08-29T10:15:00Z",
		// received_by_user_id is NOT NULL and FK-checked; a stub id would fail
		// here, which is part of what makes this a real test.
		ReceivedByUserID: fx.userID,
		BusinessDate:     "2026-08-29",
		Notes:            strPtr("two sacks short, driver noted it"),
		Lines: []GrnLine{{
			ID: id.New(), InventoryItemID: fx.massItemID, LineNumber: 1,
			PurchaseOrderLineID: &poLineID,
			// BOTH SIDES OF THE CONVERSION, which is the row's whole reason
			// for existing: "what did the operator actually type?" must be
			// answerable without reconstructing it from a supplier_item that
			// may have been edited since.
			EnteredPurchaseUnit:  "50kg sack",
			EnteredQuantityMicro: 2_000_000,
			QuantityDimension:    DimensionMass,
			BaseQuantityMicro:    100_000_000,
			PackSizeMicroApplied: 50_000_000,
			UnitCostPaise:        240,
			LineTotalPaise:       240000,
			BatchCode:            strPtr("BATCH-2026-08-A"),
			ExpiryDate:           strPtr("2027-02-28"),
		}},
	}

	// The fixture must actually BE populated. A test that drifted to a
	// null-heavy fixture would keep reporting green while proving nothing —
	// the failure this whole test exists to prevent.
	if grn.PurchaseOrderID == nil || grn.SupplierID == nil || grn.DeliveryNoteRef == nil || grn.Notes == nil {
		t.Fatal("fixture regression: the receipt must populate every nullable header field")
	}
	if grn.Lines[0].PurchaseOrderLineID == nil || grn.Lines[0].BatchCode == nil || grn.Lines[0].ExpiryDate == nil {
		t.Fatal("fixture regression: the line must populate every nullable provenance field")
	}

	if _, err := svc.IngestGoodsReceiptNote(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeGoodsReceiptNote, grnID), grn); err != nil {
		t.Fatalf("IngestGoodsReceiptNote: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM goods_receipt_note WHERE id = $1`, grnID); n != 1 {
		t.Fatalf("the receipt did not insert: %d rows", n)
	}

	repo := NewRepository(pool)
	back, found, err := repo.GetGoodsReceiptNoteByID(ctx, grnID)
	if err != nil || !found {
		t.Fatalf("GetGoodsReceiptNoteByID: found=%v err=%v", found, err)
	}
	lines, err := repo.GrnLines(ctx, grnID)
	if err != nil {
		t.Fatalf("GrnLines: %v", err)
	}
	if len(lines) != 1 {
		t.Fatalf("want one line back, got %d", len(lines))
	}
	back.Lines = lines

	// Field by field, so a loss names itself.
	assertStr(t, "id", back.ID, grn.ID)
	assertStr(t, "outlet_id", back.OutletID, grn.OutletID)
	assertPtr(t, "purchase_order_id", back.PurchaseOrderID, grn.PurchaseOrderID)
	assertPtr(t, "supplier_id", back.SupplierID, grn.SupplierID)
	assertStr(t, "grn_number", back.GrnNumber, grn.GrnNumber)
	assertPtr(t, "delivery_note_ref", back.DeliveryNoteRef, grn.DeliveryNoteRef)
	assertStr(t, "received_by_user_id", back.ReceivedByUserID, grn.ReceivedByUserID)
	assertStr(t, "business_date", back.BusinessDate, grn.BusinessDate)
	assertPtr(t, "notes", back.Notes, grn.Notes)
	assertStr(t, "received_at", back.ReceivedAt, grn.ReceivedAt)

	want := grn.Lines[0]
	got := back.Lines[0]
	assertStr(t, "line.id", got.ID, want.ID)
	assertStr(t, "line.grn_id", got.GrnID, grn.ID)
	assertStr(t, "line.inventory_item_id", got.InventoryItemID, want.InventoryItemID)
	assertPtr(t, "line.purchase_order_line_id", got.PurchaseOrderLineID, want.PurchaseOrderLineID)
	assertStr(t, "line.entered_purchase_unit", got.EnteredPurchaseUnit, want.EnteredPurchaseUnit)
	assertStr(t, "line.quantity_dimension", string(got.QuantityDimension), string(want.QuantityDimension))
	assertPtr(t, "line.batch_code", got.BatchCode, want.BatchCode)
	assertPtr(t, "line.expiry_date", got.ExpiryDate, want.ExpiryDate)
	assertI64(t, "line.line_number", int64(got.LineNumber), int64(want.LineNumber))
	assertI64(t, "line.entered_quantity_micro", got.EnteredQuantityMicro, want.EnteredQuantityMicro)
	assertI64(t, "line.base_quantity_micro", got.BaseQuantityMicro, want.BaseQuantityMicro)
	assertI64(t, "line.pack_size_micro_applied", got.PackSizeMicroApplied, want.PackSizeMicroApplied)
	assertI64(t, "line.unit_cost_paise", got.UnitCostPaise, want.UnitCostPaise)
	assertI64(t, "line.line_total_paise", got.LineTotalPaise, want.LineTotalPaise)

	// The receipt is IMMUTABLE in Postgres — enforced by a trigger, not by a
	// convention this package could quietly stop honouring.
	if _, err := pool.Exec(ctx, `UPDATE goods_receipt_note SET notes = 'tampered' WHERE id = $1`, grnID); err == nil {
		t.Error("goods_receipt_note must be immutable: the UPDATE trigger did not fire")
	}
	if _, err := pool.Exec(ctx, `DELETE FROM goods_receipt_note WHERE id = $1`, grnID); err == nil {
		t.Error("goods_receipt_note must be immutable: the DELETE trigger did not fire")
	}

	// Receipt progress derives from these real grn_line rows and writes
	// nothing back onto the order.
	progress, err := svc.PurchaseOrderReceiptProgress(ctx, fx.tenantID, po.ID)
	if err != nil {
		t.Fatalf("PurchaseOrderReceiptProgress: %v", err)
	}
	if progress.Scope != ScopeCloudWide {
		t.Errorf("the derived figure must label its scope, got %q", progress.Scope)
	}
	if len(progress.Lines) != 1 || progress.Lines[0].ReceivedBaseQuantityMicro != 100_000_000 {
		t.Fatalf("progress not derived from the real grn_line rows: %+v", progress.Lines)
	}
	if progress.Lines[0].OrderedQuantityMicro != 100_000_000 {
		t.Errorf("ordered quantity lost: %+v", progress.Lines[0])
	}
	var status string
	if err := pool.QueryRow(ctx, `SELECT status FROM purchase_order WHERE id = $1`, po.ID).Scan(&status); err != nil {
		t.Fatalf("re-reading the order: %v", err)
	}
	if status != string(PurchaseOrderStatusPendingApproval) {
		t.Errorf("deriving progress must not transition the order, status is %s", status)
	}
}

func assertStr(t *testing.T, field, got, want string) {
	t.Helper()
	if got != want {
		t.Errorf("%s: want %q, got %q", field, want, got)
	}
}

func assertI64(t *testing.T, field string, got, want int64) {
	t.Helper()
	if got != want {
		t.Errorf("%s: want %d, got %d", field, want, got)
	}
}

// assertPtr fails on a nil it did not expect, which is the specific loss this
// file guards against: a dropped column reads back as nil, indistinguishable
// from a legitimately absent value unless the fixture populated it.
func assertPtr(t *testing.T, field string, got, want *string) {
	t.Helper()
	switch {
	case want == nil && got == nil:
	case want == nil:
		t.Errorf("%s: want nil, got %q", field, *got)
	case got == nil:
		t.Errorf("%s: want %q, got nil — the column was DROPPED between the struct, the INSERT and the SELECT", field, *want)
	case *got != *want:
		t.Errorf("%s: want %q, got %q", field, *want, *got)
	}
}

// --- grn_gap: the same route, a plain outbox --------------------------------

// TestPostgres_GrnGapIngestOnTheSameRoute stores a gap beside the receipt it
// explains. It also pins the two structural decisions: the gap has NO
// entry_seq column (it is a discrete event, not a ranged stream), and unlike
// its receipt it carries no immutability trigger.
func TestPostgres_GrnGapIngestOnTheSameRoute(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "GrnGap")
	svc := newPgService(pool)

	grnID := id.New()
	grn := GoodsReceiptNote{
		ID: grnID, OutletID: fx.outletID, GrnNumber: "GRN-" + idSuffix(grnID),
		ReceivedAt: "2026-08-29T10:00:00Z", ReceivedByUserID: fx.userID, BusinessDate: "2026-08-29",
		Lines: []GrnLine{{
			ID: id.New(), InventoryItemID: fx.massItemID, LineNumber: 1,
			EnteredPurchaseUnit: "50kg sack", EnteredQuantityMicro: 1_000_000,
			QuantityDimension: DimensionMass, BaseQuantityMicro: 50_000_000,
			PackSizeMicroApplied: 50_000_000, UnitCostPaise: 240, LineTotalPaise: 120000,
		}},
	}
	if _, err := svc.IngestGoodsReceiptNote(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeGoodsReceiptNote, grnID), grn); err != nil {
		t.Fatalf("seeding the receipt: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM goods_receipt_note WHERE id = $1`, grnID); n != 1 {
		t.Fatalf("receipt fixture did not insert")
	}
	grnLineID := grn.Lines[0].ID

	gapID := id.New()
	gap := GrnGap{
		ID: gapID, OutletID: fx.outletID, GrnID: grnID,
		GrnLineID: &grnLineID, InventoryItemID: &fx.massItemID,
		Reason: contracts.GrnGapReasonNoPurchaseOrder,
		// Prose, because a human reads it: acceptance criterion 3 requires the
		// gap be VISIBLE to a person, not merely present in a table.
		Detail:     strPtr("delivery arrived with no purchase order quoted on the note"),
		OccurredAt: "2026-08-29T10:01:00Z", BusinessDate: "2026-08-29",
	}
	stored, err := svc.IngestGrnGap(ctx, fx.tenantID, pgEnvelope(fx, contracts.AggregateTypeGrnGap, gapID), gap)
	if err != nil {
		t.Fatalf("IngestGrnGap: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM grn_gap WHERE id = $1`, gapID); n != 1 {
		t.Fatalf("the gap did not insert: %d rows", n)
	}
	if stored.Reason != contracts.GrnGapReasonNoPurchaseOrder {
		t.Errorf("reason did not round-trip: %s", stored.Reason)
	}

	repo := NewRepository(pool)
	back, found, err := repo.GetGrnGapByID(ctx, gapID)
	if err != nil || !found {
		t.Fatalf("GetGrnGapByID: found=%v err=%v", found, err)
	}
	assertPtr(t, "gap.detail", back.Detail, gap.Detail)
	assertPtr(t, "gap.grn_line_id", back.GrnLineID, gap.GrnLineID)
	assertPtr(t, "gap.inventory_item_id", back.InventoryItemID, gap.InventoryItemID)
	assertStr(t, "gap.grn_id", back.GrnID, grnID)
	assertStr(t, "gap.business_date", back.BusinessDate, gap.BusinessDate)

	// PLAIN OUTBOX. stock_deduction_gap earned 0.5.8's ranged-sync machinery
	// because it is a per-sale stream; this one is a handful a week that a
	// buyer acts on, and giving it a counter would import that whole failure
	// surface for nothing. The contrast is what makes it a decision.
	var hasSeq bool
	if err := pool.QueryRow(ctx,
		`SELECT EXISTS(SELECT 1 FROM information_schema.columns
		               WHERE table_name = 'grn_gap' AND column_name = 'entry_seq')`).Scan(&hasSeq); err != nil {
		t.Fatalf("inspecting grn_gap columns: %v", err)
	}
	if hasSeq {
		t.Error("grn_gap must NOT have an entry_seq: it is a plain outbox, not a ranged stream (ADR-019 §2)")
	}
	if err := pool.QueryRow(ctx,
		`SELECT EXISTS(SELECT 1 FROM information_schema.columns
		               WHERE table_name = 'stock_deduction_gap' AND column_name = 'entry_seq')`).Scan(&hasSeq); err != nil {
		t.Fatalf("inspecting stock_deduction_gap columns: %v", err)
	}
	if !hasSeq {
		t.Error("stock_deduction_gap MUST still have entry_seq — without the contrast the assertion above is vacuous")
	}
}

// --- purchase_return and stock_transfer_out ---------------------------------

// TestPostgres_PurchaseReturnAndTransferOutRoundTrip exercises the remaining
// two ingest paths and their immutability triggers, and the real
// stock_transfer_out_not_to_itself CHECK.
func TestPostgres_PurchaseReturnAndTransferOutRoundTrip(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "ReturnXfer")
	svc := newPgService(pool)
	repo := NewRepository(pool)
	supplierID := seedPgSupplier(t, svc, fx)

	retID := id.New()
	ret := PurchaseReturn{
		ID: retID, OutletID: fx.outletID, SupplierID: &supplierID, GrnID: nil,
		ReturnNumber: "RET-" + idSuffix(retID), Reason: contracts.PurchaseReturnReasonDamaged,
		ReturnedAt: "2026-08-29T11:00:00Z", ReturnedByUserID: fx.userID,
		BusinessDate: "2026-08-29", Notes: strPtr("three sacks water-damaged in transit"),
		Lines: []PurchaseReturnLine{{
			ID: id.New(), InventoryItemID: fx.massItemID, GrnLineID: nil, LineNumber: 1,
			EnteredPurchaseUnit: "50kg sack", EnteredQuantityMicro: 3_000_000,
			QuantityDimension: DimensionMass, BaseQuantityMicro: 150_000_000, UnitCostPaise: 240,
		}},
	}
	if _, err := svc.IngestPurchaseReturn(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypePurchaseReturn, retID), ret); err != nil {
		t.Fatalf("IngestPurchaseReturn: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM purchase_return WHERE id = $1`, retID); n != 1 {
		t.Fatalf("the return did not insert: %d rows", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM purchase_return_line WHERE purchase_return_id = $1`, retID); n != 1 {
		t.Fatalf("the return line did not insert: %d rows", n)
	}
	backRet, found, err := repo.GetPurchaseReturnByID(ctx, retID)
	if err != nil || !found {
		t.Fatalf("GetPurchaseReturnByID: found=%v err=%v", found, err)
	}
	assertPtr(t, "return.supplier_id", backRet.SupplierID, ret.SupplierID)
	assertPtr(t, "return.notes", backRet.Notes, ret.Notes)
	assertStr(t, "return.reason", string(backRet.Reason), string(ret.Reason))
	retLines, err := repo.PurchaseReturnLines(ctx, retID)
	if err != nil {
		t.Fatalf("PurchaseReturnLines: %v", err)
	}
	if len(retLines) != 1 || retLines[0].BaseQuantityMicro != 150_000_000 {
		t.Fatalf("return line did not round-trip: %+v", retLines)
	}
	if _, err := pool.Exec(ctx, `UPDATE purchase_return SET notes = 'tampered' WHERE id = $1`, retID); err == nil {
		t.Error("purchase_return must be immutable: the trigger did not fire")
	}

	xferID := id.New()
	xfer := StockTransferOut{
		ID: xferID, OutletID: fx.outletID, DestinationOutletID: fx.otherOutletID,
		TransferNumber: "TR-" + idSuffix(xferID), DispatchedAt: "2026-08-29T12:00:00Z",
		DispatchedByUserID: fx.userID, BusinessDate: "2026-08-29",
		Notes: strPtr("covering a shortfall at the second outlet"),
		Lines: []StockTransferLine{{
			ID: id.New(), InventoryItemID: fx.massItemID, LineNumber: 1,
			BaseQuantityMicro: 25_000_000, QuantityDimension: DimensionMass, UnitCostPaise: 240,
		}},
	}
	if _, err := svc.IngestStockTransferOut(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeStockTransferOut, xferID), xfer); err != nil {
		t.Fatalf("IngestStockTransferOut: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM stock_transfer_out WHERE id = $1`, xferID); n != 1 {
		t.Fatalf("the transfer did not insert: %d rows", n)
	}
	backXfer, found, err := repo.GetStockTransferOutByID(ctx, xferID)
	if err != nil || !found {
		t.Fatalf("GetStockTransferOutByID: found=%v err=%v", found, err)
	}
	assertStr(t, "transfer.destination_outlet_id", backXfer.DestinationOutletID, fx.otherOutletID)
	assertPtr(t, "transfer.notes", backXfer.Notes, xfer.Notes)
	xferLines, err := repo.StockTransferLines(ctx, xferID)
	if err != nil {
		t.Fatalf("StockTransferLines: %v", err)
	}
	if len(xferLines) != 1 || xferLines[0].BaseQuantityMicro != 25_000_000 {
		t.Fatalf("transfer line did not round-trip: %+v", xferLines)
	}
	if _, err := pool.Exec(ctx, `DELETE FROM stock_transfer_out WHERE id = $1`, xferID); err == nil {
		t.Error("stock_transfer_out must be immutable: the DELETE trigger did not fire")
	}
}

// --- supplier_invoice / supplier_credit: cloud-only, M5 records only ---------

// TestPostgres_SupplierAccountsCreateAndList covers the two cloud-only shapes
// against postgres/0029, and pins that M5 writes RECEIVED and nothing else.
// The settlement states exist in the column so its shape does not change when
// M7 lands; they are not a hint that this milestone may write them.
func TestPostgres_SupplierAccountsCreateAndList(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "SupplierAccounts")
	svc := newPgService(pool)
	supplierID := seedPgSupplier(t, svc, fx)

	invID := id.New()
	inv, err := svc.CreateSupplierInvoice(ctx, fx.tenantID, SupplierInvoice{
		ID: invID, OutletID: fx.outletID, SupplierID: supplierID,
		SupplierInvoiceNo: "SI-" + idSuffix(invID), InvoiceDate: "2026-08-29",
		DueDate: strPtr("2026-09-28"), SubtotalPaise: 240000, TaxPaise: 12000, TotalPaise: 252000,
	})
	if err != nil {
		t.Fatalf("CreateSupplierInvoice: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM supplier_invoice WHERE id = $1`, invID); n != 1 {
		t.Fatalf("the supplier invoice did not insert: %d rows", n)
	}
	if inv.Status != SupplierInvoiceStatusReceived {
		t.Errorf("M5 writes RECEIVED only, got %s", inv.Status)
	}

	crID := id.New()
	if _, err := svc.CreateSupplierCredit(ctx, fx.tenantID, SupplierCredit{
		ID: crID, OutletID: fx.outletID, SupplierID: supplierID,
		CreditNoteNo: "CN-" + idSuffix(crID), CreditDate: "2026-08-30", AmountPaise: 72000,
	}); err != nil {
		t.Fatalf("CreateSupplierCredit: %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM supplier_credit WHERE id = $1`, crID); n != 1 {
		t.Fatalf("the supplier credit did not insert: %d rows", n)
	}

	invoices, err := svc.ListSupplierInvoices(ctx, fx.tenantID, fx.outletID)
	if err != nil {
		t.Fatalf("ListSupplierInvoices: %v", err)
	}
	if len(invoices) != 1 {
		t.Fatalf("want one invoice, got %d", len(invoices))
	}
	assertStr(t, "invoice.invoice_date", invoices[0].InvoiceDate, "2026-08-29")
	assertPtr(t, "invoice.due_date", invoices[0].DueDate, strPtr("2026-09-28"))
	assertI64(t, "invoice.tax_paise", invoices[0].TaxPaise, 12000)
	assertStr(t, "invoice.status", string(invoices[0].Status), string(SupplierInvoiceStatusReceived))

	credits, err := svc.ListSupplierCredits(ctx, fx.tenantID, fx.outletID)
	if err != nil {
		t.Fatalf("ListSupplierCredits: %v", err)
	}
	if len(credits) != 1 {
		t.Fatalf("want one credit, got %d", len(credits))
	}
	assertStr(t, "credit.credit_date", credits[0].CreditDate, "2026-08-30")
	assertI64(t, "credit.amount_paise", credits[0].AmountPaise, 72000)

	// An M7 settlement state is refused, and refused BEFORE it reaches the
	// database, so the column keeps its shape without the CHECK doing the work.
	if _, err := svc.CreateSupplierInvoice(ctx, fx.tenantID, SupplierInvoice{
		ID: id.New(), OutletID: fx.outletID, SupplierID: supplierID,
		SupplierInvoiceNo: "SI-PAID", InvoiceDate: "2026-08-29", TotalPaise: 1,
		Status: contracts.SupplierInvoiceStatusPaid,
	}); !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("PAID is an M7 state and must be refused in M5, got %v", err)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM supplier_invoice WHERE supplier_invoice_no = 'SI-PAID'`); n != 0 {
		t.Errorf("a refused invoice must not be written: %d rows", n)
	}

	// Cross-tenant: another tenant may not list this outlet's accounts.
	other := newPgFixture(t, pool, "SupplierAccountsOther")
	if _, err := svc.ListSupplierInvoices(ctx, other.tenantID, fx.outletID); !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("cross-tenant supplier invoice list must be refused, got %v", err)
	}
}

// TestPostgres_UniqueViolationsMapToConflict exercises isUniqueViolation
// against the real UNIQUE constraints, which is the one branch of
// repository.go that nothing else reaches. It matters because these are the
// errors a human sees: "conflict: po_number X already exists in this outlet"
// is actionable, an unmapped SQLSTATE 23505 is not.
func TestPostgres_UniqueViolationsMapToConflict(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "UniqueViolation")
	svc := newPgService(pool)
	supplierID := seedPgSupplier(t, svc, fx)

	po := seedPgPurchaseOrder(t, svc, fx, supplierID, 1000)
	if n := countRows(t, pool, `SELECT count(*) FROM purchase_order WHERE id = $1`, po.ID); n != 1 {
		t.Fatalf("purchase order fixture did not insert")
	}

	// A DIFFERENT id reusing the same po_number at the same outlet.
	_, err := svc.CreatePurchaseOrder(ctx, fx.tenantID, NewPurchaseOrderInput{
		PurchaseOrder: PurchaseOrder{
			ID: id.New(), OutletID: fx.outletID, SupplierID: supplierID,
			PoNumber: po.PoNumber, Status: PurchaseOrderStatusDraft, TotalPaise: 1000,
		},
	})
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("a duplicate po_number must map to ErrConflict, got %v", err)
	}

	// Same for a supplier code, which is scoped UNIQUE (outlet_id, code).
	var code string
	if err := pool.QueryRow(ctx, `SELECT code FROM supplier WHERE id = $1`, supplierID).Scan(&code); err != nil {
		t.Fatalf("reading the supplier code: %v", err)
	}
	_, _, err = svc.CreateSupplier(ctx, fx.tenantID, NewSupplierInput{
		Supplier: Supplier{ID: id.New(), OutletID: fx.outletID, Code: code, Name: "Duplicate", IsActive: true},
	})
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("a duplicate supplier code must map to ErrConflict, got %v", err)
	}

	// The SAME code at a DIFFERENT outlet is fine — uniqueness is
	// outlet-scoped, never global.
	if _, _, err := svc.CreateSupplier(ctx, fx.tenantID, NewSupplierInput{
		Supplier: Supplier{ID: id.New(), OutletID: fx.otherOutletID, Code: code, Name: "Other Outlet", IsActive: true},
	}); err != nil {
		t.Fatalf("the same supplier code at another outlet must be allowed: %v", err)
	}
}
