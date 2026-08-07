package outlet

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// Service implements the outlet business commands. Every method takes the
// caller's Principal and derives tenantID from it — callers never pass a
// bare tenant id, so there is no call site that can accidentally source
// tenant scoping from request input.
type Service struct {
	repo Repository
	now  func() time.Time
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo, now: time.Now}
}

// CreateOutlet opens a new outlet under brandID. brandID must belong to the
// caller's tenant; if it does not (or does not exist), the caller sees
// httpx.ErrNotFound, exactly as it would for a brand that never existed.
func (s *Service) CreateOutlet(ctx context.Context, principal Principal, brandID, name, timezone string) (Outlet, error) {
	if principal.TenantID == "" {
		return Outlet{}, httpx.ErrUnauthorized
	}
	brandID = strings.TrimSpace(brandID)
	if brandID == "" {
		return Outlet{}, fmt.Errorf("%w: brand id is required", httpx.ErrInvalidInput)
	}
	name = strings.TrimSpace(name)
	if name == "" {
		return Outlet{}, fmt.Errorf("%w: outlet name is required", httpx.ErrInvalidInput)
	}
	timezone = strings.TrimSpace(timezone)
	if timezone == "" {
		timezone = defaultTimezone
	}

	now := s.now().UTC()
	o := Outlet{
		ID:            id.New(),
		BrandID:       brandID,
		Name:          name,
		Timezone:      timezone,
		ConfigVersion: 0,
		CreatedAt:     now,
		UpdatedAt:     now,
	}
	if err := s.repo.Insert(ctx, principal.TenantID, o); err != nil {
		return Outlet{}, err
	}
	return o, nil
}

// ListOutlets returns every outlet the caller's tenant owns. Milestone 1
// does not yet narrow this further by per-outlet role assignment (that is
// auth's user_role table); it does guarantee the list can never contain
// another tenant's outlet.
func (s *Service) ListOutlets(ctx context.Context, principal Principal) ([]Outlet, error) {
	if principal.TenantID == "" {
		return nil, httpx.ErrUnauthorized
	}
	outlets, err := s.repo.ListByTenant(ctx, principal.TenantID)
	if err != nil {
		return nil, err
	}
	if outlets == nil {
		outlets = []Outlet{}
	}
	return outlets, nil
}

// GetOutlet returns a single outlet, scoped to the caller's tenant. A
// request carrying tenant A's principal and tenant B's outlet id gets
// httpx.ErrNotFound — never a 200 with another tenant's data, never a 403
// that confirms the id exists.
func (s *Service) GetOutlet(ctx context.Context, principal Principal, outletID string) (Outlet, error) {
	if principal.TenantID == "" {
		return Outlet{}, httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	if outletID == "" {
		return Outlet{}, fmt.Errorf("%w: outlet id is required", httpx.ErrInvalidInput)
	}
	return s.repo.GetByID(ctx, principal.TenantID, outletID)
}
