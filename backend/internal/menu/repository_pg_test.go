package menu

import (
	"context"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/testdb"
)

// TestRepository_Postgres exercises the real pgRepository against a live
// PostgreSQL instance. See internal/platform/testdb: an unset
// HOLLER_TEST_DATABASE_URL fails this test loudly by default.
func TestRepository_Postgres(t *testing.T) {
	dsn := testdb.RequireDatabaseURL(t)

	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connecting to test database: %v", err)
	}
	defer pool.Close()

	tenantID := id.New()
	brandID := id.New()
	outletID := id.New()

	if _, err := pool.Exec(ctx, `INSERT INTO tenant (id, name) VALUES ($1, 'Integration Test Tenant')`, tenantID); err != nil {
		t.Fatalf("seeding tenant: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO brand (id, tenant_id, name) VALUES ($1, $2, 'Integration Test Brand')`, brandID, tenantID); err != nil {
		t.Fatalf("seeding brand: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO outlet (id, brand_id, name, config_version) VALUES ($1, $2, 'Integration Test Outlet', 0)`, outletID, brandID); err != nil {
		t.Fatalf("seeding outlet: %v", err)
	}
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM menu_item_modifier WHERE menu_item_id IN (SELECT id FROM menu_item WHERE outlet_id = $1)`, outletID)
		pool.Exec(ctx, `DELETE FROM menu_item_variant WHERE menu_item_id IN (SELECT id FROM menu_item WHERE outlet_id = $1)`, outletID)
		pool.Exec(ctx, `DELETE FROM menu_item WHERE outlet_id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM menu_category WHERE outlet_id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE id = $1`, outletID)
		pool.Exec(ctx, `DELETE FROM brand WHERE id = $1`, brandID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, tenantID)
	})

	repo := NewRepository(pool)
	svc := NewService(repo)

	cat, err := svc.CreateCategory(authorizedContext(), NewCategoryInput{OutletID: outletID, Name: "Starters", SortOrder: 1})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	if cat.ConfigVersion != 1 {
		t.Fatalf("expected first write to bump outlet config_version to 1, got %d", cat.ConfigVersion)
	}

	item, variants, modifiers, err := svc.CreateItem(authorizedContext(), NewItemInput{
		OutletID:       outletID,
		CategoryID:     cat.ID,
		Name:           "Paneer Tikka",
		BasePricePaise: 28000,
		Variants:       []NewVariantInput{{Name: "Half", PriceDeltaPaise: -8000}},
		Modifiers:      []NewModifierInput{{GroupName: "Spice", OptionName: "Hot", MinSelection: 1, MaxSelection: 1}},
	})
	if err != nil {
		t.Fatalf("CreateItem: %v", err)
	}
	if item.ConfigVersion != 2 {
		t.Fatalf("expected item write to bump outlet config_version to 2, got %d", item.ConfigVersion)
	}
	if len(variants) != 1 || variants[0].ConfigVersion != 2 {
		t.Fatalf("expected variant stamped with config_version 2, got %+v", variants)
	}
	if len(modifiers) != 1 || modifiers[0].ConfigVersion != 2 {
		t.Fatalf("expected modifier stamped with config_version 2, got %+v", modifiers)
	}

	items, err := svc.ListItems(context.Background(), outletID)
	if err != nil {
		t.Fatalf("ListItems: %v", err)
	}
	if len(items) != 1 {
		t.Fatalf("expected 1 item, got %d", len(items))
	}

	updated, err := svc.SetItemAvailability(authorizedContext(), outletID, item.ID, false)
	if err != nil {
		t.Fatalf("SetItemAvailability: %v", err)
	}
	if updated.ConfigVersion != 3 {
		t.Fatalf("expected availability write to bump outlet config_version to 3, got %d", updated.ConfigVersion)
	}
}
