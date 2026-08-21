package outlet

import (
	"context"
	"fmt"
	"regexp"
	"strings"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// dayStartTimePattern matches strict 24-hour HH:MM, the shape
// packages/contracts/postgres/0013_outlet_day_start.sql stores and the edge
// parses. The edge treats invalid config as a hard rejection since 0.5.3
// (task instruction); the cloud write path holds the same line rather than
// accepting a value the edge would then refuse.
var dayStartTimePattern = regexp.MustCompile(`^([01][0-9]|2[0-3]):[0-5][0-9]$`)

func validateDayStartTime(v string) error {
	if !dayStartTimePattern.MatchString(v) {
		return fmt.Errorf("%w: day_start_time must be HH:MM (24-hour), got %q", httpx.ErrInvalidInput, v)
	}
	return nil
}

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
		DayStartTime:  defaultDayStartTime,
		ConfigVersion: 0,
		CreatedAt:     now,
		UpdatedAt:     now,
	}
	if err := s.repo.Insert(ctx, principal.TenantID, o); err != nil {
		return Outlet{}, err
	}
	return o, nil
}

// SetDayStartTime is the outlet write path for ADR-018 §9.2's
// outlet.day_start_time: CONFIG, cloud->edge, bumping config_version like
// every other cloud-owned config write in this codebase (backend/internal/
// kitchen and backend/internal/compliance's BumpOutletConfigVersion
// precedent) so a subsequent GET /sync/config carries the new value.
// Anything that is not strict HH:MM is a hard rejection, never coerced —
// the edge has treated invalid config as a hard rejection since 0.5.3.
func (s *Service) SetDayStartTime(ctx context.Context, principal Principal, outletID, dayStartTime string) (Outlet, error) {
	if principal.TenantID == "" {
		return Outlet{}, httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	if outletID == "" {
		return Outlet{}, fmt.Errorf("%w: outlet id is required", httpx.ErrInvalidInput)
	}
	dayStartTime = strings.TrimSpace(dayStartTime)
	if err := validateDayStartTime(dayStartTime); err != nil {
		return Outlet{}, err
	}
	return s.repo.UpdateDayStartTime(ctx, principal.TenantID, outletID, dayStartTime)
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
