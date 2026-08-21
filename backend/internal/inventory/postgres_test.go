package inventory_test

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/inventory"
	"github.com/holler/backend/internal/menu"
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

// fixture is one tenant/brand/outlet with one sellable menu item variant,
// ready for a recipe to bind to.
type fixture struct {
	tenantID   string
	outletID   string
	variantID  string
	menuItemID string
}

func newFixture(t *testing.T, pool postgres.Pool, label string) fixture {
	t.Helper()
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))
	menuSvc := menu.NewService(menu.NewRepository(pool))

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

	menuCtx := menu.WithPrincipal(ctx, auth.NewPrincipal(auth.AuthenticatedPrincipal{
		UserID: "principal-user", TenantID: org.ID, OutletID: out.ID,
		Permissions: []auth.Permission{auth.PermissionMenuManage},
	}))
	category, err := menuSvc.CreateCategory(menuCtx, menu.NewCategoryInput{OutletID: out.ID, Name: "Mains", SortOrder: 1})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	item, variants, _, err := menuSvc.CreateItem(menuCtx, menu.NewItemInput{
		OutletID: out.ID, CategoryID: category.ID, Name: label + " Dish", BasePricePaise: 32000,
		Variants: []menu.NewVariantInput{{Name: "Regular", PriceDeltaPaise: 0}},
	})
	if err != nil {
		t.Fatalf("CreateItem: %v", err)
	}
	if len(variants) != 1 {
		t.Fatalf("expected exactly one variant, got %d", len(variants))
	}
	return fixture{tenantID: org.ID, outletID: out.ID, variantID: variants[0].ID, menuItemID: item.ID}
}

func newInventoryItem(t *testing.T, svc *inventory.Service, ctx context.Context, tenantID, outletID, sku string, dim contracts.Dimension) contracts.InventoryItem {
	t.Helper()
	item, _, err := svc.CreateInventoryItem(ctx, tenantID, inventory.NewInventoryItemInput{
		ID: newULID(), OutletID: outletID, SKU: sku, Name: sku, Dimension: dim, IsActive: true,
	})
	if err != nil {
		t.Fatalf("CreateInventoryItem(%s): %v", sku, err)
	}
	return item
}

// newULID mints a real, time-sortable app-generated UUIDv7 (§74) — not a
// deterministic counter. This test suite runs against a persistent (not
// per-test-transaction-rolled-back) Postgres database, so a deterministic id
// would collide with a previous run's leftover rows the moment the process
// restarts and the counter resets to 1. That collision surfaced for real
// while writing these tests (see docs/retro.md-style lesson recorded in the
// task report): a "duplicate key" and a stale-row read masqueraded as
// application bugs until traced to the id generator.
func newULID() string {
	return id.New()
}

// TestPostgresRepository_CrossTenantRecipeConfigIsIsolated is the dedicated
// automated test docs/spec/security-rbac.md §Tenant isolation requires for
// recipe, recipe_ingredient and modifier_ingredient_delta, modelled on
// TestPostgresRepository_CrossTenantOrderLookupIsNotFound: tenant B must
// never see tenant A's rows through SyncConfigBundle, and probing tenant A's
// own outlet as tenant B is denied outright.
func TestPostgresRepository_CrossTenantRecipeConfigIsIsolated(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	repo := inventory.NewRepository(pool)
	svc := inventory.NewService(repo)

	fxA := newFixture(t, pool, "Cross Tenant Recipe A")
	fxB := newFixture(t, pool, "Cross Tenant Recipe B")

	paneer := newInventoryItem(t, svc, ctx, fxA.tenantID, fxA.outletID, "PANEER-A", contracts.DimensionMass)

	recipe, ingredients, err := svc.CreateRecipe(ctx, fxA.tenantID, inventory.NewRecipeInput{
		ID: newULID(), MenuItemVariantID: fxA.variantID, Name: "A Recipe",
		OutputDimension: contracts.DimensionCount, OutputQuantityMicro: 1_000_000,
		Ingredients: []inventory.NewRecipeIngredientInput{
			{ID: newULID(), ComponentKind: contracts.RecipeComponentKindItem, InventoryItemID: &paneer.ID,
				QuantityMicro: 200_000_000, QuantityDimension: contracts.DimensionMass},
		},
	})
	if err != nil {
		t.Fatalf("CreateRecipe: %v", err)
	}
	// Assert the fixture actually landed before asserting anything about
	// isolation — a failed insert would make every later assertion here
	// trivially pass on absent data.
	if recipe.ID == "" || len(ingredients) != 1 {
		t.Fatalf("recipe fixture did not persist as expected: recipe=%+v ingredients=%d", recipe, len(ingredients))
	}

	// Tenant B may never read tenant A's outlet at all.
	if _, err := svc.SyncConfigBundle(ctx, fxB.tenantID, fxA.outletID, 0); !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden for cross-tenant outlet probe, got %v", err)
	}

	// Tenant B's own bundle must never contain tenant A's recipe/ingredient.
	bundleB, err := svc.SyncConfigBundle(ctx, fxB.tenantID, fxB.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle B: %v", err)
	}
	for _, r := range bundleB.Recipes {
		if r.ID == recipe.ID {
			t.Fatal("tenant B's bundle leaked tenant A's recipe")
		}
	}
	for _, ri := range bundleB.RecipeIngredients {
		if ri.RecipeID == recipe.ID {
			t.Fatal("tenant B's bundle leaked tenant A's recipe_ingredient")
		}
	}

	// Sanity: tenant A can see its own recipe and ingredient.
	bundleA, err := svc.SyncConfigBundle(ctx, fxA.tenantID, fxA.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle A: %v", err)
	}
	foundRecipe := false
	for _, r := range bundleA.Recipes {
		if r.ID == recipe.ID {
			foundRecipe = true
		}
	}
	if !foundRecipe {
		t.Fatal("tenant A's own bundle is missing its own recipe")
	}
}

// TestPostgresRepository_CrossTenantModifierIngredientDeltaIsIsolated covers
// modifier_ingredient_delta specifically: it has no write route of its own
// (ADR-018 §1 — it rides in the MenuItem config payload), so this test
// inserts the row directly, the way backend/internal/menu will once it owns
// that write path, and asserts the read side (SyncConfigBundle) still never
// leaks it across tenants.
func TestPostgresRepository_CrossTenantModifierIngredientDeltaIsIsolated(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	repo := inventory.NewRepository(pool)
	svc := inventory.NewService(repo)

	fxA := newFixture(t, pool, "Cross Tenant Delta A")
	fxB := newFixture(t, pool, "Cross Tenant Delta B")

	paneer := newInventoryItem(t, svc, ctx, fxA.tenantID, fxA.outletID, "PANEER-DELTA-A", contracts.DimensionMass)

	modifierID := newULID()
	if _, err := pool.Exec(ctx,
		`INSERT INTO menu_item_modifier (id, menu_item_id, group_name, option_name, price_delta_paise, min_selection, max_selection, config_version)
		 VALUES ($1, $2, 'Extras', 'Extra Paneer', 4000, 0, 1, 1)`,
		modifierID, fxA.menuItemID,
	); err != nil {
		t.Fatalf("seeding menu_item_modifier: %v", err)
	}
	deltaID := newULID()
	if _, err := pool.Exec(ctx,
		`INSERT INTO modifier_ingredient_delta (id, menu_item_modifier_id, inventory_item_id, quantity_micro, config_version)
		 VALUES ($1, $2, $3, 50000000, 1)`,
		deltaID, modifierID, paneer.ID,
	); err != nil {
		t.Fatalf("seeding modifier_ingredient_delta: %v", err)
	}
	// Assert the fixture landed before asserting anything about isolation.
	var count int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM modifier_ingredient_delta WHERE id = $1`, deltaID).Scan(&count); err != nil {
		t.Fatalf("counting seeded modifier_ingredient_delta: %v", err)
	}
	if count != 1 {
		t.Fatalf("modifier_ingredient_delta fixture did not persist: count=%d", count)
	}

	bundleB, err := svc.SyncConfigBundle(ctx, fxB.tenantID, fxB.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle B: %v", err)
	}
	for _, d := range bundleB.ModifierIngredientDeltas {
		if d.ID == deltaID {
			t.Fatal("tenant B's bundle leaked tenant A's modifier_ingredient_delta")
		}
	}

	bundleA, err := svc.SyncConfigBundle(ctx, fxA.tenantID, fxA.outletID, 0)
	if err != nil {
		t.Fatalf("SyncConfigBundle A: %v", err)
	}
	found := false
	for _, d := range bundleA.ModifierIngredientDeltas {
		if d.ID == deltaID {
			found = true
		}
	}
	if !found {
		t.Fatal("tenant A's own bundle is missing its own modifier_ingredient_delta")
	}
}

// TestCreateRecipe_CycleGuardRejectsAndNamesThePath is ADR-018 §7's cycle
// guard, falsified before being trusted: Gravy references itself indirectly
// through Curry, and Curry references Gravy — the second write must be
// rejected with the offending path named, never silently accepted.
func TestCreateRecipe_CycleGuardRejectsAndNamesThePath(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	svc := inventory.NewService(inventory.NewRepository(pool))
	fx := newFixture(t, pool, "Cycle Guard")

	// A second sellable variant so Gravy and Curry can each bind their own
	// recipe (recipe.menu_item_variant_id is unique).
	menuSvc := menu.NewService(menu.NewRepository(pool))
	menuCtx := menu.WithPrincipal(ctx, auth.NewPrincipal(auth.AuthenticatedPrincipal{
		UserID: "principal-user", TenantID: fx.tenantID, OutletID: fx.outletID,
		Permissions: []auth.Permission{auth.PermissionMenuManage},
	}))
	category, err := menuSvc.CreateCategory(menuCtx, menu.NewCategoryInput{OutletID: fx.outletID, Name: "Gravies", SortOrder: 2})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	gravyItem, gravyVariants, _, err := menuSvc.CreateItem(menuCtx, menu.NewItemInput{
		OutletID: fx.outletID, CategoryID: category.ID, Name: "Gravy Dish", BasePricePaise: 1,
		Variants: []menu.NewVariantInput{{Name: "Regular"}},
	})
	if err != nil || len(gravyVariants) != 1 {
		t.Fatalf("CreateItem gravy: item=%+v variants=%d err=%v", gravyItem, len(gravyVariants), err)
	}

	// Curry (fx.variantID) starts with no sub-recipe ingredient — a plain
	// ITEM recipe — so it exists to be referenced.
	curryItem := newInventoryItem(t, svc, ctx, fx.tenantID, fx.outletID, "CURRY-BASE", contracts.DimensionMass)
	curry, _, err := svc.CreateRecipe(ctx, fx.tenantID, inventory.NewRecipeInput{
		ID: newULID(), MenuItemVariantID: fx.variantID, Name: "Curry",
		OutputDimension: contracts.DimensionCount, OutputQuantityMicro: 1_000_000,
		Ingredients: []inventory.NewRecipeIngredientInput{
			{ID: newULID(), ComponentKind: contracts.RecipeComponentKindItem, InventoryItemID: &curryItem.ID,
				QuantityMicro: 100_000_000, QuantityDimension: contracts.DimensionMass},
		},
	})
	if err != nil {
		t.Fatalf("CreateRecipe curry: %v", err)
	}

	// Gravy references Curry as a sub-recipe — legal, no cycle yet.
	gravy, _, err := svc.CreateRecipe(ctx, fx.tenantID, inventory.NewRecipeInput{
		ID: newULID(), MenuItemVariantID: gravyVariants[0].ID, Name: "Gravy",
		OutputDimension: contracts.DimensionVolume, OutputQuantityMicro: 300_000_000,
		Ingredients: []inventory.NewRecipeIngredientInput{
			{ID: newULID(), ComponentKind: contracts.RecipeComponentKindSubRecipe, SubRecipeID: &curry.ID,
				QuantityMicro: 1_000_000, QuantityDimension: contracts.DimensionCount},
		},
	})
	if err != nil {
		t.Fatalf("CreateRecipe gravy (legal sub-recipe reference): %v", err)
	}

	// Now try to make Curry reference Gravy — Curry -> Gravy -> Curry is a
	// cycle and must be rejected, never silently accepted.
	_, _, err = svc.CreateRecipe(ctx, fx.tenantID, inventory.NewRecipeInput{
		ID: curry.ID, MenuItemVariantID: fx.variantID, Name: "Curry",
		OutputDimension: contracts.DimensionCount, OutputQuantityMicro: 1_000_000,
		Ingredients: []inventory.NewRecipeIngredientInput{
			{ID: newULID(), ComponentKind: contracts.RecipeComponentKindSubRecipe, SubRecipeID: &gravy.ID,
				QuantityMicro: 1_000_000, QuantityDimension: contracts.DimensionVolume},
		},
	})
	if !errors.Is(err, inventory.ErrRecipeCycle) {
		t.Fatalf("expected ErrRecipeCycle, got %v", err)
	}
	if err == nil || !contains(err.Error(), gravy.ID) {
		t.Fatalf("expected the cycle error to name the offending sub-recipe %s, got: %v", gravy.ID, err)
	}
}

func contains(haystack, needle string) bool {
	return len(needle) == 0 || (len(haystack) >= len(needle) && indexOf(haystack, needle) >= 0)
}

func indexOf(haystack, needle string) int {
	for i := 0; i+len(needle) <= len(haystack); i++ {
		if haystack[i:i+len(needle)] == needle {
			return i
		}
	}
	return -1
}

// TestCreateRecipe_DimensionMismatchIsRejected422 covers ADR-018 0.5.2: a
// recipe_ingredient's author-chosen quantity_dimension must match its
// referent's own dimension, never silently converted.
func TestCreateRecipe_DimensionMismatchIsRejected422(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	svc := inventory.NewService(inventory.NewRepository(pool))
	fx := newFixture(t, pool, "Dimension Mismatch")

	oil := newInventoryItem(t, svc, ctx, fx.tenantID, fx.outletID, "OIL", contracts.DimensionVolume)

	_, _, err := svc.CreateRecipe(ctx, fx.tenantID, inventory.NewRecipeInput{
		ID: newULID(), MenuItemVariantID: fx.variantID, Name: "Mismatch Dish",
		OutputDimension: contracts.DimensionCount, OutputQuantityMicro: 1_000_000,
		Ingredients: []inventory.NewRecipeIngredientInput{
			// oil is VOLUME; author claims MASS — an authoring error.
			{ID: newULID(), ComponentKind: contracts.RecipeComponentKindItem, InventoryItemID: &oil.ID,
				QuantityMicro: 10_000_000, QuantityDimension: contracts.DimensionMass},
		},
	})
	if !errors.Is(err, inventory.ErrDimensionMismatch) {
		t.Fatalf("expected ErrDimensionMismatch, got %v", err)
	}
}

// TestIngestLedgerEntry_SequenceGapIsRejectedAndNamesTheMissingEntry is the
// replay addendum's contiguity check: entry_seq 1,2,3 succeed, then 5 (skipping
// 4) must fail loudly naming entry 4 as missing — never a silent skip.
func TestIngestLedgerEntry_SequenceGapIsRejectedAndNamesTheMissingEntry(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	svc := inventory.NewService(inventory.NewRepository(pool))
	fx := newFixture(t, pool, "Ledger Gap")

	item := newInventoryItem(t, svc, ctx, fx.tenantID, fx.outletID, "GAP-ITEM", contracts.DimensionMass)

	send := func(seq int64) (contracts.StockLedgerEntry, error) {
		id := newULID()
		env := ledgerEnvelope(id, fx.tenantID, fx.outletID, 1)
		entry := contracts.StockLedgerEntry{
			ID: id, OutletID: fx.outletID, EntrySeq: seq,
			InventoryItemID: item.ID, InventoryItemName: item.Name, Dimension: item.Dimension,
			EntryType: contracts.StockEntryTypeConsumption, Origin: contracts.StockEntryOriginManual,
			QuantityAppliedMicro: -1_000_000,
			OccurredAt:           time.Now().UTC().Format(time.RFC3339),
			BusinessDate:         time.Now().UTC().Format("2006-01-02"),
			SchemaVersion:        1,
		}
		return svc.IngestLedgerEntry(ctx, fx.tenantID, env, entry)
	}

	for _, seq := range []int64{1, 2, 3} {
		if _, err := send(seq); err != nil {
			t.Fatalf("ingesting entry_seq %d: %v", seq, err)
		}
	}

	_, err := send(5)
	if !errors.Is(err, inventory.ErrLedgerSequenceGap) {
		t.Fatalf("expected ErrLedgerSequenceGap for entry_seq 5 after 3, got %v", err)
	}
	if !contains(err.Error(), "4") {
		t.Fatalf("expected the gap error to name the missing entry_seq 4, got: %v", err)
	}
}

func ledgerEnvelope(recordID, tenantID, outletID string, version int) contracts.SyncEnvelope {
	return contracts.SyncEnvelope{
		RecordID: recordID, TenantID: tenantID, OutletID: outletID,
		DeviceID:      "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
		AggregateType: contracts.AggregateTypeStockLedgerEntry,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		Version:       version, SyncStatus: contracts.SyncStatusPending,
	}
}
