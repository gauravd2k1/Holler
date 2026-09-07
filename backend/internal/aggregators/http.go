package aggregators

import (
	"encoding/base64"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"context"
	"io"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/storage"
	contracts "github.com/holler/contracts"
)

// Handler serves the M6 Phase C surfaces.
type Handler struct {
	svc  *Service
	repo Repository
}

func NewHandler(svc *Service, repo Repository) *Handler { return &Handler{svc: svc, repo: repo} }

// MountDeviceRoutes registers the routes an ENROLLED EDGE NODE calls. The
// caller is always a device, never a browser -- mirroring inventory.MountIngest
// and procurement.MountIngest (ADR-017 0.4.3).
//
// This is the C1 down-path: cloud -> edge for aggregator_order, the first
// non-config aggregate to travel that way. It is a PULL, not a push, for the
// reason every other edge->cloud stream is a push: the till has no address the
// cloud can reach, which is the premise of ADR-022 in the first place.
func (h *Handler) MountDeviceRoutes(r chi.Router) {
	r.Get("/sync/aggregator-orders", h.pullAggregatorOrders)
}

// MountLocalCallback registers the inbound callback path.
//
// REACHABLE LOCALLY ONLY IN M6. No signature verification, no registry, no
// public exposure -- M6.1 owns all three, and this route must not be mounted on
// a publicly-routable listener until it does. An unauthenticated endpoint that
// writes documents is exactly what that gate exists for.
func (h *Handler) MountLocalCallback(r chi.Router) {
	r.Post("/aggregators/{platform}/callback", h.receiveCallback)
}

func (h *Handler) receiveCallback(w http.ResponseWriter, r *http.Request) {
	principal, ok := outlet.DevicePrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	// Read whole and bounded. The raw bytes go to the adapter, which owns the
	// translation -- the core never parses a platform payload. 1 MiB is
	// generous for an order document and finite, which an unbounded read is
	// not.
	body, err := io.ReadAll(io.LimitReader(r.Body, 1<<20))
	if err != nil {
		httpx.Error(w, fmt.Errorf("%w: could not read the callback body", httpx.ErrInvalidInput))
		return
	}
	receipt, err := h.svc.ReceiveCallback(
		r.Context(), principal.TenantID, principal.OutletID, chi.URLParam(r, "platform"), body)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, receipt)
}

// pullAggregatorOrders serves documents newer than the caller's cursor.
//
// KEYSET ON (updated_at, id), NOT OFFSET. Documents are updated while an outlet
// is paging -- a platform status change rewrites one -- and an OFFSET walk
// silently skips or repeats rows when the set shifts underneath it, with the
// reader unable to tell. The id breaks ties so the order is total: two
// documents can share an updated_at to the microsecond, and a non-total order
// makes a cursor ambiguous exactly when a shop is busiest.
//
// UPDATED_AT, NOT RECEIVED_AT, and the difference is the whole point of a
// down-path: a document whose platform status changed must travel again. If
// this ordered by arrival, a cancellation would never reach the till that is
// cooking the order.
func (h *Handler) pullAggregatorOrders(w http.ResponseWriter, r *http.Request) {
	principal, ok := outlet.DevicePrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}

	limit := 100
	if raw := r.URL.Query().Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 || n > 500 {
			httpx.Error(w, fmt.Errorf("%w: limit must be between 1 and 500", httpx.ErrInvalidInput))
			return
		}
		limit = n
	}

	docs, next, err := h.pullPage(r, principal.TenantID, principal.OutletID, r.URL.Query().Get("cursor"), limit)
	if err != nil {
		httpx.Error(w, err)
		return
	}

	httpx.JSON(w, http.StatusOK, aggregatorOrderPage{Orders: docs, NextCursor: next})
}

type aggregatorOrderPage struct {
	Orders []contracts.AggregatorOrder `json:"orders"`
	// Absent when this page is the last one. The edge advances its cursor only
	// on what it actually applied, so a nil here means "you are caught up"
	// rather than "ask again".
	NextCursor *string `json:"next_cursor"`
}

func (h *Handler) pullPage(r *http.Request, tenantID, outletID, cursor string, limit int) ([]contracts.AggregatorOrder, *string, error) {
	ctx := r.Context()
	args := []any{tenantID, outletID, limit + 1}
	where := ` WHERE tenant_id = $1 AND outlet_id = $2`
	if cursor != "" {
		at, id, err := decodePullCursor(cursor)
		if err != nil {
			return nil, nil, fmt.Errorf("%w: cursor is not valid", httpx.ErrInvalidInput)
		}
		args = append(args, at, id)
		where += ` AND (updated_at, id) > ($4, $5)`
	}

	rows, err := h.repo.QueryDocuments(ctx, where+` ORDER BY updated_at ASC, id ASC LIMIT $3`, args)
	if err != nil {
		return nil, nil, err
	}

	var next *string
	if len(rows) > limit {
		last := rows[limit-1]
		c := encodePullCursor(last.UpdatedAt, last.Order.ID)
		next = &c
		rows = rows[:limit]
	}

	out := make([]contracts.AggregatorOrder, 0, len(rows))
	for _, row := range rows {
		out = append(out, row.Order)
	}
	return out, next, nil
}

// The cursor is opaque by construction: base64 of the sort key, so a client
// cannot build one by hand and depend on its shape, and a change to the
// ordering is not a silent behaviour change for anyone holding an old one --
// decode fails and the caller starts from the beginning, which is safe because
// the edge applies by id and is idempotent.
func encodePullCursor(updatedAt time.Time, id string) string {
	return base64.RawURLEncoding.EncodeToString([]byte(updatedAt.UTC().Format(time.RFC3339Nano) + "|" + id))
}

func decodePullCursor(cursor string) (time.Time, string, error) {
	raw, err := base64.RawURLEncoding.DecodeString(cursor)
	if err != nil {
		return time.Time{}, "", err
	}
	at, id, found := strings.Cut(string(raw), "|")
	if !found {
		return time.Time{}, "", fmt.Errorf("malformed cursor")
	}
	parsed, err := time.Parse(time.RFC3339Nano, at)
	if err != nil {
		return time.Time{}, "", err
	}
	return parsed, id, nil
}

// DocumentRow carries the sort key alongside the document, so the handler can
// build a cursor without the contract shape having to carry updated_at -- which
// is a server-side ordering concern and no business of an edge.
type DocumentRow struct {
	Order     contracts.AggregatorOrder
	UpdatedAt time.Time
}

func (r *pgRepository) QueryDocuments(ctx context.Context, where string, args []any) ([]DocumentRow, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, tenant_id, outlet_id, platform, external_order_id, platform_status,
		        document_version, raw_payload, stated_total_paise, received_at, business_date,
		        updated_at
		   FROM aggregator_order`+where, args...)
	if err != nil {
		return nil, storage.Wrap("aggregators: querying documents", err)
	}
	defer rows.Close()

	out := []DocumentRow{}
	for rows.Next() {
		var d DocumentRow
		var receivedAt, businessDate time.Time
		if err := rows.Scan(
			&d.Order.ID, &d.Order.TenantID, &d.Order.OutletID, &d.Order.Platform,
			&d.Order.ExternalOrderID, &d.Order.PlatformStatus, &d.Order.DocumentVersion,
			&d.Order.RawPayload, &d.Order.StatedTotalPaise, &receivedAt, &businessDate,
			&d.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("aggregators: scanning document: %w", err)
		}
		d.Order.ReceivedAt = receivedAt.UTC().Format(time.RFC3339)
		d.Order.BusinessDate = businessDate.Format("2006-01-02")
		d.Order.SchemaVersion = 1
		d.Order.Lines = []contracts.AggregatorOrderLine{}
		out = append(out, d)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("aggregators: querying documents: %w", err)
	}

	// Lines fetched per document rather than in one join: a join repeats the
	// document across its lines, and the assembly that un-repeats them is where
	// a line goes missing when a page boundary lands mid-document.
	for i := range out {
		lines, err := r.linesFor(ctx, out[i].Order.ID)
		if err != nil {
			return nil, err
		}
		out[i].Order.Lines = lines
	}
	return out, nil
}

func (r *pgRepository) linesFor(ctx context.Context, documentID string) ([]contracts.AggregatorOrderLine, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, aggregator_order_id, line_number, external_item_id, external_item_name,
		        menu_item_id, quantity, stated_unit_price_paise
		   FROM aggregator_order_line WHERE aggregator_order_id = $1 ORDER BY line_number`,
		documentID)
	if err != nil {
		return nil, storage.Wrap("aggregators: querying lines", err)
	}
	defer rows.Close()

	out := []contracts.AggregatorOrderLine{}
	for rows.Next() {
		var l contracts.AggregatorOrderLine
		if err := rows.Scan(&l.ID, &l.AggregatorOrderID, &l.LineNumber, &l.ExternalItemID,
			&l.ExternalItemName, &l.MenuItemID, &l.Quantity, &l.StatedUnitPricePaise); err != nil {
			return nil, fmt.Errorf("aggregators: scanning line: %w", err)
		}
		l.SchemaVersion = 1
		out = append(out, l)
	}
	return out, rows.Err()
}
