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

// TestIngestLedgerEntry_AHoleIsRecordedAndTheStreamKeepsMoving is the
// contiguity check as it must behave, and the regression test for the defect
// this replaced.
//
// THE DEFECT. Shipped T4 code rejected any entry_seq beyond the high-water
// mark. One genuinely lost entry therefore halted that outlet's ledger replay
// permanently: entry 5 rejected, 6, 7, 8… all rejected behind it, forever,
// with nothing downstream able to tell "quiet outlet" from "wedged since
// Tuesday". Detection is the goal; blocking was a side effect nobody wanted.
//
// So: the hole is recorded and entry 5 is STORED, and — the half that proves
// there is no outage — entry 6 lands afterwards.
func TestIngestLedgerEntry_AHoleIsRecordedAndTheStreamKeepsMoving(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	repo := inventory.NewRepository(pool)
	svc := inventory.NewService(repo)
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

	// 4 never arrives.
	if _, err := send(5); err != nil {
		t.Fatalf("entry_seq 5 must be ACCEPTED despite the hole at 4, got: %v", err)
	}

	gaps, err := repo.ListUnresolvedReplayGaps(ctx, fx.outletID)
	if err != nil {
		t.Fatalf("listing replay gaps: %v", err)
	}
	if len(gaps) != 1 {
		t.Fatalf("expected exactly one recorded hole, got %d: %+v", len(gaps), gaps)
	}
	if gaps[0].Stream != inventory.ReplayStreamLedger ||
		gaps[0].FromEntrySeq != 4 || gaps[0].ToEntrySeq != 4 {
		t.Fatalf("expected LEDGER hole 4..4, got %+v", gaps[0])
	}
	if gaps[0].ResolvedAt != nil {
		t.Fatalf("a hole that has not filled must stay unresolved, got %+v", gaps[0])
	}

	// The whole point: replay is not wedged.
	if _, err := send(6); err != nil {
		t.Fatalf("entry_seq 6 must still replay after a recorded hole, got: %v", err)
	}
}

// TestIngestLedgerEntry_ReObservingTheSameHoleStaysOneRow. The edge retries a
// batch and the same hole is seen again; that is not new information. Without
// the UNIQUE key and its upsert, one hole becomes N rows and the table
// degrades into the log-line outcome it exists to avoid — unreadable, so
// unread.
func TestIngestLedgerEntry_ReObservingTheSameHoleStaysOneRow(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	repo := inventory.NewRepository(pool)
	fx := newFixture(t, pool, "Hole Reobserved")

	now := time.Now().UTC()
	for i := 0; i < 3; i++ {
		if err := repo.RecordReplayGap(ctx, fx.outletID, inventory.ReplayStreamLedger, 4, 4, now); err != nil {
			t.Fatalf("recording the hole (observation %d): %v", i+1, err)
		}
	}

	gaps, err := repo.ListUnresolvedReplayGaps(ctx, fx.outletID)
	if err != nil {
		t.Fatalf("listing replay gaps: %v", err)
	}
	if len(gaps) != 1 {
		t.Fatalf("three observations of one hole must stay one row, got %d", len(gaps))
	}
	if gaps[0].ObservationCount != 3 {
		t.Fatalf("expected observation_count 3, got %d", gaps[0].ObservationCount)
	}
}

// TestIngestLedgerEntry_AFilledHoleResolves. A hole that later fills is not a
// loss — late arrival is ordinary. A row still claiming a permanent loss that
// has healed is a false alarm, and a table of false alarms is one nobody
// reads.
func TestIngestLedgerEntry_AFilledHoleResolves(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	repo := inventory.NewRepository(pool)
	svc := inventory.NewService(repo)
	fx := newFixture(t, pool, "Hole Filled")

	item := newInventoryItem(t, svc, ctx, fx.tenantID, fx.outletID, "FILL-ITEM", contracts.DimensionMass)

	send := func(seq int64) error {
		id := newULID()
		env := ledgerEnvelope(id, fx.tenantID, fx.outletID, 1)
		_, err := svc.IngestLedgerEntry(ctx, fx.tenantID, env, contracts.StockLedgerEntry{
			ID: id, OutletID: fx.outletID, EntrySeq: seq,
			InventoryItemID: item.ID, InventoryItemName: item.Name, Dimension: item.Dimension,
			EntryType: contracts.StockEntryTypeConsumption, Origin: contracts.StockEntryOriginManual,
			QuantityAppliedMicro: -1_000_000,
			OccurredAt:           time.Now().UTC().Format(time.RFC3339),
			BusinessDate:         time.Now().UTC().Format("2006-01-02"),
			SchemaVersion:        1,
		})
		return err
	}

	if err := send(1); err != nil {
		t.Fatalf("entry 1: %v", err)
	}
	if err := send(3); err != nil { // hole at 2
		t.Fatalf("entry 3: %v", err)
	}
	gaps, err := repo.ListUnresolvedReplayGaps(ctx, fx.outletID)
	if err != nil || len(gaps) != 1 {
		t.Fatalf("expected one open hole, got %d (err %v)", len(gaps), err)
	}

	// 2 arrives late, out of order — exactly the case that must not leave a
	// permanent alarm behind.
	if err := send(2); err != nil {
		t.Fatalf("the late entry must be accepted: %v", err)
	}

	gaps, err = repo.ListUnresolvedReplayGaps(ctx, fx.outletID)
	if err != nil {
		t.Fatalf("listing replay gaps: %v", err)
	}
	if len(gaps) != 0 {
		t.Fatalf("a filled hole must resolve, still open: %+v", gaps)
	}
}

// TestIngestLedgerEntry_SameIDReIngestIsASilentNoOp. A dropped ack means the
// edge resends a row the cloud already stored. That is an ordinary retry: it
// must return the stored row quietly, never a conflict, or every reconnect
// manufactures a false alarm. It must also not be read as a reused mark —
// which is why the idempotency check runs BEFORE the contiguity check.
func TestIngestLedgerEntry_SameIDReIngestIsASilentNoOp(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	repo := inventory.NewRepository(pool)
	svc := inventory.NewService(repo)
	fx := newFixture(t, pool, "Same ID Retry")

	item := newInventoryItem(t, svc, ctx, fx.tenantID, fx.outletID, "RETRY-ITEM", contracts.DimensionMass)

	id := newULID()
	env := ledgerEnvelope(id, fx.tenantID, fx.outletID, 1)
	entry := contracts.StockLedgerEntry{
		ID: id, OutletID: fx.outletID, EntrySeq: 1,
		InventoryItemID: item.ID, InventoryItemName: item.Name, Dimension: item.Dimension,
		EntryType: contracts.StockEntryTypeConsumption, Origin: contracts.StockEntryOriginManual,
		QuantityAppliedMicro: -1_000_000,
		OccurredAt:           time.Now().UTC().Format(time.RFC3339),
		BusinessDate:         time.Now().UTC().Format("2006-01-02"),
		SchemaVersion:        1,
	}

	first, err := svc.IngestLedgerEntry(ctx, fx.tenantID, env, entry)
	if err != nil {
		t.Fatalf("first ingest: %v", err)
	}
	second, err := svc.IngestLedgerEntry(ctx, fx.tenantID, env, entry)
	if err != nil {
		t.Fatalf("re-ingesting the same id must be a quiet no-op, got: %v", err)
	}
	if second.ID != first.ID || second.EntrySeq != first.EntrySeq {
		t.Fatalf("the retry must return the stored row, got %+v want %+v", second, first)
	}

	gaps, err := repo.ListUnresolvedReplayGaps(ctx, fx.outletID)
	if err != nil {
		t.Fatalf("listing replay gaps: %v", err)
	}
	if len(gaps) != 0 {
		t.Fatalf("an ordinary retry must record no hole, got %+v", gaps)
	}
}

// TestIngestLedgerEntry_AReusedMarkUnderADifferentIDIsRefused. The one
// contiguity condition that still rejects: two rows cannot claim one
// position, or the mark the sealed snapshot and the gap detection both read
// becomes ambiguous. Unreachable through the edge's own durable counter;
// reaching it means something upstream is minting marks it does not own.
func TestIngestLedgerEntry_AReusedMarkUnderADifferentIDIsRefused(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	svc := inventory.NewService(inventory.NewRepository(pool))
	fx := newFixture(t, pool, "Reused Mark")

	item := newInventoryItem(t, svc, ctx, fx.tenantID, fx.outletID, "REUSE-ITEM", contracts.DimensionMass)

	send := func(seq int64) error {
		id := newULID()
		env := ledgerEnvelope(id, fx.tenantID, fx.outletID, 1)
		_, err := svc.IngestLedgerEntry(ctx, fx.tenantID, env, contracts.StockLedgerEntry{
			ID: id, OutletID: fx.outletID, EntrySeq: seq,
			InventoryItemID: item.ID, InventoryItemName: item.Name, Dimension: item.Dimension,
			EntryType: contracts.StockEntryTypeConsumption, Origin: contracts.StockEntryOriginManual,
			QuantityAppliedMicro: -1_000_000,
			OccurredAt:           time.Now().UTC().Format(time.RFC3339),
			BusinessDate:         time.Now().UTC().Format("2006-01-02"),
			SchemaVersion:        1,
		})
		return err
	}

	if err := send(1); err != nil {
		t.Fatalf("entry 1: %v", err)
	}
	if err := send(1); !errors.Is(err, inventory.ErrLedgerSequenceMarkReused) {
		t.Fatalf("expected ErrLedgerSequenceMarkReused for a reused mark, got %v", err)
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

// TestCreateRecipe_DepthExceededIsRejected covers the other half of ADR-018
// §7's cycle guard — bounded depth, not just cycles — flagged as an open
// risk (no dedicated test) in the M4 T4 report and closed here.
//
// Builds a linear chain R1 -> R2 -> ... -> R9 (nine recipes, eight
// sub-recipe links, so R9 sits at depth 9 below R1), then tries to create a
// brand-new recipe referencing R1 as a sub-recipe. Reachable-from-R1 includes
// R9 at depth 9, which exceeds MaxRecipeDepth (8), so the write must be
// rejected — never silently accepted into a graph the edge's own resolver
// would then have to walk nine levels deep inside confirm_order's
// transaction.
func TestCreateRecipe_DepthExceededIsRejected(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	svc := inventory.NewService(inventory.NewRepository(pool))
	fx := newFixture(t, pool, "Depth Exceeded")

	menuSvc := menu.NewService(menu.NewRepository(pool))
	menuCtx := menu.WithPrincipal(ctx, auth.NewPrincipal(auth.AuthenticatedPrincipal{
		UserID: "principal-user", TenantID: fx.tenantID, OutletID: fx.outletID,
		Permissions: []auth.Permission{auth.PermissionMenuManage},
	}))
	category, err := menuSvc.CreateCategory(menuCtx, menu.NewCategoryInput{OutletID: fx.outletID, Name: "Depth Chain", SortOrder: 3})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}

	const chainLength = 9 // depths 1..9 below the proposed reference; 9 > MaxRecipeDepth(8)
	variantIDs := make([]string, chainLength)
	for i := 0; i < chainLength; i++ {
		item, variants, _, err := menuSvc.CreateItem(menuCtx, menu.NewItemInput{
			OutletID: fx.outletID, CategoryID: category.ID, Name: "Chain Dish", BasePricePaise: 1,
			Variants: []menu.NewVariantInput{{Name: "Regular"}},
		})
		if err != nil || len(variants) != 1 {
			t.Fatalf("CreateItem chain link %d: item=%+v variants=%d err=%v", i, item, len(variants), err)
		}
		variantIDs[i] = variants[0].ID
	}

	recipeIDs := make([]string, chainLength)
	for i := chainLength - 1; i >= 0; i-- {
		in := inventory.NewRecipeInput{
			ID: newULID(), MenuItemVariantID: variantIDs[i], Name: "Chain Link",
			OutputDimension: contracts.DimensionCount, OutputQuantityMicro: 1_000_000,
		}
		if i < chainLength-1 {
			nextID := recipeIDs[i+1]
			in.Ingredients = []inventory.NewRecipeIngredientInput{
				{ID: newULID(), ComponentKind: contracts.RecipeComponentKindSubRecipe, SubRecipeID: &nextID,
					QuantityMicro: 1_000_000, QuantityDimension: contracts.DimensionCount},
			}
		}
		recipe, _, err := svc.CreateRecipe(ctx, fx.tenantID, in)
		if err != nil {
			t.Fatalf("CreateRecipe chain link %d: %v", i, err)
		}
		recipeIDs[i] = recipe.ID
	}
	// Assert the chain actually persisted the full length before asserting
	// anything about the depth guard — a short chain would make the
	// rejection below pass for the wrong reason (or not fire at all).
	if len(recipeIDs) != chainLength || recipeIDs[chainLength-1] == "" {
		t.Fatalf("recipe chain did not persist as expected: %v", recipeIDs)
	}

	// A brand-new recipe on its own variant, referencing R1 (recipeIDs[0])
	// as a sub-recipe: reachable-from-R1 bottoms out at R9, depth 9.
	rootItem, rootVariants, _, err := menuSvc.CreateItem(menuCtx, menu.NewItemInput{
		OutletID: fx.outletID, CategoryID: category.ID, Name: "Depth Probe Root", BasePricePaise: 1,
		Variants: []menu.NewVariantInput{{Name: "Regular"}},
	})
	if err != nil || len(rootVariants) != 1 {
		t.Fatalf("CreateItem root: item=%+v variants=%d err=%v", rootItem, len(rootVariants), err)
	}

	firstLinkID := recipeIDs[0]
	_, _, err = svc.CreateRecipe(ctx, fx.tenantID, inventory.NewRecipeInput{
		ID: newULID(), MenuItemVariantID: rootVariants[0].ID, Name: "Depth Probe",
		OutputDimension: contracts.DimensionCount, OutputQuantityMicro: 1_000_000,
		Ingredients: []inventory.NewRecipeIngredientInput{
			{ID: newULID(), ComponentKind: contracts.RecipeComponentKindSubRecipe, SubRecipeID: &firstLinkID,
				QuantityMicro: 1_000_000, QuantityDimension: contracts.DimensionCount},
		},
	})
	if !errors.Is(err, inventory.ErrRecipeDepthExceeded) {
		t.Fatalf("expected ErrRecipeDepthExceeded, got %v", err)
	}
}
