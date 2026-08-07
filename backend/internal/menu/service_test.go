package menu

import (
	"context"
	"errors"
	"testing"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// fakeRepository is an in-memory Repository used to unit test Service
// without a database, so config_version bumping and price/validation logic
// can be exercised in isolation.
type fakeRepository struct {
	outletVersions map[string]int
	categories     []Category
	items          []Item
	variants       []Variant
	modifiers      []Modifier
	bumpCalls      int
}

func newFakeRepository(outletID string) *fakeRepository {
	return &fakeRepository{
		outletVersions: map[string]int{outletID: 0},
	}
}

func (f *fakeRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	return fn(nil)
}

func (f *fakeRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	f.bumpCalls++
	v, ok := f.outletVersions[outletID]
	if !ok {
		return 0, httpx.ErrNotFound
	}
	v++
	f.outletVersions[outletID] = v
	return v, nil
}

func (f *fakeRepository) ListCategories(ctx context.Context, outletID string) ([]Category, error) {
	var out []Category
	for _, c := range f.categories {
		if c.OutletID == outletID {
			out = append(out, c)
		}
	}
	return out, nil
}

func (f *fakeRepository) InsertCategory(ctx context.Context, tx pgx.Tx, c Category) error {
	f.categories = append(f.categories, c)
	return nil
}

func (f *fakeRepository) ListItems(ctx context.Context, outletID string) ([]Item, error) {
	var out []Item
	for _, i := range f.items {
		if i.OutletID == outletID {
			out = append(out, i)
		}
	}
	return out, nil
}

func (f *fakeRepository) InsertItem(ctx context.Context, tx pgx.Tx, i Item) error {
	f.items = append(f.items, i)
	return nil
}

func (f *fakeRepository) GetItem(ctx context.Context, outletID, itemID string) (Item, error) {
	for _, i := range f.items {
		if i.ID == itemID && i.OutletID == outletID {
			return i, nil
		}
	}
	return Item{}, httpx.ErrNotFound
}

func (f *fakeRepository) UpdateItemAvailability(ctx context.Context, tx pgx.Tx, itemID string, isAvailable bool, configVersion int) error {
	for idx := range f.items {
		if f.items[idx].ID == itemID {
			f.items[idx].IsAvailable = isAvailable
			f.items[idx].ConfigVersion = configVersion
			return nil
		}
	}
	return httpx.ErrNotFound
}

func (f *fakeRepository) CategoryExists(ctx context.Context, outletID, categoryID string) (bool, error) {
	for _, c := range f.categories {
		if c.ID == categoryID && c.OutletID == outletID {
			return true, nil
		}
	}
	return false, nil
}

func (f *fakeRepository) InsertVariant(ctx context.Context, tx pgx.Tx, v Variant) error {
	f.variants = append(f.variants, v)
	return nil
}

func (f *fakeRepository) InsertModifier(ctx context.Context, tx pgx.Tx, m Modifier) error {
	f.modifiers = append(f.modifiers, m)
	return nil
}

type fakePrincipal struct{ permissions map[string]bool }

func (p fakePrincipal) HasPermission(permission string) bool { return p.permissions[permission] }

func authorizedContext() context.Context {
	return WithPrincipal(context.Background(), fakePrincipal{permissions: map[string]bool{permMenuManage: true}})
}

func unauthorizedContext() context.Context {
	return WithPrincipal(context.Background(), fakePrincipal{permissions: map[string]bool{}})
}

func TestCreateCategory_BumpsConfigVersionExactlyOnce(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	c, err := svc.CreateCategory(authorizedContext(), NewCategoryInput{OutletID: outletID, Name: "Starters", SortOrder: 1})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	if repo.bumpCalls != 1 {
		t.Fatalf("expected exactly 1 config_version bump, got %d", repo.bumpCalls)
	}
	if c.ConfigVersion != 1 {
		t.Fatalf("expected category config_version 1, got %d", c.ConfigVersion)
	}
	if repo.outletVersions[outletID] != 1 {
		t.Fatalf("expected outlet config_version 1, got %d", repo.outletVersions[outletID])
	}
}

func TestListCategories_DoesNotBumpConfigVersion(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	if _, err := svc.ListCategories(context.Background(), outletID); err != nil {
		t.Fatalf("ListCategories: %v", err)
	}
	if repo.bumpCalls != 0 {
		t.Fatalf("expected a read to never bump config_version, got %d bumps", repo.bumpCalls)
	}
	if repo.outletVersions[outletID] != 0 {
		t.Fatalf("expected outlet config_version unchanged at 0, got %d", repo.outletVersions[outletID])
	}
}

func TestCreateItem_WithVariantsAndModifiers_BumpsOnce(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	cat, err := svc.CreateCategory(authorizedContext(), NewCategoryInput{OutletID: outletID, Name: "Mains", SortOrder: 0})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	repo.bumpCalls = 0 // reset after setup so we measure only the item write

	item, variants, modifiers, err := svc.CreateItem(authorizedContext(), NewItemInput{
		OutletID:       outletID,
		CategoryID:     cat.ID,
		Name:           "Butter Chicken",
		BasePricePaise: 41000,
		Variants: []NewVariantInput{
			{Name: "Half", PriceDeltaPaise: -10000},
			{Name: "Full", PriceDeltaPaise: 0},
		},
		Modifiers: []NewModifierInput{
			{GroupName: "Spice", OptionName: "Mild", PriceDeltaPaise: 0, MinSelection: 1, MaxSelection: 1},
			{GroupName: "Spice", OptionName: "Hot", PriceDeltaPaise: 0, MinSelection: 1, MaxSelection: 1},
		},
	})
	if err != nil {
		t.Fatalf("CreateItem: %v", err)
	}
	if repo.bumpCalls != 1 {
		t.Fatalf("expected exactly 1 config_version bump for the whole item write, got %d", repo.bumpCalls)
	}
	if item.ConfigVersion != 2 {
		t.Fatalf("expected item config_version 2 (after category's bump), got %d", item.ConfigVersion)
	}
	for _, v := range variants {
		if v.ConfigVersion != item.ConfigVersion {
			t.Fatalf("expected variant config_version %d to match item, got %d", item.ConfigVersion, v.ConfigVersion)
		}
	}
	for _, m := range modifiers {
		if m.ConfigVersion != item.ConfigVersion {
			t.Fatalf("expected modifier config_version %d to match item, got %d", item.ConfigVersion, m.ConfigVersion)
		}
	}

	// Price composition: base 41000 - 10000 (Half) + 0 (Hot) = 31000 paise.
	got := ComposePrice(item.BasePricePaise, variants[0].PriceDeltaPaise, []int64{modifiers[1].PriceDeltaPaise})
	if got != 31000 {
		t.Fatalf("ComposePrice() = %d paise, want 31000 paise", got)
	}
}

func TestCreateItem_RejectsUnknownCategory(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	_, _, _, err := svc.CreateItem(authorizedContext(), NewItemInput{
		OutletID:       outletID,
		CategoryID:     id.New(), // never created
		Name:           "Ghost Item",
		BasePricePaise: 1000,
	})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for unknown category, got %v", err)
	}
	if repo.bumpCalls != 0 {
		t.Fatalf("a rejected write must not bump config_version, got %d bumps", repo.bumpCalls)
	}
}

func TestCreateItem_RejectsInconsistentModifierGroupBounds(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	cat, err := svc.CreateCategory(authorizedContext(), NewCategoryInput{OutletID: outletID, Name: "Mains", SortOrder: 0})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}

	_, _, _, err = svc.CreateItem(authorizedContext(), NewItemInput{
		OutletID:       outletID,
		CategoryID:     cat.ID,
		Name:           "Pizza",
		BasePricePaise: 30000,
		Modifiers: []NewModifierInput{
			{GroupName: "Size", OptionName: "Regular", MinSelection: 1, MaxSelection: 1},
			{GroupName: "Size", OptionName: "Large", MinSelection: 0, MaxSelection: 1}, // inconsistent min
		},
	})
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for inconsistent group bounds, got %v", err)
	}
}

func TestCreateCategory_RequiresPermission(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	_, err := svc.CreateCategory(unauthorizedContext(), NewCategoryInput{OutletID: outletID, Name: "Starters"})
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected ErrForbidden, got %v", err)
	}
	if repo.bumpCalls != 0 {
		t.Fatalf("forbidden write must not bump config_version, got %d bumps", repo.bumpCalls)
	}

	_, err = svc.CreateCategory(context.Background(), NewCategoryInput{OutletID: outletID, Name: "Starters"})
	if !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected ErrUnauthorized without a principal in context, got %v", err)
	}
}

func TestSetItemAvailability_BumpsConfigVersionExactlyOnce(t *testing.T) {
	outletID := id.New()
	repo := newFakeRepository(outletID)
	svc := NewService(repo)

	cat, err := svc.CreateCategory(authorizedContext(), NewCategoryInput{OutletID: outletID, Name: "Mains"})
	if err != nil {
		t.Fatalf("CreateCategory: %v", err)
	}
	item, _, _, err := svc.CreateItem(authorizedContext(), NewItemInput{
		OutletID: outletID, CategoryID: cat.ID, Name: "Dal", BasePricePaise: 15000,
	})
	if err != nil {
		t.Fatalf("CreateItem: %v", err)
	}
	repo.bumpCalls = 0

	updated, err := svc.SetItemAvailability(authorizedContext(), outletID, item.ID, false)
	if err != nil {
		t.Fatalf("SetItemAvailability: %v", err)
	}
	if repo.bumpCalls != 1 {
		t.Fatalf("expected exactly 1 config_version bump for availability write, got %d", repo.bumpCalls)
	}
	if updated.IsAvailable {
		t.Fatalf("expected item to be unavailable after snooze")
	}
	if updated.ConfigVersion != 3 { // 1 (category) + 1 (item) + 1 (this write)
		t.Fatalf("expected config_version 3, got %d", updated.ConfigVersion)
	}
}
