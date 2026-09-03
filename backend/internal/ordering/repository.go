package ordering

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/storage"
	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5"
)

// Repository is the persistence boundary Service depends on. Every method
// that reads or writes an order takes tenantID as an explicit, mandatory
// parameter and every implementation must use it in the query itself — never
// as a post-hoc check on the loaded row — so a mistaken query can never
// silently return another tenant's data (mirrors internal/outlet's rule).
type Repository interface {
	// InsertOrder creates order under outletID, but only if outletID
	// belongs to tenantID. If an order with this id already exists
	// (edge retry), it is a no-op — the id, plus the append-only nature
	// of everything downstream, is the idempotency key. Returns the row
	// as stored (either just-inserted or pre-existing) and whether this
	// call actually inserted it.
	InsertOrder(ctx context.Context, tenantID, deviceID string, version int, order Order) (stored StoredOrder, inserted bool, err error)
	// GetByID returns httpx.ErrNotFound unless orderID's order belongs
	// (via its outlet) to tenantID.
	GetByID(ctx context.Context, tenantID, orderID string) (StoredOrder, error)
	// ListByOutlet returns every order for outletID, scoped to tenantID.
	ListByOutlet(ctx context.Context, tenantID, outletID string) ([]StoredOrder, error)
	// AppendItem inserts an order line item. Idempotent on item id: a
	// duplicate delivery of the same item id is a no-op. Financial line
	// items are append-only — there is no update or delete path.
	AppendItem(ctx context.Context, tenantID string, orderID string, item contracts.OrderItem) (inserted bool, err error)
	// ItemsForOrder returns every line item recorded against orderID, in
	// insertion order.
	ItemsForOrder(ctx context.Context, orderID string) ([]contracts.OrderItem, error)
	// UpdateStatus performs an optimistic-concurrency transition:
	// applies newStatus and bumps version to newVersion only if the
	// stored row's current version equals expectedCurrentVersion. Returns
	// the row as stored after the call (whether or not this call itself
	// applied the change) and whether this call applied it.
	UpdateStatus(ctx context.Context, tenantID, orderID string, expectedCurrentVersion, newVersion int, newStatus contracts.OrderStatus) (stored StoredOrder, applied bool, err error)
	// ConfirmOrder is UpdateStatus's DRAFT->CONFIRMED variant: it stamps
	// confirmed_at atomically with the status/version change. Deliberately a
	// separate method rather than a payload parameter bolted onto
	// UpdateStatus (ADR-011 0.2.5 addendum) — that generic path is shared by
	// SendToKitchen/Cancel, neither of which may carry a payload.
	ConfirmOrder(ctx context.Context, tenantID, orderID string, expectedCurrentVersion, newVersion int, confirmedAt time.Time) (stored StoredOrder, applied bool, err error)
}

// Order is the wire shape ingested from the edge; it mirrors
// contracts.CanonicalOrder exactly (CLAUDE.md: import contract types, never
// hand-roll mirrors).
type Order = contracts.CanonicalOrder

// StoredOrder is Order plus the cloud-side optimistic-concurrency version —
// the "order" table's version column, driven by the sync envelope's
// version field (contracts.CanonicalOrder itself carries no such field; the
// envelope is the only place edge/cloud agree on a record version).
type StoredOrder struct {
	Order
	Version int
}

// PostgresRepository is the Repository implementation backed by the
// packages/contracts/postgres schema.
type PostgresRepository struct {
	pool postgres.Pool
}

func NewPostgresRepository(pool postgres.Pool) *PostgresRepository {
	return &PostgresRepository{pool: pool}
}

// InsertOrder's idempotency is ON CONFLICT (id) DO NOTHING: correct for an
// identical replay of the same record_id, but it is a silent no-op if a
// DIFFERENT payload ever arrives reusing an existing id (e.g. a device_id
// bug reusing UUIDs). That would surface as "insert looked successful, row
// unchanged" rather than an explicit content-mismatch error. Accepted as a
// known trade-off for Milestone 1 — the edge is trusted not to reuse
// UUIDv7s — revisit if that trust assumption ever needs enforcing cloud-side.
func (r *PostgresRepository) InsertOrder(ctx context.Context, tenantID, deviceID string, version int, order Order) (StoredOrder, bool, error) {
	sourcePayload, err := json.Marshal(order.SourcePayload)
	if err != nil {
		return StoredOrder{}, false, fmt.Errorf("ordering: marshalling source_payload: %w", err)
	}

	tag, err := r.pool.Exec(ctx,
		`INSERT INTO "order" (id, outlet_id, device_id, order_type, status, table_id,
			subtotal_paise, discount_paise, taxes_paise, total_paise, version,
			source_payload, created_at, updated_at,
			source, external_order_id, payment_status, payment_source, confirmed_at, schema_version)
		 SELECT $1, $2, $3, $4, $5, $6,
			$7, $8, $9, $10, $11, $12, $13, $14,
			$16, $17, $18, $19, $20, $21
		 WHERE EXISTS (SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id WHERE o.id = $2 AND b.tenant_id = $15)
		 ON CONFLICT (id) DO NOTHING`,
		order.HollerOrderID, order.OutletID, deviceID, string(order.OrderType), string(order.Status), order.TableID,
		order.SubtotalPaise, order.DiscountPaise, order.TaxesPaise, order.TotalPaise, version,
		sourcePayload, order.Timestamps.CreatedAt, order.Timestamps.UpdatedAt, tenantID,
		string(order.Source), order.ExternalOrderID, string(order.PaymentStatus), order.PaymentSource,
		order.Timestamps.ConfirmedAt, order.SchemaVersion,
	)
	if err != nil {
		return StoredOrder{}, false, storage.Wrap("ordering: inserting order", err)
	}

	stored, getErr := r.GetByID(ctx, tenantID, order.HollerOrderID)
	if getErr != nil {
		if tag.RowsAffected() == 0 {
			// Neither an insert happened nor does the row exist for this
			// tenant: outletID did not belong to tenantID.
			return StoredOrder{}, false, httpx.ErrNotFound
		}
		return StoredOrder{}, false, getErr
	}
	return stored, tag.RowsAffected() > 0, nil
}

func (r *PostgresRepository) GetByID(ctx context.Context, tenantID, orderID string) (StoredOrder, error) {
	var so StoredOrder
	o := &so.Order
	var orderType, status, source, paymentStatus string
	var sourcePayload []byte
	err := r.pool.QueryRow(ctx,
		`SELECT ord.id, ord.outlet_id, ord.order_type, ord.status, ord.table_id,
			ord.subtotal_paise, ord.discount_paise, ord.taxes_paise, ord.total_paise,
			ord.version, ord.source_payload, ord.created_at, ord.updated_at,
			ord.source, ord.external_order_id, ord.payment_status, ord.payment_source,
			ord.confirmed_at, ord.schema_version
		 FROM "order" ord
		 JOIN outlet ot ON ot.id = ord.outlet_id
		 JOIN brand b ON b.id = ot.brand_id
		 WHERE ord.id = $1 AND b.tenant_id = $2`,
		orderID, tenantID,
	).Scan(&o.HollerOrderID, &o.OutletID, &orderType, &status, &o.TableID,
		&o.SubtotalPaise, &o.DiscountPaise, &o.TaxesPaise, &o.TotalPaise,
		&so.Version, &sourcePayload, &o.Timestamps.CreatedAt, &o.Timestamps.UpdatedAt,
		&source, &o.ExternalOrderID, &paymentStatus, &o.PaymentSource,
		&o.Timestamps.ConfirmedAt, &o.SchemaVersion)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return StoredOrder{}, httpx.ErrNotFound
		}
		return StoredOrder{}, fmt.Errorf("ordering: querying order: %w", err)
	}
	o.OrderType = contracts.OrderType(orderType)
	o.Status = contracts.OrderStatus(status)
	o.Source = contracts.OrderSource(source)
	o.PaymentStatus = contracts.PaymentStatus(paymentStatus)
	if len(sourcePayload) > 0 {
		if err := json.Unmarshal(sourcePayload, &o.SourcePayload); err != nil {
			return StoredOrder{}, fmt.Errorf("ordering: decoding source_payload: %w", err)
		}
	}

	// Deferred wire fields: no storage column yet (see the ADR-011 0.2.4
	// addendum's deferred-columns table). Synthesized at a fixed value until
	// their milestone lands storage for them — pinned by an exact-value test.
	o.PackagingPaise = 0
	o.DeliveryChargePaise = 0
	o.AggregatorDiscountPaise = 0
	o.MerchantDiscountPaise = 0
	o.Customer = nil
	o.DeliveryAddress = nil
	o.Rider = nil
	o.PreparationTimeMinutes = nil

	items, err := r.ItemsForOrder(ctx, orderID)
	if err != nil {
		return StoredOrder{}, err
	}
	o.Items = items
	return so, nil
}

func (r *PostgresRepository) ListByOutlet(ctx context.Context, tenantID, outletID string) ([]StoredOrder, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT ord.id
		 FROM "order" ord
		 JOIN outlet ot ON ot.id = ord.outlet_id
		 JOIN brand b ON b.id = ot.brand_id
		 WHERE ord.outlet_id = $1 AND b.tenant_id = $2
		 ORDER BY ord.created_at`,
		outletID, tenantID,
	)
	if err != nil {
		return nil, fmt.Errorf("ordering: listing orders: %w", err)
	}
	var ids []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			rows.Close()
			return nil, fmt.Errorf("ordering: scanning order id: %w", err)
		}
		ids = append(ids, id)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("ordering: iterating orders: %w", err)
	}

	orders := make([]StoredOrder, 0, len(ids))
	for _, id := range ids {
		o, err := r.GetByID(ctx, tenantID, id)
		if err != nil {
			return nil, err
		}
		orders = append(orders, o)
	}
	return orders, nil
}

// AppendItem shares InsertOrder's ON CONFLICT (id) DO NOTHING trade-off: a
// duplicate replay of the same item id is correctly a no-op, but a
// different payload reusing an existing item id would also silently no-op
// rather than error. Same accepted Milestone 1 trade-off as InsertOrder.
func (r *PostgresRepository) AppendItem(ctx context.Context, tenantID string, orderID string, item contracts.OrderItem) (bool, error) {
	tag, err := r.pool.Exec(ctx,
		`INSERT INTO order_item (id, order_id, menu_item_id, variant_id, quantity, unit_price_paise, line_total_paise, notes, created_at)
		 SELECT $1, $2, $3, $4, $5, $6, $7, $8, now()
		 WHERE EXISTS (
			SELECT 1 FROM "order" ord
			JOIN outlet ot ON ot.id = ord.outlet_id
			JOIN brand b ON b.id = ot.brand_id
			WHERE ord.id = $2 AND b.tenant_id = $9
		 )
		 ON CONFLICT (id) DO NOTHING`,
		item.ID, orderID, item.MenuItemID, item.VariantID, item.Quantity, item.UnitPricePaise, item.LineTotalPaise, item.Notes, tenantID,
	)
	if err != nil {
		return false, storage.Wrap("ordering: appending item", err)
	}
	return tag.RowsAffected() > 0, nil
}

func (r *PostgresRepository) ItemsForOrder(ctx context.Context, orderID string) ([]contracts.OrderItem, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, menu_item_id, variant_id, quantity, unit_price_paise, line_total_paise, notes
		 FROM order_item WHERE order_id = $1 ORDER BY created_at`,
		orderID,
	)
	if err != nil {
		return nil, fmt.Errorf("ordering: listing items: %w", err)
	}
	defer rows.Close()

	items := make([]contracts.OrderItem, 0)
	for rows.Next() {
		var it contracts.OrderItem
		if err := rows.Scan(&it.ID, &it.MenuItemID, &it.VariantID, &it.Quantity, &it.UnitPricePaise, &it.LineTotalPaise, &it.Notes); err != nil {
			return nil, fmt.Errorf("ordering: scanning item: %w", err)
		}
		it.Modifiers = []contracts.OrderItemModifier{}
		items = append(items, it)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("ordering: iterating items: %w", err)
	}
	return items, nil
}

func (r *PostgresRepository) UpdateStatus(ctx context.Context, tenantID, orderID string, expectedCurrentVersion, newVersion int, newStatus contracts.OrderStatus) (StoredOrder, bool, error) {
	tag, err := r.pool.Exec(ctx,
		`UPDATE "order" ord SET status = $1, version = $2, updated_at = now()
		 FROM outlet ot, brand b
		 WHERE ord.id = $3 AND ord.version = $4
		   AND ot.id = ord.outlet_id AND b.id = ot.brand_id AND b.tenant_id = $5`,
		string(newStatus), newVersion, orderID, expectedCurrentVersion, tenantID,
	)
	if err != nil {
		return StoredOrder{}, false, storage.Wrap("ordering: updating status", err)
	}

	stored, err := r.GetByID(ctx, tenantID, orderID)
	if err != nil {
		return StoredOrder{}, false, err
	}
	return stored, tag.RowsAffected() > 0, nil
}

// ConfirmOrder is UpdateStatus plus confirmed_at, applied in the same
// statement so the status/version bump and the timestamp land atomically.
// confirmedAt is the value the caller (Service) already validated came from
// the sync envelope's payload — this method never substitutes its own clock.
func (r *PostgresRepository) ConfirmOrder(ctx context.Context, tenantID, orderID string, expectedCurrentVersion, newVersion int, confirmedAt time.Time) (StoredOrder, bool, error) {
	tag, err := r.pool.Exec(ctx,
		`UPDATE "order" ord SET status = $1, version = $2, confirmed_at = $3, updated_at = now()
		 FROM outlet ot, brand b
		 WHERE ord.id = $4 AND ord.version = $5
		   AND ot.id = ord.outlet_id AND b.id = ot.brand_id AND b.tenant_id = $6`,
		string(contracts.OrderStatusConfirmed), newVersion, confirmedAt, orderID, expectedCurrentVersion, tenantID,
	)
	if err != nil {
		return StoredOrder{}, false, storage.Wrap("ordering: confirming order", err)
	}

	stored, err := r.GetByID(ctx, tenantID, orderID)
	if err != nil {
		return StoredOrder{}, false, err
	}
	return stored, tag.RowsAffected() > 0, nil
}
