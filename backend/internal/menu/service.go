package menu

import (
	"context"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// Service holds the menu context's business logic. HTTP handlers call this;
// it never touches sql/pgx directly (CLAUDE.md §Coding rules).
type Service struct {
	repo Repository
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo}
}

// NewCategoryInput is what a caller supplies to create a category; the
// service assigns id and config_version.
type NewCategoryInput struct {
	OutletID  string
	Name      string
	SortOrder int
}

func (s *Service) ListCategories(ctx context.Context, outletID string) ([]Category, error) {
	if strings.TrimSpace(outletID) == "" {
		return nil, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	return s.repo.ListCategories(ctx, outletID)
}

func (s *Service) CreateCategory(ctx context.Context, in NewCategoryInput) (Category, error) {
	if err := requirePermission(ctx, permMenuManage); err != nil {
		return Category{}, err
	}
	if strings.TrimSpace(in.OutletID) == "" {
		return Category{}, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return Category{}, fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if in.SortOrder < 0 {
		return Category{}, fmt.Errorf("%w: sort_order must not be negative", httpx.ErrInvalidInput)
	}

	c := Category{
		ID:        id.New(),
		OutletID:  in.OutletID,
		Name:      in.Name,
		SortOrder: in.SortOrder,
	}

	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		c.ConfigVersion = newVersion
		return s.repo.InsertCategory(ctx, tx, c)
	})
	if err != nil {
		return Category{}, err
	}
	return c, nil
}

// NewItemInput is what a caller supplies to create a menu item, optionally
// together with its variants and modifiers in the same logical write (so the
// config_version bumps exactly once for the whole item).
type NewItemInput struct {
	OutletID       string
	CategoryID     string
	Name           string
	BasePricePaise int64
	Variants       []NewVariantInput
	Modifiers      []NewModifierInput
}

type NewVariantInput struct {
	Name            string
	PriceDeltaPaise int64
	IsDefault       bool
}

type NewModifierInput struct {
	GroupName       string
	OptionName      string
	PriceDeltaPaise int64
	MinSelection    int
	MaxSelection    int
}

func (s *Service) ListItems(ctx context.Context, outletID string) ([]Item, error) {
	if strings.TrimSpace(outletID) == "" {
		return nil, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	return s.repo.ListItems(ctx, outletID)
}

// ListVariantsSince and ListModifiersSince are this context's
// since_version-filtered sync exports (M4 T4 delivery-fix follow-up):
// menu_item_variant and menu_item_modifier never reached GET /sync/config
// before this, so a cloud-synced outlet had every recipe pointing at variant
// rows it did not have — recipe.menu_item_variant_id is NOT NULL, so every
// order line failed to stamp a variant and every sale gapped NO_VARIANT.
func (s *Service) ListVariantsSince(ctx context.Context, outletID string, sinceVersion int) ([]Variant, error) {
	if strings.TrimSpace(outletID) == "" {
		return nil, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	return s.repo.ListVariantsSince(ctx, outletID, sinceVersion)
}

func (s *Service) ListModifiersSince(ctx context.Context, outletID string, sinceVersion int) ([]Modifier, error) {
	if strings.TrimSpace(outletID) == "" {
		return nil, fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	return s.repo.ListModifiersSince(ctx, outletID, sinceVersion)
}

func (s *Service) CreateItem(ctx context.Context, in NewItemInput) (Item, []Variant, []Modifier, error) {
	if err := requirePermission(ctx, permMenuManage); err != nil {
		return Item{}, nil, nil, err
	}
	if err := validateNewItemInput(in); err != nil {
		return Item{}, nil, nil, err
	}

	exists, err := s.repo.CategoryExists(ctx, in.OutletID, in.CategoryID)
	if err != nil {
		return Item{}, nil, nil, err
	}
	if !exists {
		return Item{}, nil, nil, fmt.Errorf("%w: category %s does not belong to outlet %s", httpx.ErrInvalidInput, in.CategoryID, in.OutletID)
	}

	item := Item{
		ID:             id.New(),
		OutletID:       in.OutletID,
		CategoryID:     in.CategoryID,
		Name:           in.Name,
		BasePricePaise: in.BasePricePaise,
		IsAvailable:    true,
	}

	variants := make([]Variant, len(in.Variants))
	for idx, v := range in.Variants {
		variants[idx] = Variant{
			ID:              id.New(),
			MenuItemID:      item.ID,
			Name:            v.Name,
			PriceDeltaPaise: v.PriceDeltaPaise,
			IsDefault:       v.IsDefault,
		}
	}

	modifiers := make([]Modifier, len(in.Modifiers))
	for idx, m := range in.Modifiers {
		modifiers[idx] = Modifier{
			ID:              id.New(),
			MenuItemID:      item.ID,
			GroupName:       m.GroupName,
			OptionName:      m.OptionName,
			PriceDeltaPaise: m.PriceDeltaPaise,
			MinSelection:    m.MinSelection,
			MaxSelection:    m.MaxSelection,
		}
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		item.ConfigVersion = newVersion
		if err := s.repo.InsertItem(ctx, tx, item); err != nil {
			return err
		}
		for i := range variants {
			variants[i].ConfigVersion = newVersion
			if err := s.repo.InsertVariant(ctx, tx, variants[i]); err != nil {
				return err
			}
		}
		for i := range modifiers {
			modifiers[i].ConfigVersion = newVersion
			if err := s.repo.InsertModifier(ctx, tx, modifiers[i]); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		return Item{}, nil, nil, err
	}
	return item, variants, modifiers, nil
}

// SetItemAvailability toggles the manual availability ("snooze") flag. This
// is a catalog write like any other and follows the same config_version
// discipline: bumped once, stamped onto the item row.
func (s *Service) SetItemAvailability(ctx context.Context, outletID, itemID string, isAvailable bool) (Item, error) {
	if err := requirePermission(ctx, permMenuManage); err != nil {
		return Item{}, err
	}
	if _, err := s.repo.GetItem(ctx, outletID, itemID); err != nil {
		return Item{}, err
	}

	var updated Item
	err := s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, outletID)
		if err != nil {
			return err
		}
		if err := s.repo.UpdateItemAvailability(ctx, tx, itemID, isAvailable, newVersion); err != nil {
			return err
		}
		updated = Item{ID: itemID, OutletID: outletID, IsAvailable: isAvailable, ConfigVersion: newVersion}
		return nil
	})
	if err != nil {
		return Item{}, err
	}
	return updated, nil
}

func validateNewItemInput(in NewItemInput) error {
	if strings.TrimSpace(in.OutletID) == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.CategoryID) == "" {
		return fmt.Errorf("%w: category_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if in.BasePricePaise < 0 {
		return fmt.Errorf("%w: base_price_paise must not be negative", httpx.ErrInvalidInput)
	}
	for _, v := range in.Variants {
		if strings.TrimSpace(v.Name) == "" {
			return fmt.Errorf("%w: variant name is required", httpx.ErrInvalidInput)
		}
	}
	groups := map[string]struct{ min, max int }{}
	for _, m := range in.Modifiers {
		if strings.TrimSpace(m.GroupName) == "" || strings.TrimSpace(m.OptionName) == "" {
			return fmt.Errorf("%w: modifier group_name and option_name are required", httpx.ErrInvalidInput)
		}
		if m.MinSelection < 0 {
			return fmt.Errorf("%w: modifier min_selection must not be negative", httpx.ErrInvalidInput)
		}
		if m.MaxSelection > 0 && m.MinSelection > m.MaxSelection {
			return fmt.Errorf("%w: modifier group %q has min_selection greater than max_selection", httpx.ErrInvalidInput, m.GroupName)
		}
		if existing, ok := groups[m.GroupName]; ok {
			if existing.min != m.MinSelection || existing.max != m.MaxSelection {
				return fmt.Errorf("%w: modifier group %q has inconsistent min/max across its options", httpx.ErrInvalidInput, m.GroupName)
			}
		} else {
			groups[m.GroupName] = struct{ min, max int }{m.MinSelection, m.MaxSelection}
		}
	}
	return nil
}

func requirePermission(ctx context.Context, permission string) error {
	p, ok := PrincipalFromContext(ctx)
	if !ok {
		return httpx.ErrUnauthorized
	}
	if !p.HasPermission(permission) {
		return httpx.ErrForbidden
	}
	return nil
}
