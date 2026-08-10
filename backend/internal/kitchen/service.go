package kitchen

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	contracts "github.com/holler/contracts"
)

// KOT ingest's permission (auth.PermissionOrderModify) is enforced directly
// by the auth.RequirePermission middleware in http.go's Mount, alongside
// send-to-kitchen's — there is no Service-level check to name here.
const (
	permStationManage = auth.PermissionMenuManage   // stations route: docs openapi "permission menu.manage"
	permPrinterManage = auth.PermissionOutletManage // printers route: docs openapi "permission outlet.manage"
)

// Service holds the kitchen context's business logic. HTTP handlers call
// this; it never touches pgx directly (CLAUDE.md §Coding rules).
type Service struct {
	repo  Repository
	audit auth.AuditRecorder
	now   func() time.Time
}

func NewService(repo Repository, audit auth.AuditRecorder) *Service {
	return &Service{repo: repo, audit: audit, now: time.Now}
}

func requirePermission(ctx context.Context, permission auth.Permission) (auth.AuthenticatedPrincipal, error) {
	principal, ok := auth.PrincipalFromContext(ctx)
	if !ok {
		return auth.AuthenticatedPrincipal{}, httpx.ErrUnauthorized
	}
	found := false
	for _, p := range principal.Permissions {
		if p == permission {
			found = true
			break
		}
	}
	if !found {
		return auth.AuthenticatedPrincipal{}, httpx.ErrForbidden
	}
	return principal, nil
}

// --- Station: CLOUD_TO_EDGE config -----------------------------------------

func (s *Service) ListStations(ctx context.Context, tenantID, outletID string) ([]Station, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return nil, err
	}
	return s.repo.ListStations(ctx, outletID)
}

// CreateStation defines a new production station. The id is caller-supplied
// (§74) — the cloud stores it, it never mints one, matching
// packages/contracts/openapi/openapi.yaml POST /stations.
func (s *Service) CreateStation(ctx context.Context, tenantID string, in NewStationInput) (Station, error) {
	principal, err := requirePermission(ctx, permStationManage)
	if err != nil {
		return Station{}, err
	}
	if err := validateNewStation(in); err != nil {
		return Station{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return Station{}, err
	}

	st := Station{
		ID:        in.ID,
		OutletID:  in.OutletID,
		Code:      in.Code,
		Name:      in.Name,
		SortOrder: in.SortOrder,
		IsActive:  in.IsActive,
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		st.ConfigVersion = newVersion
		st.SchemaVersion = 1
		return s.repo.InsertStation(ctx, tx, st)
	})
	if err != nil {
		return Station{}, err
	}

	s.auditAction(ctx, tenantID, &in.OutletID, principal, "station.create", "station", st.ID, nil, map[string]interface{}{
		"id": st.ID, "outlet_id": st.OutletID, "code": st.Code, "name": st.Name,
	})
	return st, nil
}

func validateNewStation(in NewStationInput) error {
	if _, err := id.Parse(in.ID); err != nil {
		return fmt.Errorf("%w: id must be a valid UUID", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.OutletID) == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Code) == "" {
		return fmt.Errorf("%w: code is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	if in.SortOrder < 0 {
		return fmt.Errorf("%w: sort_order must not be negative", httpx.ErrInvalidInput)
	}
	return nil
}

// --- Printer: CLOUD_TO_EDGE config ------------------------------------------

func (s *Service) ListPrinters(ctx context.Context, tenantID, outletID string) ([]Printer, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return nil, err
	}
	return s.repo.ListPrinters(ctx, outletID)
}

func (s *Service) CreatePrinter(ctx context.Context, tenantID string, in NewPrinterInput) (Printer, error) {
	principal, err := requirePermission(ctx, permPrinterManage)
	if err != nil {
		return Printer{}, err
	}
	if err := validateNewPrinter(in); err != nil {
		return Printer{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, in.OutletID); err != nil {
		return Printer{}, err
	}

	p := Printer{
		ID:             in.ID,
		OutletID:       in.OutletID,
		Name:           in.Name,
		ConnectionKind: in.ConnectionKind,
		Address:        in.Address,
		PaperWidthMM:   in.PaperWidthMM,
		IsActive:       in.IsActive,
	}

	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, in.OutletID)
		if err != nil {
			return err
		}
		p.ConfigVersion = newVersion
		p.SchemaVersion = 1
		return s.repo.InsertPrinter(ctx, tx, p)
	})
	if err != nil {
		return Printer{}, err
	}

	s.auditAction(ctx, tenantID, &in.OutletID, principal, "printer.create", "printer", p.ID, nil, map[string]interface{}{
		"id": p.ID, "outlet_id": p.OutletID, "name": p.Name, "connection_kind": string(p.ConnectionKind),
	})
	return p, nil
}

func validateNewPrinter(in NewPrinterInput) error {
	if _, err := id.Parse(in.ID); err != nil {
		return fmt.Errorf("%w: id must be a valid UUID", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.OutletID) == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(in.Name) == "" {
		return fmt.Errorf("%w: name is required", httpx.ErrInvalidInput)
	}
	switch in.ConnectionKind {
	case PrinterConnectionNetwork, PrinterConnectionUSB, PrinterConnectionBluetooth:
	default:
		return fmt.Errorf("%w: connection_kind %q is not valid", httpx.ErrInvalidInput, in.ConnectionKind)
	}
	if strings.TrimSpace(in.Address) == "" {
		return fmt.Errorf("%w: address is required", httpx.ErrInvalidInput)
	}
	if in.PaperWidthMM != 58 && in.PaperWidthMM != 80 {
		return fmt.Errorf("%w: paper_width_mm must be 58 or 80", httpx.ErrInvalidInput)
	}
	return nil
}

// --- Routing: PUT/replace, never append (ADR-014 §2) ------------------------

// ReplaceItemStations replaces itemID's station routing wholesale. Called
// from the item→station route mounted in backend/internal/menu, which owns
// only the route registration — the routing logic and authorization live
// here, next to the station/printer domain they route into.
func (s *Service) ReplaceItemStations(ctx context.Context, tenantID, itemID string, stationIDs []string) ([]MenuItemStation, error) {
	principal, err := requirePermission(ctx, permStationManage)
	if err != nil {
		return nil, err
	}
	itemID = strings.TrimSpace(itemID)
	if itemID == "" {
		return nil, fmt.Errorf("%w: item id is required", httpx.ErrInvalidInput)
	}
	for _, sid := range stationIDs {
		if _, err := id.Parse(sid); err != nil {
			return nil, fmt.Errorf("%w: station_ids must be valid UUIDs", httpx.ErrInvalidInput)
		}
	}

	outletID, err := s.repo.MenuItemOutlet(ctx, itemID)
	if err != nil {
		return nil, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return nil, err
	}

	belong, err := s.repo.StationsBelongToOutlet(ctx, outletID, stationIDs)
	if err != nil {
		return nil, err
	}
	if !belong {
		return nil, fmt.Errorf("%w: one or more station_ids do not belong to this item's outlet", httpx.ErrInvalidInput)
	}

	var out []MenuItemStation
	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, outletID)
		if err != nil {
			return err
		}
		out, err = s.repo.ReplaceItemStations(ctx, tx, itemID, stationIDs, newVersion)
		return err
	})
	if err != nil {
		return nil, err
	}

	s.auditAction(ctx, tenantID, &outletID, principal, "menu_item.stations.replace", "menu_item", itemID, nil, map[string]interface{}{
		"menu_item_id": itemID, "station_ids": stationIDs,
	})
	return out, nil
}

// ReplaceStationPrinters replaces stationID's printer routing wholesale.
func (s *Service) ReplaceStationPrinters(ctx context.Context, tenantID, stationID string, printerIDs []string) ([]StationPrinter, error) {
	principal, err := requirePermission(ctx, permPrinterManage)
	if err != nil {
		return nil, err
	}
	stationID = strings.TrimSpace(stationID)
	if stationID == "" {
		return nil, fmt.Errorf("%w: station id is required", httpx.ErrInvalidInput)
	}
	for _, pid := range printerIDs {
		if _, err := id.Parse(pid); err != nil {
			return nil, fmt.Errorf("%w: printer_ids must be valid UUIDs", httpx.ErrInvalidInput)
		}
	}

	station, err := s.repo.GetStation(ctx, stationID)
	if err != nil {
		return nil, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, station.OutletID); err != nil {
		return nil, err
	}

	belong, err := s.repo.PrintersBelongToOutlet(ctx, station.OutletID, printerIDs)
	if err != nil {
		return nil, err
	}
	if !belong {
		return nil, fmt.Errorf("%w: one or more printer_ids do not belong to this station's outlet", httpx.ErrInvalidInput)
	}

	var out []StationPrinter
	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, err := s.repo.BumpOutletConfigVersion(ctx, tx, station.OutletID)
		if err != nil {
			return err
		}
		out, err = s.repo.ReplaceStationPrinters(ctx, tx, stationID, printerIDs, newVersion)
		return err
	})
	if err != nil {
		return nil, err
	}

	s.auditAction(ctx, tenantID, &station.OutletID, principal, "station.printers.replace", "station", stationID, nil, map[string]interface{}{
		"station_id": stationID, "printer_ids": printerIDs,
	})
	return out, nil
}

// --- KOT: EDGE_TO_CLOUD, envelope-ingest, replay-only ------------------------

// IngestKot replays a KOT generated at the edge (§50.1). The cloud stores
// what it replays and never generates, renumbers or re-routes a ticket.
// Idempotent on the envelope's record_id (edge retry produces exactly one
// row).
func (s *Service) IngestKot(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, kot Kot) (Kot, error) {
	if err := requireKotEnvelope(env); err != nil {
		return Kot{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return Kot{}, err
	}
	if strings.TrimSpace(kot.ID) == "" {
		return Kot{}, fmt.Errorf("%w: kot id is required", httpx.ErrInvalidInput)
	}
	if kot.ID != env.RecordID {
		return Kot{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(kot.OrderID) == "" {
		return Kot{}, fmt.Errorf("%w: order_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(kot.Station) == "" {
		return Kot{}, fmt.Errorf("%w: station is required", httpx.ErrInvalidInput)
	}
	if kot.Status == "" {
		kot.Status = KotStatusNew
	}
	if kot.Status != KotStatusNew {
		return Kot{}, fmt.Errorf("%w: a newly-ingested kot must carry status NEW", httpx.ErrInvalidInput)
	}
	if kot.Items == nil {
		kot.Items = []KotTicketItem{}
	}
	if kot.CreatedAt.IsZero() {
		return Kot{}, fmt.Errorf("%w: created_at is required", httpx.ErrInvalidInput)
	}
	if kot.UpdatedAt.IsZero() {
		kot.UpdatedAt = kot.CreatedAt
	}

	orderOutletID, err := s.repo.OrderOutlet(ctx, kot.OrderID)
	if err != nil {
		return Kot{}, err
	}
	if orderOutletID != env.OutletID {
		return Kot{}, fmt.Errorf("%w: envelope outlet_id does not match the order's outlet", httpx.ErrInvalidInput)
	}
	if err := s.requireOutletInTenant(ctx, callerTenantID, orderOutletID); err != nil {
		return Kot{}, err
	}

	var stored Kot
	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		var insertErr error
		stored, _, insertErr = s.repo.InsertKot(ctx, tx, env.DeviceID, kot)
		return insertErr
	})
	if err != nil {
		return Kot{}, err
	}

	s.auditAction(ctx, callerTenantID, &orderOutletID, auth.AuthenticatedPrincipal{}, "kot.create", "kot", stored.ID, nil, map[string]interface{}{
		"id": stored.ID, "order_id": stored.OrderID, "station": stored.Station,
	})
	return stored, nil
}

// IngestKotStatus is the ONLY method that writes kot.status, and it only
// ever replays a transition the edge already decided (§50.1, ADR-014). No
// other Service method — and no other HTTP route — may call
// Repository.UpdateKotStatus.
func (s *Service) IngestKotStatus(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, kotID string, transition KotStatusTransition) (Kot, error) {
	if err := requireKotEnvelope(env); err != nil {
		return Kot{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return Kot{}, err
	}
	kotID = strings.TrimSpace(kotID)
	if kotID == "" {
		return Kot{}, fmt.Errorf("%w: kot id is required", httpx.ErrInvalidInput)
	}
	if env.RecordID != "" && env.RecordID != kotID {
		return Kot{}, fmt.Errorf("%w: envelope record_id must match the route's kot id", httpx.ErrInvalidInput)
	}
	if transition.Status == "" {
		return Kot{}, fmt.Errorf("%w: status is required", httpx.ErrInvalidInput)
	}
	if transition.ChangedAt.IsZero() {
		return Kot{}, fmt.Errorf("%w: changed_at is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(transition.ChangedByDeviceID) == "" {
		return Kot{}, fmt.Errorf("%w: changed_by_device_id is required", httpx.ErrInvalidInput)
	}

	current, err := s.repo.GetKot(ctx, kotID)
	if err != nil {
		return Kot{}, err
	}

	orderOutletID, err := s.repo.OrderOutlet(ctx, current.OrderID)
	if err != nil {
		return Kot{}, err
	}
	if err := s.requireOutletInTenant(ctx, callerTenantID, orderOutletID); err != nil {
		return Kot{}, err
	}

	// Idempotent replay: the edge resent an event this ticket already
	// carries. Return the current row rather than re-applying.
	if current.Status == transition.Status {
		return current, nil
	}
	if !validKotTransition(current.Status, transition.Status) {
		return Kot{}, fmt.Errorf("%w: cannot move kot from %q to %q", ErrIllegalTransition, current.Status, transition.Status)
	}

	var stored Kot
	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		var updErr error
		stored, updErr = s.repo.UpdateKotStatus(ctx, tx, kotID, transition.Status, transition.ChangedAt.UTC())
		return updErr
	})
	if err != nil {
		return Kot{}, err
	}

	s.auditAction(ctx, callerTenantID, &orderOutletID, auth.AuthenticatedPrincipal{}, "kot.status_changed", "kot", kotID,
		map[string]interface{}{"status": string(current.Status)},
		map[string]interface{}{"status": string(stored.Status), "changed_by_device_id": transition.ChangedByDeviceID},
	)
	return stored, nil
}

// GetKot returns a single ticket, scoped to the caller's tenant.
func (s *Service) GetKot(ctx context.Context, tenantID, kotID string) (Kot, error) {
	kot, err := s.repo.GetKot(ctx, kotID)
	if err != nil {
		return Kot{}, err
	}
	outletID, err := s.repo.OrderOutlet(ctx, kot.OrderID)
	if err != nil {
		return Kot{}, err
	}
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return Kot{}, err
	}
	return kot, nil
}

// --- Sync config bundle ------------------------------------------------

// SyncConfigBundle returns the kitchen context's contribution to
// GET /sync/config: every station/printer/routing row newer than
// sinceVersion for outletID. See ConfigBundle's doc comment for why this
// type, not an HTTP handler, is kitchen's boundary here.
func (s *Service) SyncConfigBundle(ctx context.Context, tenantID, outletID string, sinceVersion int) (ConfigBundle, error) {
	if err := s.requireOutletInTenant(ctx, tenantID, outletID); err != nil {
		return ConfigBundle{}, err
	}

	stations, err := s.repo.StationsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	itemStations, err := s.repo.ItemStationsSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	printers, err := s.repo.PrintersSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	stationPrinters, err := s.repo.StationPrintersSince(ctx, outletID, sinceVersion)
	if err != nil {
		return ConfigBundle{}, err
	}
	return ConfigBundle{
		Stations:        emptyIfNilStations(stations),
		ItemStations:    emptyIfNilItemStations(itemStations),
		Printers:        emptyIfNilPrinters(printers),
		StationPrinters: emptyIfNilStationPrinters(stationPrinters),
	}, nil
}

func emptyIfNilStations(s []Station) []Station {
	if s == nil {
		return []Station{}
	}
	return s
}

func emptyIfNilItemStations(s []MenuItemStation) []MenuItemStation {
	if s == nil {
		return []MenuItemStation{}
	}
	return s
}

func emptyIfNilPrinters(s []Printer) []Printer {
	if s == nil {
		return []Printer{}
	}
	return s
}

func emptyIfNilStationPrinters(s []StationPrinter) []StationPrinter {
	if s == nil {
		return []StationPrinter{}
	}
	return s
}

// --- shared helpers ----------------------------------------------------

func (s *Service) requireOutletInTenant(ctx context.Context, tenantID, outletID string) error {
	if strings.TrimSpace(tenantID) == "" {
		return httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	if outletID == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	ok, err := s.repo.OutletBelongsToTenant(ctx, tenantID, outletID)
	if err != nil {
		return err
	}
	if !ok {
		return httpx.ErrForbidden
	}
	return nil
}

// requireTenantMatch guards against a caller replaying an envelope for a
// tenant other than the one their token authenticates as.
func requireTenantMatch(callerTenantID string, env contracts.SyncEnvelope) error {
	if callerTenantID == "" {
		return httpx.ErrUnauthorized
	}
	if env.TenantID != callerTenantID {
		return httpx.ErrForbidden
	}
	return nil
}

// auditAction records a sensitive action via the shared audit helper, which
// redacts credential material (CLAUDE.md, ADR-011). A zero-value principal
// (no ActorUserID available, e.g. an edge-replayed KOT event) is recorded
// with a nil actor rather than a synthetic one.
func (s *Service) auditAction(ctx context.Context, tenantID string, outletID *string, principal auth.AuthenticatedPrincipal, action, entityType, entityID string, oldValue, newValue map[string]interface{}) {
	if s.audit == nil {
		return
	}
	var actorUserID *string
	if principal.UserID != "" {
		uid := principal.UserID
		actorUserID = &uid
	}
	_ = s.audit.Audit(ctx, auth.AuditInput{
		TenantID:    tenantID,
		OutletID:    outletID,
		ActorUserID: actorUserID,
		Action:      action,
		EntityType:  entityType,
		EntityID:    &entityID,
		OldValue:    oldValue,
		NewValue:    newValue,
	})
}
