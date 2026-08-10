package kitchen

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
)

// pgUniqueViolation is the PostgreSQL SQLSTATE for a unique_violation,
// mirroring backend/internal/tables's convention.
const pgUniqueViolation = "23505"

func isUniqueViolation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == pgUniqueViolation
}

// Repository is the persistence boundary for the kitchen context. Service
// depends on this interface, never on pgx directly (CLAUDE.md §Coding
// rules).
type Repository interface {
	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error

	// BumpOutletConfigVersion increments outlet.config_version by exactly
	// one, mirroring backend/internal/menu and backend/internal/tables.
	BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error)

	// OutletBelongsToTenant reports whether outletID belongs (via brand) to
	// tenantID. Every kitchen write is scoped through this — never trusting
	// a caller-supplied outlet_id without checking it against the caller's
	// own tenant (contract review rubric: "uniqueness constraints
	// tenant-scoped, not global" applies equally to authorization checks).
	OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error)

	ListStations(ctx context.Context, outletID string) ([]Station, error)
	InsertStation(ctx context.Context, tx pgx.Tx, s Station) error
	GetStation(ctx context.Context, stationID string) (Station, error)
	StationsBelongToOutlet(ctx context.Context, outletID string, stationIDs []string) (bool, error)

	ListPrinters(ctx context.Context, outletID string) ([]Printer, error)
	InsertPrinter(ctx context.Context, tx pgx.Tx, p Printer) error
	GetPrinter(ctx context.Context, printerID string) (Printer, error)
	PrintersBelongToOutlet(ctx context.Context, outletID string, printerIDs []string) (bool, error)

	// MenuItemOutlet returns the outlet_id a menu item belongs to.
	MenuItemOutlet(ctx context.Context, itemID string) (string, error)
	ReplaceItemStations(ctx context.Context, tx pgx.Tx, itemID string, stationIDs []string, configVersion int) ([]MenuItemStation, error)
	ReplaceStationPrinters(ctx context.Context, tx pgx.Tx, stationID string, printerIDs []string, configVersion int) ([]StationPrinter, error)

	// OrderOutlet returns the outlet_id an order belongs to, for validating
	// a replayed KOT's envelope outlet_id against the order it tickets.
	OrderOutlet(ctx context.Context, orderID string) (string, error)
	// InsertKot is idempotent on id: a duplicate replay of the same KOT id
	// (edge retry) is a no-op. Returns the row as stored (either
	// just-inserted or pre-existing) and whether this call inserted it.
	InsertKot(ctx context.Context, tx pgx.Tx, deviceID string, k Kot) (stored Kot, inserted bool, err error)
	GetKot(ctx context.Context, kotID string) (Kot, error)
	// UpdateKotStatus is the ONLY write path for kot.status (§50.1, ADR-014)
	// — it exists to serve exactly one caller, Service.IngestKotStatus.
	UpdateKotStatus(ctx context.Context, tx pgx.Tx, kotID string, status KotStatus, changedAt time.Time) (Kot, error)

	StationsSince(ctx context.Context, outletID string, sinceVersion int) ([]Station, error)
	ItemStationsSince(ctx context.Context, outletID string, sinceVersion int) ([]MenuItemStation, error)
	PrintersSince(ctx context.Context, outletID string, sinceVersion int) ([]Printer, error)
	StationPrintersSince(ctx context.Context, outletID string, sinceVersion int) ([]StationPrinter, error)
}

type pgRepository struct {
	pool postgres.Pool
}

// NewRepository returns a Repository backed by a live PostgreSQL pool.
func NewRepository(pool postgres.Pool) Repository {
	return &pgRepository{pool: pool}
}

func (r *pgRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("kitchen: begin tx: %w", err)
	}
	if err := fn(tx); err != nil {
		_ = tx.Rollback(ctx)
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("kitchen: commit tx: %w", err)
	}
	return nil
}

func (r *pgRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	var newVersion int
	err := tx.QueryRow(ctx,
		`UPDATE outlet SET config_version = config_version + 1, updated_at = now()
		 WHERE id = $1 RETURNING config_version`,
		outletID,
	).Scan(&newVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, fmt.Errorf("%w: outlet %s", httpx.ErrNotFound, outletID)
	}
	if err != nil {
		return 0, fmt.Errorf("kitchen: bumping outlet config_version: %w", err)
	}
	return newVersion, nil
}

func (r *pgRepository) OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error) {
	var exists bool
	err := r.pool.QueryRow(ctx,
		`SELECT EXISTS(
			SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id
			WHERE o.id = $1 AND b.tenant_id = $2
		 )`,
		outletID, tenantID,
	).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("kitchen: checking outlet tenancy: %w", err)
	}
	return exists, nil
}

// --- Station ---------------------------------------------------------------

func (r *pgRepository) ListStations(ctx context.Context, outletID string) ([]Station, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, code, name, sort_order, is_active, config_version
		 FROM station WHERE outlet_id = $1 ORDER BY sort_order, code`,
		outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("kitchen: listing stations: %w", err)
	}
	defer rows.Close()

	var out []Station
	for rows.Next() {
		var s Station
		if err := rows.Scan(&s.ID, &s.OutletID, &s.Code, &s.Name, &s.SortOrder, &s.IsActive, &s.ConfigVersion); err != nil {
			return nil, fmt.Errorf("kitchen: scanning station: %w", err)
		}
		s.SchemaVersion = 1
		out = append(out, s)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertStation(ctx context.Context, tx pgx.Tx, s Station) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO station (id, outlet_id, code, name, sort_order, is_active, config_version)
		 VALUES ($1, $2, $3, $4, $5, $6, $7)`,
		s.ID, s.OutletID, s.Code, s.Name, s.SortOrder, s.IsActive, s.ConfigVersion,
	)
	if err != nil {
		if isUniqueViolation(err) {
			return fmt.Errorf("%w: station code %q already exists in this outlet", httpx.ErrConflict, s.Code)
		}
		return fmt.Errorf("kitchen: inserting station: %w", err)
	}
	return nil
}

func (r *pgRepository) GetStation(ctx context.Context, stationID string) (Station, error) {
	var s Station
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, code, name, sort_order, is_active, config_version
		 FROM station WHERE id = $1`,
		stationID,
	).Scan(&s.ID, &s.OutletID, &s.Code, &s.Name, &s.SortOrder, &s.IsActive, &s.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return Station{}, fmt.Errorf("%w: station %s", httpx.ErrNotFound, stationID)
	}
	if err != nil {
		return Station{}, fmt.Errorf("kitchen: getting station: %w", err)
	}
	s.SchemaVersion = 1
	return s, nil
}

func (r *pgRepository) StationsBelongToOutlet(ctx context.Context, outletID string, stationIDs []string) (bool, error) {
	if len(stationIDs) == 0 {
		return true, nil
	}
	var count int
	err := r.pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM station WHERE outlet_id = $1 AND id = ANY($2)`,
		outletID, stationIDs,
	).Scan(&count)
	if err != nil {
		return false, fmt.Errorf("kitchen: checking station membership: %w", err)
	}
	return count == len(uniqueStrings(stationIDs)), nil
}

// --- Printer -----------------------------------------------------------

func (r *pgRepository) ListPrinters(ctx context.Context, outletID string) ([]Printer, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version
		 FROM printer WHERE outlet_id = $1 ORDER BY name`,
		outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("kitchen: listing printers: %w", err)
	}
	defer rows.Close()

	var out []Printer
	for rows.Next() {
		var p Printer
		var kind string
		if err := rows.Scan(&p.ID, &p.OutletID, &p.Name, &kind, &p.Address, &p.PaperWidthMM, &p.IsActive, &p.ConfigVersion); err != nil {
			return nil, fmt.Errorf("kitchen: scanning printer: %w", err)
		}
		p.ConnectionKind = PrinterConnectionKind(kind)
		p.SchemaVersion = 1
		out = append(out, p)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertPrinter(ctx context.Context, tx pgx.Tx, p Printer) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO printer (id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
		p.ID, p.OutletID, p.Name, string(p.ConnectionKind), p.Address, p.PaperWidthMM, p.IsActive, p.ConfigVersion,
	)
	if err != nil {
		if isUniqueViolation(err) {
			return fmt.Errorf("%w: printer name %q already exists in this outlet", httpx.ErrConflict, p.Name)
		}
		return fmt.Errorf("kitchen: inserting printer: %w", err)
	}
	return nil
}

func (r *pgRepository) GetPrinter(ctx context.Context, printerID string) (Printer, error) {
	var p Printer
	var kind string
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version
		 FROM printer WHERE id = $1`,
		printerID,
	).Scan(&p.ID, &p.OutletID, &p.Name, &kind, &p.Address, &p.PaperWidthMM, &p.IsActive, &p.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return Printer{}, fmt.Errorf("%w: printer %s", httpx.ErrNotFound, printerID)
	}
	if err != nil {
		return Printer{}, fmt.Errorf("kitchen: getting printer: %w", err)
	}
	p.ConnectionKind = PrinterConnectionKind(kind)
	p.SchemaVersion = 1
	return p, nil
}

func (r *pgRepository) PrintersBelongToOutlet(ctx context.Context, outletID string, printerIDs []string) (bool, error) {
	if len(printerIDs) == 0 {
		return true, nil
	}
	var count int
	err := r.pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM printer WHERE outlet_id = $1 AND id = ANY($2)`,
		outletID, printerIDs,
	).Scan(&count)
	if err != nil {
		return false, fmt.Errorf("kitchen: checking printer membership: %w", err)
	}
	return count == len(uniqueStrings(printerIDs)), nil
}

// --- Routing -----------------------------------------------------------

func (r *pgRepository) MenuItemOutlet(ctx context.Context, itemID string) (string, error) {
	var outletID string
	err := r.pool.QueryRow(ctx, `SELECT outlet_id FROM menu_item WHERE id = $1`, itemID).Scan(&outletID)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", fmt.Errorf("%w: menu item %s", httpx.ErrNotFound, itemID)
	}
	if err != nil {
		return "", fmt.Errorf("kitchen: getting menu item outlet: %w", err)
	}
	return outletID, nil
}

// ReplaceItemStations replaces itemID's routing wholesale: config is
// replaced, not merged (ADR-014 §2 — PUT, not POST).
func (r *pgRepository) ReplaceItemStations(ctx context.Context, tx pgx.Tx, itemID string, stationIDs []string, configVersion int) ([]MenuItemStation, error) {
	if _, err := tx.Exec(ctx, `DELETE FROM menu_item_station WHERE menu_item_id = $1`, itemID); err != nil {
		return nil, fmt.Errorf("kitchen: clearing item station routing: %w", err)
	}
	out := make([]MenuItemStation, 0, len(stationIDs))
	for _, sid := range uniqueStrings(stationIDs) {
		if _, err := tx.Exec(ctx,
			`INSERT INTO menu_item_station (menu_item_id, station_id, config_version) VALUES ($1, $2, $3)`,
			itemID, sid, configVersion,
		); err != nil {
			return nil, fmt.Errorf("kitchen: inserting item station routing: %w", err)
		}
		out = append(out, MenuItemStation{MenuItemID: itemID, StationID: sid, ConfigVersion: configVersion, SchemaVersion: 1})
	}
	return out, nil
}

// ReplaceStationPrinters replaces stationID's printer routing wholesale.
func (r *pgRepository) ReplaceStationPrinters(ctx context.Context, tx pgx.Tx, stationID string, printerIDs []string, configVersion int) ([]StationPrinter, error) {
	if _, err := tx.Exec(ctx, `DELETE FROM station_printer WHERE station_id = $1`, stationID); err != nil {
		return nil, fmt.Errorf("kitchen: clearing station printer routing: %w", err)
	}
	out := make([]StationPrinter, 0, len(printerIDs))
	for _, pid := range uniqueStrings(printerIDs) {
		if _, err := tx.Exec(ctx,
			`INSERT INTO station_printer (station_id, printer_id, config_version) VALUES ($1, $2, $3)`,
			stationID, pid, configVersion,
		); err != nil {
			return nil, fmt.Errorf("kitchen: inserting station printer routing: %w", err)
		}
		out = append(out, StationPrinter{StationID: stationID, PrinterID: pid, ConfigVersion: configVersion, SchemaVersion: 1})
	}
	return out, nil
}

// --- KOT -----------------------------------------------------------------

func (r *pgRepository) OrderOutlet(ctx context.Context, orderID string) (string, error) {
	var outletID string
	err := r.pool.QueryRow(ctx, `SELECT outlet_id FROM "order" WHERE id = $1`, orderID).Scan(&outletID)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", fmt.Errorf("%w: order %s", httpx.ErrNotFound, orderID)
	}
	if err != nil {
		return "", fmt.Errorf("kitchen: getting order outlet: %w", err)
	}
	return outletID, nil
}

func (r *pgRepository) InsertKot(ctx context.Context, tx pgx.Tx, deviceID string, k Kot) (Kot, bool, error) {
	itemsJSON, err := json.Marshal(k.Items)
	if err != nil {
		return Kot{}, false, fmt.Errorf("kitchen: marshalling kot items: %w", err)
	}

	tag, err := tx.Exec(ctx,
		`INSERT INTO kot (id, order_id, station, sequence, status, items_json, created_by_device_id, created_at, updated_at)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
		 ON CONFLICT (id) DO NOTHING`,
		k.ID, k.OrderID, k.Station, k.Sequence, string(k.Status), itemsJSON, deviceID, k.CreatedAt, k.UpdatedAt,
	)
	if err != nil {
		return Kot{}, false, fmt.Errorf("kitchen: inserting kot: %w", err)
	}

	stored, getErr := r.getKotTx(ctx, tx, k.ID)
	if getErr != nil {
		return Kot{}, false, getErr
	}
	return stored, tag.RowsAffected() > 0, nil
}

func (r *pgRepository) GetKot(ctx context.Context, kotID string) (Kot, error) {
	return scanKot(r.pool.QueryRow(ctx,
		`SELECT id, order_id, station, sequence, status, items_json, created_at, updated_at
		 FROM kot WHERE id = $1`, kotID))
}

func (r *pgRepository) getKotTx(ctx context.Context, tx pgx.Tx, kotID string) (Kot, error) {
	return scanKot(tx.QueryRow(ctx,
		`SELECT id, order_id, station, sequence, status, items_json, created_at, updated_at
		 FROM kot WHERE id = $1`, kotID))
}

type rowScanner interface {
	Scan(dest ...interface{}) error
}

func scanKot(row rowScanner) (Kot, error) {
	var k Kot
	var status string
	var itemsJSON []byte
	err := row.Scan(&k.ID, &k.OrderID, &k.Station, &k.Sequence, &status, &itemsJSON, &k.CreatedAt, &k.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return Kot{}, httpx.ErrNotFound
	}
	if err != nil {
		return Kot{}, fmt.Errorf("kitchen: scanning kot: %w", err)
	}
	k.Status = KotStatus(status)
	if len(itemsJSON) > 0 {
		if err := json.Unmarshal(itemsJSON, &k.Items); err != nil {
			return Kot{}, fmt.Errorf("kitchen: decoding kot items: %w", err)
		}
	}
	if k.Items == nil {
		k.Items = []KotTicketItem{}
	}
	// kot has no stored schema_version/created_by_device_id read path here:
	// created_by_device_id is write-only metadata not required by any read
	// caller yet; the Kot wire schema pins schema_version to the constant 1.
	k.SchemaVersion = 1
	return k, nil
}

// UpdateKotStatus is the single write path for kot.status (§50.1, ADR-014).
// It is called only from Service.IngestKotStatus, which is itself the only
// route permitted to invoke it.
func (r *pgRepository) UpdateKotStatus(ctx context.Context, tx pgx.Tx, kotID string, status KotStatus, changedAt time.Time) (Kot, error) {
	tag, err := tx.Exec(ctx,
		`UPDATE kot SET status = $1, updated_at = $2 WHERE id = $3`,
		string(status), changedAt, kotID,
	)
	if err != nil {
		return Kot{}, fmt.Errorf("kitchen: updating kot status: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return Kot{}, fmt.Errorf("%w: kot %s", httpx.ErrNotFound, kotID)
	}
	return r.getKotTx(ctx, tx, kotID)
}

// --- Sync config bundle ---------------------------------------------------

func (r *pgRepository) StationsSince(ctx context.Context, outletID string, sinceVersion int) ([]Station, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, code, name, sort_order, is_active, config_version
		 FROM station WHERE outlet_id = $1 AND config_version > $2 ORDER BY config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("kitchen: listing stations since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []Station
	for rows.Next() {
		var s Station
		if err := rows.Scan(&s.ID, &s.OutletID, &s.Code, &s.Name, &s.SortOrder, &s.IsActive, &s.ConfigVersion); err != nil {
			return nil, fmt.Errorf("kitchen: scanning station: %w", err)
		}
		s.SchemaVersion = 1
		out = append(out, s)
	}
	return out, rows.Err()
}

func (r *pgRepository) ItemStationsSince(ctx context.Context, outletID string, sinceVersion int) ([]MenuItemStation, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT mis.menu_item_id, mis.station_id, mis.config_version
		 FROM menu_item_station mis
		 JOIN station s ON s.id = mis.station_id
		 WHERE s.outlet_id = $1 AND mis.config_version > $2
		 ORDER BY mis.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("kitchen: listing item stations since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []MenuItemStation
	for rows.Next() {
		var m MenuItemStation
		if err := rows.Scan(&m.MenuItemID, &m.StationID, &m.ConfigVersion); err != nil {
			return nil, fmt.Errorf("kitchen: scanning item station: %w", err)
		}
		m.SchemaVersion = 1
		out = append(out, m)
	}
	return out, rows.Err()
}

func (r *pgRepository) PrintersSince(ctx context.Context, outletID string, sinceVersion int) ([]Printer, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, name, connection_kind, address, paper_width_mm, is_active, config_version
		 FROM printer WHERE outlet_id = $1 AND config_version > $2 ORDER BY config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("kitchen: listing printers since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []Printer
	for rows.Next() {
		var p Printer
		var kind string
		if err := rows.Scan(&p.ID, &p.OutletID, &p.Name, &kind, &p.Address, &p.PaperWidthMM, &p.IsActive, &p.ConfigVersion); err != nil {
			return nil, fmt.Errorf("kitchen: scanning printer: %w", err)
		}
		p.ConnectionKind = PrinterConnectionKind(kind)
		p.SchemaVersion = 1
		out = append(out, p)
	}
	return out, rows.Err()
}

func (r *pgRepository) StationPrintersSince(ctx context.Context, outletID string, sinceVersion int) ([]StationPrinter, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT sp.station_id, sp.printer_id, sp.config_version
		 FROM station_printer sp
		 JOIN station s ON s.id = sp.station_id
		 WHERE s.outlet_id = $1 AND sp.config_version > $2
		 ORDER BY sp.config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("kitchen: listing station printers since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []StationPrinter
	for rows.Next() {
		var sp StationPrinter
		if err := rows.Scan(&sp.StationID, &sp.PrinterID, &sp.ConfigVersion); err != nil {
			return nil, fmt.Errorf("kitchen: scanning station printer: %w", err)
		}
		sp.SchemaVersion = 1
		out = append(out, sp)
	}
	return out, rows.Err()
}

// uniqueStrings de-duplicates while preserving first-seen order, so a caller
// resubmitting the same id twice in a routing PUT doesn't produce two rows.
func uniqueStrings(in []string) []string {
	seen := make(map[string]struct{}, len(in))
	out := make([]string, 0, len(in))
	for _, s := range in {
		if _, ok := seen[s]; ok {
			continue
		}
		seen[s] = struct{}{}
		out = append(out, s)
	}
	return out
}
