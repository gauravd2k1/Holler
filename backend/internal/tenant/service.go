package tenant

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// Service implements the tenant/brand business commands. It is deliberately
// small: Milestone 1 only needs enough to make an outlet creatable, not full
// organisation management (that is Milestone 8 central admin).
type Service struct {
	repo Repository
	now  func() time.Time
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo, now: time.Now}
}

// CreateOrganisation registers a new tenant. There is no authenticated
// tenant context yet at this point in the flow (the tenant doesn't exist),
// so unlike every other command in this codebase this one is not
// tenant-scoped by a caller principal — it is the operation that creates
// the scope.
func (s *Service) CreateOrganisation(ctx context.Context, name string) (Tenant, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return Tenant{}, fmt.Errorf("%w: organisation name is required", httpx.ErrInvalidInput)
	}

	now := s.now().UTC()
	t := Tenant{
		ID:        id.New(),
		Name:      name,
		CreatedAt: now,
		UpdatedAt: now,
	}
	if err := s.repo.InsertTenant(ctx, t); err != nil {
		return Tenant{}, err
	}
	return t, nil
}

// CreateBrand creates a brand under tenantID. tenantID must come from the
// authenticated principal, never from a request body or query parameter
// (docs/spec/security-rbac.md §Tenant isolation).
func (s *Service) CreateBrand(ctx context.Context, tenantID, name string) (Brand, error) {
	if tenantID == "" {
		return Brand{}, fmt.Errorf("%w: tenant id is required", httpx.ErrInvalidInput)
	}
	name = strings.TrimSpace(name)
	if name == "" {
		return Brand{}, fmt.Errorf("%w: brand name is required", httpx.ErrInvalidInput)
	}

	now := s.now().UTC()
	b := Brand{
		ID:        id.New(),
		TenantID:  tenantID,
		Name:      name,
		CreatedAt: now,
		UpdatedAt: now,
	}
	if err := s.repo.InsertBrand(ctx, b); err != nil {
		return Brand{}, err
	}
	return b, nil
}

// BrandForTenant returns the brand only if it belongs to tenantID — the
// building block outlet creation uses to prevent attaching an outlet to
// another tenant's brand.
func (s *Service) BrandForTenant(ctx context.Context, tenantID, brandID string) (Brand, error) {
	return s.repo.GetBrand(ctx, tenantID, brandID)
}
