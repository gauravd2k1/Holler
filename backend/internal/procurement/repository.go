package procurement

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/storage"
	contracts "github.com/holler/contracts"
)

// businessDateLayout mirrors backend/internal/inventory and
// backend/internal/payments: how a Postgres DATE column round-trips through
// this milestone's string-typed BusinessDate/InvoiceDate fields.
const businessDateLayout = "2006-01-02"

// Repository is the persistence boundary for the procurement context. Service
// depends on this interface, never on pgx directly (CLAUDE.md §Coding rules).
type Repository interface {
	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error

	// BumpOutletConfigVersion increments outlet.config_version by exactly one,
	// mirroring backend/internal/inventory and backend/internal/compliance.
	// Every cloud→edge config write in this package goes through it, which is
	// what makes GET /sync/config's since_version filter reach the new tables.
	BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error)

	// OutletBelongsToTenant is the CROSS-TENANT ISOLATION primitive. Every
	// write and every read in this package is scoped through it or through an
	// explicitly tenant-parameterised query — never through an id the caller
	// supplied on its own.
	OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error)

	// InventoryItemDimension resolves the referent's own dimension so the
	// write path can COMPARE the author's quantity_dimension against it. It
	// deliberately does not return something to copy: auto-filling would make
	// the comparison x == x and the guard could never fire (ADR-019 §6).
	InventoryItemDimension(ctx context.Context, itemID string) (Dimension, bool, error)

	// --- supplier + supplier_item -----------------------------------------

	UpsertSupplier(ctx context.Context, tx pgx.Tx, supplier Supplier, items []SupplierItem) error
	SuppliersSince(ctx context.Context, outletID string, sinceVersion int) ([]Supplier, error)
	SupplierItemsSince(ctx context.Context, outletID string, sinceVersion int) ([]SupplierItem, error)
	// SupplierOutlet resolves the outlet a supplier belongs to, so a write
	// naming only a supplier_id can still be tenant-checked.
	SupplierOutlet(ctx context.Context, supplierID string) (string, bool, error)

	// --- purchase_order + purchase_order_line -------------------------------

	UpsertPurchaseOrder(ctx context.Context, tx pgx.Tx, po PurchaseOrder) error
	// GetPurchaseOrder is TENANT-SCOPED at the query, not filtered afterwards.
	GetPurchaseOrder(ctx context.Context, tenantID, purchaseOrderID string) (PurchaseOrder, bool, error)
	PurchaseOrdersSince(ctx context.Context, outletID string, sinceVersion int) ([]PurchaseOrder, error)
	PurchaseOrderLinesSince(ctx context.Context, outletID string, sinceVersion int) ([]PurchaseOrderLine, error)
	PurchaseOrderLines(ctx context.Context, purchaseOrderID string) ([]PurchaseOrderLine, error)
	// ApprovePurchaseOrder writes approved_by_user_id AND approved_at
	// together, with the status transition, in one statement. There is no
	// method that writes either alone: an approval is whole or it did not
	// happen (ADR-019 §5).
	ApprovePurchaseOrder(ctx context.Context, tx pgx.Tx, purchaseOrderID, approverUserID string, approvedAt time.Time, configVersion int) error

	// --- approval limits (role-scoped, cloud-only) --------------------------

	// PoApprovalLimitForUser returns the highest po_approval_limit_paise
	// across the roles this user holds THAT ALSO CARRY procurement.approve,
	// scoped to the outlet or held tenant-wide.
	//
	// A NIL RETURN MEANS "MAY NOT APPROVE ANY AMOUNT" — no such role, or every
	// such role's limit is NULL. Absence is never read as unlimited (the
	// printer_role rule). The permission join is the first gate and the value
	// is the second; neither substitutes for the other.
	PoApprovalLimitForUser(ctx context.Context, tenantID, outletID, userID string) (*int64, error)
	// RolesAbleToApprove names the roles whose limit covers totalPaise, for
	// the §64 "who can approve it instead" half of the refusal message.
	RolesAbleToApprove(ctx context.Context, tenantID string, totalPaise int64) ([]string, error)

	// --- goods_receipt_note + grn_line (EDGE_TO_CLOUD replay) ---------------

	GetGoodsReceiptNoteByID(ctx context.Context, id string) (GoodsReceiptNote, bool, error)
	GrnLines(ctx context.Context, grnID string) ([]GrnLine, error)
	// InsertGoodsReceiptNote stores a receipt and its lines in one
	// transaction. IT PERFORMS NO PO LOOKUP AND NO PO VALIDATION: a receipt
	// whose purchase_order_id, supplier_id or line purchase_order_line_id is
	// null, or points at a row this cloud has never seen, is stored exactly as
	// received (ADR-019 §1).
	InsertGoodsReceiptNote(ctx context.Context, tx pgx.Tx, tenantID string, grn GoodsReceiptNote, lines []GrnLine) error

	// --- grn_gap (plain outbox: no entry_seq, no cursor, no contiguity) -----

	GetGrnGapByID(ctx context.Context, id string) (GrnGap, bool, error)
	InsertGrnGap(ctx context.Context, tenantID string, gap GrnGap) error

	// --- purchase_return / stock_transfer_out -------------------------------

	GetPurchaseReturnByID(ctx context.Context, id string) (PurchaseReturn, bool, error)
	PurchaseReturnLines(ctx context.Context, returnID string) ([]PurchaseReturnLine, error)
	InsertPurchaseReturn(ctx context.Context, tx pgx.Tx, tenantID string, ret PurchaseReturn, lines []PurchaseReturnLine) error

	GetStockTransferOutByID(ctx context.Context, id string) (StockTransferOut, bool, error)
	StockTransferLines(ctx context.Context, transferID string) ([]StockTransferLine, error)
	InsertStockTransferOut(ctx context.Context, tx pgx.Tx, tenantID string, transfer StockTransferOut, lines []StockTransferLine) error

	// --- derived receipt progress -------------------------------------------

	// ReceivedBaseQuantityByPurchaseOrderLine sums grn_line.base_quantity_micro
	// per purchase_order_line_id ACROSS EVERY OUTLET that replayed a receipt
	// against this order. That breadth is the whole point and is why this
	// figure legitimately differs from the edge's (ADR-019 §4). It is a read;
	// nothing writes it back onto purchase_order.
	ReceivedBaseQuantityByPurchaseOrderLine(ctx context.Context, purchaseOrderID string) (map[string]int64, error)

	// --- supplier_invoice / supplier_credit (CLOUD-ONLY, M5 = create+list) --

	InsertSupplierInvoice(ctx context.Context, inv SupplierInvoice) error
	ListSupplierInvoices(ctx context.Context, tenantID, outletID string) ([]SupplierInvoice, error)
	InsertSupplierCredit(ctx context.Context, credit SupplierCredit) error
	ListSupplierCredits(ctx context.Context, tenantID, outletID string) ([]SupplierCredit, error)

	// --- admin read paths (tenant-scoped in the QUERY, never afterwards) ----

	// GetSupplier is the tenant-scoped single read the update path needs
	// before it may touch a row. It joins outlet and brand for the same
	// reason GetPurchaseOrder does: a supplier id on its own carries no
	// tenancy, so a fetch-then-compare is one forgotten `if` from a
	// cross-tenant write.
	GetSupplier(ctx context.Context, tenantID, supplierID string) (Supplier, bool, error)
	ListSuppliers(ctx context.Context, tenantID string, filter SupplierFilter) ([]Supplier, error)
	// SupplierItemsForSuppliers fetches the price lists for a whole page of
	// suppliers in ONE statement. A per-supplier query here would be N+1 on
	// the screen a buyer opens most often.
	SupplierItemsForSuppliers(ctx context.Context, supplierIDs []string) (map[string][]SupplierItem, error)

	ListPurchaseOrders(ctx context.Context, tenantID string, filter PurchaseOrderFilter) ([]PurchaseOrder, error)
	PurchaseOrderLinesForOrders(ctx context.Context, purchaseOrderIDs []string) (map[string][]PurchaseOrderLine, error)

	// AmendPurchaseOrder rewrites an EXISTING order's contents and REVOKES ITS
	// APPROVAL in the same statement: approved_by_user_id and approved_at go
	// to NULL and the status leaves APPROVED/SENT.
	//
	// THAT IS THE WHOLE POINT OF THE METHOD. An amend that kept the approval
	// would make the ceiling in ApprovePurchaseOrder bypassable by raising a
	// small order, having it approved, and then amending it upward — the
	// approval would still read as granted and no gate would ever see the new
	// total. Re-approval is not friction here; it is the control.
	AmendPurchaseOrder(ctx context.Context, tx pgx.Tx, po PurchaseOrder) error
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
		return fmt.Errorf("procurement: begin tx: %w", err)
	}
	if err := fn(tx); err != nil {
		_ = tx.Rollback(ctx)
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("procurement: commit tx: %w", err)
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
		return 0, fmt.Errorf("procurement: bumping outlet config_version: %w", err)
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
		return false, fmt.Errorf("procurement: checking outlet tenancy: %w", err)
	}
	return exists, nil
}

func (r *pgRepository) InventoryItemDimension(ctx context.Context, itemID string) (Dimension, bool, error) {
	var dim string
	err := r.pool.QueryRow(ctx, `SELECT dimension FROM inventory_item WHERE id = $1`, itemID).Scan(&dim)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("procurement: getting inventory item dimension: %w", err)
	}
	return Dimension(dim), true, nil
}

// --- supplier ---------------------------------------------------------------

func (r *pgRepository) UpsertSupplier(ctx context.Context, tx pgx.Tx, s Supplier, items []SupplierItem) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO supplier (id, outlet_id, code, name, gstin, phone, email, address,
		                       payment_terms_days, is_active, config_version, created_at, updated_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
		 ON CONFLICT (id) DO UPDATE SET
		   code = EXCLUDED.code, name = EXCLUDED.name, gstin = EXCLUDED.gstin,
		   phone = EXCLUDED.phone, email = EXCLUDED.email, address = EXCLUDED.address,
		   payment_terms_days = EXCLUDED.payment_terms_days, is_active = EXCLUDED.is_active,
		   config_version = EXCLUDED.config_version, updated_at = EXCLUDED.updated_at`,
		s.ID, s.OutletID, s.Code, s.Name, s.Gstin, s.Phone, s.Email, s.Address,
		s.PaymentTermsDays, s.IsActive, s.ConfigVersion, s.CreatedAt, s.UpdatedAt,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: supplier code %q already exists in this outlet", httpx.ErrConflict, s.Code)
		}
		return fmt.Errorf("procurement: upserting supplier: %w", err)
	}

	// Child rows are REPLACED wholesale, the station_printer/item_unit_
	// conversion PUT-not-merge precedent: a price list is submitted whole, so
	// a row the caller omitted is a deletion, not an oversight.
	if _, err := tx.Exec(ctx, `DELETE FROM supplier_item WHERE supplier_id = $1`, s.ID); err != nil {
		return fmt.Errorf("procurement: clearing supplier items: %w", err)
	}
	for _, it := range items {
		if _, err := tx.Exec(ctx,
			`INSERT INTO supplier_item (id, supplier_id, inventory_item_id, purchase_unit,
			                            pack_size_micro, quantity_dimension, last_price_paise, is_preferred)
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
			it.ID, s.ID, it.InventoryItemID, it.PurchaseUnit,
			it.PackSizeMicro, string(it.QuantityDimension), it.LastPricePaise, it.IsPreferred,
		); err != nil {
			if storage.IsUniqueViolation(err) {
				return fmt.Errorf("%w: supplier already prices item %s in unit %q",
					httpx.ErrConflict, it.InventoryItemID, it.PurchaseUnit)
			}
			return fmt.Errorf("procurement: inserting supplier item: %w", err)
		}
	}
	return nil
}

const supplierSelect = `SELECT id, outlet_id, code, name, gstin, phone, email, address,
	       payment_terms_days, is_active, config_version, created_at, updated_at
	FROM supplier`

func (r *pgRepository) SuppliersSince(ctx context.Context, outletID string, sinceVersion int) ([]Supplier, error) {
	rows, err := r.pool.Query(ctx,
		supplierSelect+` WHERE outlet_id = $1 AND config_version > $2 ORDER BY config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing suppliers since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []Supplier
	for rows.Next() {
		var s Supplier
		var createdAt, updatedAt time.Time
		if err := rows.Scan(&s.ID, &s.OutletID, &s.Code, &s.Name, &s.Gstin, &s.Phone, &s.Email, &s.Address,
			&s.PaymentTermsDays, &s.IsActive, &s.ConfigVersion, &createdAt, &updatedAt); err != nil {
			return nil, fmt.Errorf("procurement: scanning supplier: %w", err)
		}
		s.CreatedAt = createdAt.UTC().Format(time.RFC3339)
		s.UpdatedAt = updatedAt.UTC().Format(time.RFC3339)
		s.SchemaVersion = 1
		out = append(out, s)
	}
	return out, rows.Err()
}

func (r *pgRepository) SupplierItemsSince(ctx context.Context, outletID string, sinceVersion int) ([]SupplierItem, error) {
	// supplier_item has no config_version of its own — it is a child row, so
	// it travels whenever its parent does, filtered on the PARENT's version.
	rows, err := r.pool.Query(ctx,
		`SELECT si.id, si.supplier_id, si.inventory_item_id, si.purchase_unit, si.pack_size_micro,
		        si.quantity_dimension, si.last_price_paise, si.is_preferred
		 FROM supplier_item si
		 JOIN supplier s ON s.id = si.supplier_id
		 WHERE s.outlet_id = $1 AND s.config_version > $2
		 ORDER BY s.config_version, si.id`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing supplier items since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []SupplierItem
	for rows.Next() {
		var it SupplierItem
		var dim string
		if err := rows.Scan(&it.ID, &it.SupplierID, &it.InventoryItemID, &it.PurchaseUnit, &it.PackSizeMicro,
			&dim, &it.LastPricePaise, &it.IsPreferred); err != nil {
			return nil, fmt.Errorf("procurement: scanning supplier item: %w", err)
		}
		it.QuantityDimension = Dimension(dim)
		it.SchemaVersion = 1
		out = append(out, it)
	}
	return out, rows.Err()
}

func (r *pgRepository) SupplierOutlet(ctx context.Context, supplierID string) (string, bool, error) {
	var outletID string
	err := r.pool.QueryRow(ctx, `SELECT outlet_id FROM supplier WHERE id = $1`, supplierID).Scan(&outletID)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", false, nil
	}
	if err != nil {
		return "", false, fmt.Errorf("procurement: getting supplier outlet: %w", err)
	}
	return outletID, true, nil
}

// --- purchase_order ---------------------------------------------------------

func (r *pgRepository) UpsertPurchaseOrder(ctx context.Context, tx pgx.Tx, po PurchaseOrder) error {
	// approved_by_user_id / approved_at are ABSENT from the INSERT column list
	// deliberately: this route may not grant an approval.
	//
	// ON CONFLICT THEY ARE SET TO NULL — this route may not PRESERVE one
	// either. An upsert that re-wrote total_paise and lines while leaving the
	// approval standing would let a caller raise a small order, have it
	// approved, then post it again ten times larger with the approval still on
	// the row. It could never reach status APPROVED that way (this route
	// rejects that status), but it would still read as approved by name and
	// date on every screen and in every audit answer to "who authorised this
	// spend". Approval belongs to the contents that were approved. Amending
	// the contents revokes it, on BOTH amend paths, identically
	// (AmendPurchaseOrder does the same in one UPDATE).
	_, err := tx.Exec(ctx,
		`INSERT INTO purchase_order (id, outlet_id, supplier_id, po_number, status, expected_date,
		                             notes, total_paise, created_at, updated_at, config_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		 ON CONFLICT (id) DO UPDATE SET
		   supplier_id = EXCLUDED.supplier_id, po_number = EXCLUDED.po_number,
		   status = EXCLUDED.status, expected_date = EXCLUDED.expected_date,
		   notes = EXCLUDED.notes, total_paise = EXCLUDED.total_paise,
		   approved_by_user_id = NULL, approved_at = NULL,
		   updated_at = EXCLUDED.updated_at, config_version = EXCLUDED.config_version`,
		po.ID, po.OutletID, po.SupplierID, po.PoNumber, string(po.Status), po.ExpectedDate,
		po.Notes, po.TotalPaise, po.CreatedAt, po.CreatedAt, po.ConfigVersion,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: po_number %q already exists in this outlet", httpx.ErrConflict, po.PoNumber)
		}
		return fmt.Errorf("procurement: upserting purchase order: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM purchase_order_line WHERE purchase_order_id = $1`, po.ID); err != nil {
		return fmt.Errorf("procurement: clearing purchase order lines: %w", err)
	}
	for _, l := range po.Lines {
		if _, err := tx.Exec(ctx,
			`INSERT INTO purchase_order_line (id, purchase_order_id, inventory_item_id, line_number,
			                                  purchase_unit, ordered_quantity_micro, quantity_dimension,
			                                  unit_price_paise, line_total_paise)
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)`,
			l.ID, po.ID, l.InventoryItemID, l.LineNumber, l.PurchaseUnit, l.OrderedQuantityMicro,
			string(l.QuantityDimension), l.UnitPricePaise, l.LineTotalPaise,
		); err != nil {
			if storage.IsUniqueViolation(err) {
				return fmt.Errorf("%w: purchase order line_number %d is duplicated", httpx.ErrConflict, l.LineNumber)
			}
			return fmt.Errorf("procurement: inserting purchase order line: %w", err)
		}
	}
	return nil
}

const purchaseOrderColumns = `id, outlet_id, supplier_id, po_number, status, expected_date, notes,
	       total_paise, approved_by_user_id, approved_at, created_at, config_version`

// purchaseOrderColumnsQualified is the SAME column list carrying the `po.`
// alias, for the tenant-scoped read that joins outlet and brand.
//
// IT IS A SEPARATE CONSTANT BECAUSE THE UNQUALIFIED ONE IS A RUNTIME ERROR IN
// A JOIN. purchase_order, outlet and brand each have id / created_at, and
// outlet also has outlet-side columns, so an unqualified `id` in that query is
// "column reference is ambiguous" (SQLSTATE 42702) — every call fails, always.
// It shipped that way and no amount of static checking found it: the string
// was internally consistent and every column name really did exist. It took
// one query against a live server.
const purchaseOrderColumnsQualified = `po.id, po.outlet_id, po.supplier_id, po.po_number, po.status,
	       po.expected_date, po.notes, po.total_paise, po.approved_by_user_id, po.approved_at,
	       po.created_at, po.config_version`

func scanPurchaseOrder(row rowScanner) (PurchaseOrder, bool, error) {
	var po PurchaseOrder
	var status string
	var expectedDate, approvedAt *time.Time
	var createdAt time.Time
	err := row.Scan(&po.ID, &po.OutletID, &po.SupplierID, &po.PoNumber, &status, &expectedDate, &po.Notes,
		&po.TotalPaise, &po.ApprovedByUserID, &approvedAt, &createdAt, &po.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return PurchaseOrder{}, false, nil
	}
	if err != nil {
		return PurchaseOrder{}, false, fmt.Errorf("procurement: scanning purchase order: %w", err)
	}
	po.Status = PurchaseOrderStatus(status)
	if expectedDate != nil {
		d := expectedDate.Format(businessDateLayout)
		po.ExpectedDate = &d
	}
	if approvedAt != nil {
		a := approvedAt.UTC().Format(time.RFC3339)
		po.ApprovedAt = &a
	}
	po.CreatedAt = createdAt.UTC().Format(time.RFC3339)
	po.SchemaVersion = 1
	po.Lines = []PurchaseOrderLine{}
	return po, true, nil
}

type rowScanner interface {
	Scan(dest ...interface{}) error
}

func (r *pgRepository) GetPurchaseOrder(ctx context.Context, tenantID, purchaseOrderID string) (PurchaseOrder, bool, error) {
	// Tenancy is IN the query. A tenant-blind fetch followed by a Go-side
	// comparison is one forgotten `if` away from a cross-tenant read.
	po, found, err := scanPurchaseOrder(r.pool.QueryRow(ctx,
		`SELECT `+purchaseOrderColumnsQualified+`
		 FROM purchase_order po
		 JOIN outlet o ON o.id = po.outlet_id
		 JOIN brand b ON b.id = o.brand_id
		 WHERE po.id = $1 AND b.tenant_id = $2`,
		purchaseOrderID, tenantID,
	))
	if err != nil || !found {
		return PurchaseOrder{}, found, err
	}
	lines, err := r.PurchaseOrderLines(ctx, po.ID)
	if err != nil {
		return PurchaseOrder{}, false, err
	}
	po.Lines = lines
	return po, true, nil
}

const purchaseOrderLineSelect = `SELECT id, purchase_order_id, inventory_item_id, line_number, purchase_unit,
	       ordered_quantity_micro, quantity_dimension, unit_price_paise, line_total_paise
	FROM purchase_order_line`

func scanPurchaseOrderLines(rows pgx.Rows) ([]PurchaseOrderLine, error) {
	defer rows.Close()
	out := []PurchaseOrderLine{}
	for rows.Next() {
		var l PurchaseOrderLine
		var dim string
		if err := rows.Scan(&l.ID, &l.PurchaseOrderID, &l.InventoryItemID, &l.LineNumber, &l.PurchaseUnit,
			&l.OrderedQuantityMicro, &dim, &l.UnitPricePaise, &l.LineTotalPaise); err != nil {
			return nil, fmt.Errorf("procurement: scanning purchase order line: %w", err)
		}
		l.QuantityDimension = Dimension(dim)
		out = append(out, l)
	}
	return out, rows.Err()
}

func (r *pgRepository) PurchaseOrderLines(ctx context.Context, purchaseOrderID string) ([]PurchaseOrderLine, error) {
	rows, err := r.pool.Query(ctx, purchaseOrderLineSelect+` WHERE purchase_order_id = $1 ORDER BY line_number`, purchaseOrderID)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing purchase order lines: %w", err)
	}
	return scanPurchaseOrderLines(rows)
}

func (r *pgRepository) PurchaseOrdersSince(ctx context.Context, outletID string, sinceVersion int) ([]PurchaseOrder, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT `+purchaseOrderColumns+` FROM purchase_order
		 WHERE outlet_id = $1 AND config_version > $2 ORDER BY config_version`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing purchase orders since %d: %w", sinceVersion, err)
	}
	defer rows.Close()
	var out []PurchaseOrder
	for rows.Next() {
		po, _, err := scanPurchaseOrder(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, po)
	}
	return out, rows.Err()
}

func (r *pgRepository) PurchaseOrderLinesSince(ctx context.Context, outletID string, sinceVersion int) ([]PurchaseOrderLine, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT pol.id, pol.purchase_order_id, pol.inventory_item_id, pol.line_number, pol.purchase_unit,
		        pol.ordered_quantity_micro, pol.quantity_dimension, pol.unit_price_paise, pol.line_total_paise
		 FROM purchase_order_line pol
		 JOIN purchase_order po ON po.id = pol.purchase_order_id
		 WHERE po.outlet_id = $1 AND po.config_version > $2
		 ORDER BY po.config_version, pol.line_number`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing purchase order lines since %d: %w", sinceVersion, err)
	}
	return scanPurchaseOrderLines(rows)
}

func (r *pgRepository) ApprovePurchaseOrder(ctx context.Context, tx pgx.Tx, purchaseOrderID, approverUserID string, approvedAt time.Time, configVersion int) error {
	// ONE statement writes both approval columns and the status. There is no
	// path in this package that can set one without the other.
	tag, err := tx.Exec(ctx,
		`UPDATE purchase_order
		 SET status = $1, approved_by_user_id = $2, approved_at = $3,
		     updated_at = $3, config_version = $4
		 WHERE id = $5`,
		string(PurchaseOrderStatusApproved), approverUserID, approvedAt, configVersion, purchaseOrderID,
	)
	if err != nil {
		return fmt.Errorf("procurement: approving purchase order: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("%w: purchase order %s", httpx.ErrNotFound, purchaseOrderID)
	}
	return nil
}

// --- approval limits --------------------------------------------------------

func (r *pgRepository) PoApprovalLimitForUser(ctx context.Context, tenantID, outletID, userID string) (*int64, error) {
	var limit *int64
	err := r.pool.QueryRow(ctx,
		`SELECT MAX(r.po_approval_limit_paise)
		 FROM user_role ur
		 JOIN role r ON r.id = ur.role_id
		 JOIN role_permission rp ON rp.role_id = r.id AND rp.permission = $1
		 WHERE ur.user_id = $2 AND r.tenant_id = $3
		   AND (ur.outlet_id IS NULL OR ur.outlet_id = $4)`,
		string(PermissionApprove), userID, tenantID, outletID,
	).Scan(&limit)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("procurement: reading po approval limit: %w", err)
	}
	return limit, nil
}

func (r *pgRepository) RolesAbleToApprove(ctx context.Context, tenantID string, totalPaise int64) ([]string, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT DISTINCT r.name
		 FROM role r
		 JOIN role_permission rp ON rp.role_id = r.id AND rp.permission = $1
		 WHERE r.tenant_id = $2
		   AND r.po_approval_limit_paise IS NOT NULL
		   AND r.po_approval_limit_paise >= $3
		 ORDER BY r.name`,
		string(PermissionApprove), tenantID, totalPaise,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing roles able to approve: %w", err)
	}
	defer rows.Close()
	out := []string{}
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return nil, fmt.Errorf("procurement: scanning approver role: %w", err)
		}
		out = append(out, name)
	}
	return out, rows.Err()
}

// --- goods_receipt_note -----------------------------------------------------

const grnSelect = `SELECT id, outlet_id, purchase_order_id, supplier_id, grn_number, delivery_note_ref,
	       received_at, received_by_user_id, business_date, notes
	FROM goods_receipt_note`

func (r *pgRepository) GetGoodsReceiptNoteByID(ctx context.Context, id string) (GoodsReceiptNote, bool, error) {
	var g GoodsReceiptNote
	var receivedAt, businessDate time.Time
	err := r.pool.QueryRow(ctx, grnSelect+` WHERE id = $1`, id).Scan(
		&g.ID, &g.OutletID, &g.PurchaseOrderID, &g.SupplierID, &g.GrnNumber, &g.DeliveryNoteRef,
		&receivedAt, &g.ReceivedByUserID, &businessDate, &g.Notes)
	if errors.Is(err, pgx.ErrNoRows) {
		return GoodsReceiptNote{}, false, nil
	}
	if err != nil {
		return GoodsReceiptNote{}, false, fmt.Errorf("procurement: getting goods receipt note: %w", err)
	}
	g.ReceivedAt = receivedAt.UTC().Format(time.RFC3339)
	g.BusinessDate = businessDate.Format(businessDateLayout)
	g.SchemaVersion = 1
	g.Lines = []GrnLine{}
	return g, true, nil
}

func (r *pgRepository) GrnLines(ctx context.Context, grnID string) ([]GrnLine, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, grn_id, inventory_item_id, line_number, purchase_order_line_id,
		        entered_purchase_unit, entered_quantity_micro, quantity_dimension,
		        base_quantity_micro, pack_size_micro_applied, unit_cost_paise, line_total_paise,
		        batch_code, expiry_date
		 FROM grn_line WHERE grn_id = $1 ORDER BY line_number`,
		grnID,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing grn lines: %w", err)
	}
	defer rows.Close()
	out := []GrnLine{}
	for rows.Next() {
		var l GrnLine
		var dim string
		var expiry *time.Time
		if err := rows.Scan(&l.ID, &l.GrnID, &l.InventoryItemID, &l.LineNumber, &l.PurchaseOrderLineID,
			&l.EnteredPurchaseUnit, &l.EnteredQuantityMicro, &dim,
			&l.BaseQuantityMicro, &l.PackSizeMicroApplied, &l.UnitCostPaise, &l.LineTotalPaise,
			&l.BatchCode, &expiry); err != nil {
			return nil, fmt.Errorf("procurement: scanning grn line: %w", err)
		}
		l.QuantityDimension = Dimension(dim)
		if expiry != nil {
			e := expiry.Format(businessDateLayout)
			l.ExpiryDate = &e
		}
		out = append(out, l)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertGoodsReceiptNote(ctx context.Context, tx pgx.Tx, tenantID string, g GoodsReceiptNote, lines []GrnLine) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO goods_receipt_note (id, tenant_id, outlet_id, purchase_order_id, supplier_id,
		                                 grn_number, delivery_note_ref, received_at, received_by_user_id,
		                                 business_date, notes, schema_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)`,
		g.ID, tenantID, g.OutletID, g.PurchaseOrderID, g.SupplierID,
		g.GrnNumber, g.DeliveryNoteRef, g.ReceivedAt, g.ReceivedByUserID,
		g.BusinessDate, g.Notes, 1,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: grn_number %q already exists in this outlet", httpx.ErrConflict, g.GrnNumber)
		}
		return fmt.Errorf("procurement: inserting goods receipt note: %w", err)
	}
	for _, l := range lines {
		// Both sides of the conversion are stored EXACTLY AS RECEIVED, and
		// nothing here recomputes either: recomputing base_quantity_micro
		// against a since-edited supplier_item would silently restate history
		// (ADR-019 §3).
		if _, err := tx.Exec(ctx,
			`INSERT INTO grn_line (id, grn_id, inventory_item_id, line_number, purchase_order_line_id,
			                       entered_purchase_unit, entered_quantity_micro, quantity_dimension,
			                       base_quantity_micro, pack_size_micro_applied, unit_cost_paise,
			                       line_total_paise, batch_code, expiry_date)
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)`,
			l.ID, g.ID, l.InventoryItemID, l.LineNumber, l.PurchaseOrderLineID,
			l.EnteredPurchaseUnit, l.EnteredQuantityMicro, string(l.QuantityDimension),
			l.BaseQuantityMicro, l.PackSizeMicroApplied, l.UnitCostPaise,
			l.LineTotalPaise, l.BatchCode, l.ExpiryDate,
		); err != nil {
			return fmt.Errorf("procurement: inserting grn line: %w", err)
		}
	}
	return nil
}

// --- grn_gap ----------------------------------------------------------------

func (r *pgRepository) GetGrnGapByID(ctx context.Context, id string) (GrnGap, bool, error) {
	var g GrnGap
	var reason string
	var occurredAt, businessDate time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, grn_id, grn_line_id, inventory_item_id, reason, detail,
		        occurred_at, business_date
		 FROM grn_gap WHERE id = $1`, id,
	).Scan(&g.ID, &g.OutletID, &g.GrnID, &g.GrnLineID, &g.InventoryItemID, &reason, &g.Detail,
		&occurredAt, &businessDate)
	if errors.Is(err, pgx.ErrNoRows) {
		return GrnGap{}, false, nil
	}
	if err != nil {
		return GrnGap{}, false, fmt.Errorf("procurement: getting grn gap: %w", err)
	}
	g.Reason = GrnGapReason(reason)
	g.OccurredAt = occurredAt.UTC().Format(time.RFC3339)
	g.BusinessDate = businessDate.Format(businessDateLayout)
	g.SchemaVersion = 1
	return g, true, nil
}

func (r *pgRepository) InsertGrnGap(ctx context.Context, tenantID string, g GrnGap) error {
	// PLAIN OUTBOX: no entry_seq, no cursor, no contiguity check — a grn_gap
	// is a discrete event a buyer acts on, not a per-sale stream, which is why
	// stock_deduction_gap earned 0.5.8's machinery and this does not.
	_, err := r.pool.Exec(ctx,
		`INSERT INTO grn_gap (id, tenant_id, outlet_id, grn_id, grn_line_id, inventory_item_id,
		                      reason, detail, occurred_at, business_date, schema_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)`,
		g.ID, tenantID, g.OutletID, g.GrnID, g.GrnLineID, g.InventoryItemID,
		string(g.Reason), g.Detail, g.OccurredAt, g.BusinessDate, 1,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: grn_gap %s already exists", httpx.ErrConflict, g.ID)
		}
		return fmt.Errorf("procurement: inserting grn gap: %w", err)
	}
	return nil
}

// --- purchase_return --------------------------------------------------------

func (r *pgRepository) GetPurchaseReturnByID(ctx context.Context, id string) (PurchaseReturn, bool, error) {
	var p PurchaseReturn
	var reason string
	var returnedAt, businessDate time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, supplier_id, grn_id, return_number, reason, returned_at,
		        returned_by_user_id, business_date, notes
		 FROM purchase_return WHERE id = $1`, id,
	).Scan(&p.ID, &p.OutletID, &p.SupplierID, &p.GrnID, &p.ReturnNumber, &reason, &returnedAt,
		&p.ReturnedByUserID, &businessDate, &p.Notes)
	if errors.Is(err, pgx.ErrNoRows) {
		return PurchaseReturn{}, false, nil
	}
	if err != nil {
		return PurchaseReturn{}, false, fmt.Errorf("procurement: getting purchase return: %w", err)
	}
	p.Reason = PurchaseReturnReason(reason)
	p.ReturnedAt = returnedAt.UTC().Format(time.RFC3339)
	p.BusinessDate = businessDate.Format(businessDateLayout)
	p.SchemaVersion = 1
	p.Lines = []PurchaseReturnLine{}
	return p, true, nil
}

func (r *pgRepository) PurchaseReturnLines(ctx context.Context, returnID string) ([]PurchaseReturnLine, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, purchase_return_id, inventory_item_id, grn_line_id, line_number,
		        entered_purchase_unit, entered_quantity_micro, quantity_dimension,
		        base_quantity_micro, unit_cost_paise
		 FROM purchase_return_line WHERE purchase_return_id = $1 ORDER BY line_number`,
		returnID,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing purchase return lines: %w", err)
	}
	defer rows.Close()
	out := []PurchaseReturnLine{}
	for rows.Next() {
		var l PurchaseReturnLine
		var dim string
		if err := rows.Scan(&l.ID, &l.PurchaseReturnID, &l.InventoryItemID, &l.GrnLineID, &l.LineNumber,
			&l.EnteredPurchaseUnit, &l.EnteredQuantityMicro, &dim, &l.BaseQuantityMicro, &l.UnitCostPaise); err != nil {
			return nil, fmt.Errorf("procurement: scanning purchase return line: %w", err)
		}
		l.QuantityDimension = Dimension(dim)
		out = append(out, l)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertPurchaseReturn(ctx context.Context, tx pgx.Tx, tenantID string, p PurchaseReturn, lines []PurchaseReturnLine) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO purchase_return (id, tenant_id, outlet_id, supplier_id, grn_id, return_number,
		                              reason, returned_at, returned_by_user_id, business_date, notes, schema_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)`,
		p.ID, tenantID, p.OutletID, p.SupplierID, p.GrnID, p.ReturnNumber,
		string(p.Reason), p.ReturnedAt, p.ReturnedByUserID, p.BusinessDate, p.Notes, 1,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: return_number %q already exists in this outlet", httpx.ErrConflict, p.ReturnNumber)
		}
		return fmt.Errorf("procurement: inserting purchase return: %w", err)
	}
	for _, l := range lines {
		if _, err := tx.Exec(ctx,
			`INSERT INTO purchase_return_line (id, purchase_return_id, inventory_item_id, grn_line_id,
			                                   line_number, entered_purchase_unit, entered_quantity_micro,
			                                   quantity_dimension, base_quantity_micro, unit_cost_paise)
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)`,
			l.ID, p.ID, l.InventoryItemID, l.GrnLineID, l.LineNumber, l.EnteredPurchaseUnit,
			l.EnteredQuantityMicro, string(l.QuantityDimension), l.BaseQuantityMicro, l.UnitCostPaise,
		); err != nil {
			return fmt.Errorf("procurement: inserting purchase return line: %w", err)
		}
	}
	return nil
}

// --- stock_transfer_out -----------------------------------------------------

func (r *pgRepository) GetStockTransferOutByID(ctx context.Context, id string) (StockTransferOut, bool, error) {
	var s StockTransferOut
	var dispatchedAt, businessDate time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, destination_outlet_id, transfer_number, dispatched_at,
		        dispatched_by_user_id, business_date, notes
		 FROM stock_transfer_out WHERE id = $1`, id,
	).Scan(&s.ID, &s.OutletID, &s.DestinationOutletID, &s.TransferNumber, &dispatchedAt,
		&s.DispatchedByUserID, &businessDate, &s.Notes)
	if errors.Is(err, pgx.ErrNoRows) {
		return StockTransferOut{}, false, nil
	}
	if err != nil {
		return StockTransferOut{}, false, fmt.Errorf("procurement: getting stock transfer out: %w", err)
	}
	s.DispatchedAt = dispatchedAt.UTC().Format(time.RFC3339)
	s.BusinessDate = businessDate.Format(businessDateLayout)
	s.SchemaVersion = 1
	s.Lines = []StockTransferLine{}
	return s, true, nil
}

func (r *pgRepository) StockTransferLines(ctx context.Context, transferID string) ([]StockTransferLine, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, stock_transfer_out_id, inventory_item_id, line_number,
		        base_quantity_micro, quantity_dimension, unit_cost_paise
		 FROM stock_transfer_line WHERE stock_transfer_out_id = $1 ORDER BY line_number`,
		transferID,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing stock transfer lines: %w", err)
	}
	defer rows.Close()
	out := []StockTransferLine{}
	for rows.Next() {
		var l StockTransferLine
		var dim string
		if err := rows.Scan(&l.ID, &l.StockTransferOutID, &l.InventoryItemID, &l.LineNumber,
			&l.BaseQuantityMicro, &dim, &l.UnitCostPaise); err != nil {
			return nil, fmt.Errorf("procurement: scanning stock transfer line: %w", err)
		}
		l.QuantityDimension = Dimension(dim)
		out = append(out, l)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertStockTransferOut(ctx context.Context, tx pgx.Tx, tenantID string, s StockTransferOut, lines []StockTransferLine) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO stock_transfer_out (id, tenant_id, outlet_id, destination_outlet_id, transfer_number,
		                                 dispatched_at, dispatched_by_user_id, business_date, notes, schema_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)`,
		s.ID, tenantID, s.OutletID, s.DestinationOutletID, s.TransferNumber,
		s.DispatchedAt, s.DispatchedByUserID, s.BusinessDate, s.Notes, 1,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: transfer_number %q already exists in this outlet", httpx.ErrConflict, s.TransferNumber)
		}
		return fmt.Errorf("procurement: inserting stock transfer out: %w", err)
	}
	for _, l := range lines {
		if _, err := tx.Exec(ctx,
			`INSERT INTO stock_transfer_line (id, stock_transfer_out_id, inventory_item_id, line_number,
			                                  base_quantity_micro, quantity_dimension, unit_cost_paise)
			 VALUES ($1,$2,$3,$4,$5,$6,$7)`,
			l.ID, s.ID, l.InventoryItemID, l.LineNumber, l.BaseQuantityMicro,
			string(l.QuantityDimension), l.UnitCostPaise,
		); err != nil {
			return fmt.Errorf("procurement: inserting stock transfer line: %w", err)
		}
	}
	return nil
}

// --- derived receipt progress -----------------------------------------------

func (r *pgRepository) ReceivedBaseQuantityByPurchaseOrderLine(ctx context.Context, purchaseOrderID string) (map[string]int64, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT gl.purchase_order_line_id, SUM(gl.base_quantity_micro)
		 FROM grn_line gl
		 JOIN purchase_order_line pol ON pol.id = gl.purchase_order_line_id
		 WHERE pol.purchase_order_id = $1
		 GROUP BY gl.purchase_order_line_id`,
		purchaseOrderID,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: deriving receipt progress: %w", err)
	}
	defer rows.Close()
	out := map[string]int64{}
	for rows.Next() {
		var lineID string
		var received int64
		if err := rows.Scan(&lineID, &received); err != nil {
			return nil, fmt.Errorf("procurement: scanning receipt progress: %w", err)
		}
		out[lineID] = received
	}
	return out, rows.Err()
}

// --- supplier_invoice / supplier_credit -------------------------------------

func (r *pgRepository) InsertSupplierInvoice(ctx context.Context, inv SupplierInvoice) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO supplier_invoice (id, tenant_id, outlet_id, supplier_id, grn_id, supplier_invoice_no,
		                               invoice_date, due_date, subtotal_paise, tax_paise, total_paise,
		                               status, created_at, updated_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)`,
		inv.ID, inv.TenantID, inv.OutletID, inv.SupplierID, inv.GrnID, inv.SupplierInvoiceNo,
		inv.InvoiceDate, inv.DueDate, inv.SubtotalPaise, inv.TaxPaise, inv.TotalPaise,
		string(inv.Status), inv.CreatedAt, inv.UpdatedAt,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: supplier_invoice_no %q already exists for this supplier", httpx.ErrConflict, inv.SupplierInvoiceNo)
		}
		return fmt.Errorf("procurement: inserting supplier invoice: %w", err)
	}
	return nil
}

func (r *pgRepository) ListSupplierInvoices(ctx context.Context, tenantID, outletID string) ([]SupplierInvoice, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, tenant_id, outlet_id, supplier_id, grn_id, supplier_invoice_no, invoice_date,
		        due_date, subtotal_paise, tax_paise, total_paise, status, created_at, updated_at
		 FROM supplier_invoice
		 WHERE tenant_id = $1 AND outlet_id = $2 ORDER BY invoice_date DESC, id`,
		tenantID, outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing supplier invoices: %w", err)
	}
	defer rows.Close()
	out := []SupplierInvoice{}
	for rows.Next() {
		var inv SupplierInvoice
		var status string
		var invoiceDate time.Time
		var dueDate *time.Time
		var createdAt, updatedAt time.Time
		if err := rows.Scan(&inv.ID, &inv.TenantID, &inv.OutletID, &inv.SupplierID, &inv.GrnID,
			&inv.SupplierInvoiceNo, &invoiceDate, &dueDate, &inv.SubtotalPaise, &inv.TaxPaise,
			&inv.TotalPaise, &status, &createdAt, &updatedAt); err != nil {
			return nil, fmt.Errorf("procurement: scanning supplier invoice: %w", err)
		}
		inv.Status = contracts.SupplierInvoiceStatus(status)
		inv.InvoiceDate = invoiceDate.Format(businessDateLayout)
		if dueDate != nil {
			d := dueDate.Format(businessDateLayout)
			inv.DueDate = &d
		}
		inv.CreatedAt = createdAt.UTC().Format(time.RFC3339)
		inv.UpdatedAt = updatedAt.UTC().Format(time.RFC3339)
		inv.SchemaVersion = 1
		out = append(out, inv)
	}
	return out, rows.Err()
}

func (r *pgRepository) InsertSupplierCredit(ctx context.Context, c SupplierCredit) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO supplier_credit (id, tenant_id, outlet_id, supplier_id, purchase_return_id,
		                              credit_note_no, credit_date, amount_paise, created_at, updated_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)`,
		c.ID, c.TenantID, c.OutletID, c.SupplierID, c.PurchaseReturnID,
		c.CreditNoteNo, c.CreditDate, c.AmountPaise, c.CreatedAt, c.UpdatedAt,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: credit_note_no %q already exists for this supplier", httpx.ErrConflict, c.CreditNoteNo)
		}
		return fmt.Errorf("procurement: inserting supplier credit: %w", err)
	}
	return nil
}

func (r *pgRepository) ListSupplierCredits(ctx context.Context, tenantID, outletID string) ([]SupplierCredit, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, tenant_id, outlet_id, supplier_id, purchase_return_id, credit_note_no,
		        credit_date, amount_paise, created_at, updated_at
		 FROM supplier_credit
		 WHERE tenant_id = $1 AND outlet_id = $2 ORDER BY credit_date DESC, id`,
		tenantID, outletID,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing supplier credits: %w", err)
	}
	defer rows.Close()
	out := []SupplierCredit{}
	for rows.Next() {
		var c SupplierCredit
		var creditDate, createdAt, updatedAt time.Time
		if err := rows.Scan(&c.ID, &c.TenantID, &c.OutletID, &c.SupplierID, &c.PurchaseReturnID,
			&c.CreditNoteNo, &creditDate, &c.AmountPaise, &createdAt, &updatedAt); err != nil {
			return nil, fmt.Errorf("procurement: scanning supplier credit: %w", err)
		}
		c.CreditDate = creditDate.Format(businessDateLayout)
		c.CreatedAt = createdAt.UTC().Format(time.RFC3339)
		c.UpdatedAt = updatedAt.UTC().Format(time.RFC3339)
		c.SchemaVersion = 1
		out = append(out, c)
	}
	return out, rows.Err()
}

// --- admin read paths -------------------------------------------------------

// supplierColumnsQualified is the supplier column list carrying the "s."
// alias. It is a SEPARATE constant from supplierSelect for the reason
// purchaseOrderColumnsQualified is separate from purchaseOrderColumns:
// supplier, outlet and brand all have id / created_at / updated_at, so an
// unqualified column in a query that joins them is "column reference is
// ambiguous" (SQLSTATE 42702) at runtime and invisible to every static check.
// That exact bug shipped once in this file.
const supplierColumnsQualified = `s.id, s.outlet_id, s.code, s.name, s.gstin, s.phone, s.email,
	       s.address, s.payment_terms_days, s.is_active, s.config_version, s.created_at, s.updated_at`

func scanSupplier(row rowScanner) (Supplier, bool, error) {
	var s Supplier
	var createdAt, updatedAt time.Time
	err := row.Scan(&s.ID, &s.OutletID, &s.Code, &s.Name, &s.Gstin, &s.Phone, &s.Email, &s.Address,
		&s.PaymentTermsDays, &s.IsActive, &s.ConfigVersion, &createdAt, &updatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return Supplier{}, false, nil
	}
	if err != nil {
		return Supplier{}, false, fmt.Errorf("procurement: scanning supplier: %w", err)
	}
	s.CreatedAt = createdAt.UTC().Format(time.RFC3339)
	s.UpdatedAt = updatedAt.UTC().Format(time.RFC3339)
	s.SchemaVersion = 1
	return s, true, nil
}

// nullableID turns an empty filter value into a real SQL NULL, so ONE
// statement serves both "filtered" and "unfiltered". Building the WHERE clause
// by string concatenation is how a tenant guard eventually gets concatenated
// out of one branch and nobody notices.
func nullableID(v string) *string {
	if v == "" {
		return nil
	}
	return &v
}

func (r *pgRepository) GetSupplier(ctx context.Context, tenantID, supplierID string) (Supplier, bool, error) {
	return scanSupplier(r.pool.QueryRow(ctx,
		`SELECT `+supplierColumnsQualified+`
		 FROM supplier s
		 JOIN outlet o ON o.id = s.outlet_id
		 JOIN brand b ON b.id = o.brand_id
		 WHERE s.id = $1 AND b.tenant_id = $2`,
		supplierID, tenantID,
	))
}

func (r *pgRepository) ListSuppliers(ctx context.Context, tenantID string, filter SupplierFilter) ([]Supplier, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT `+supplierColumnsQualified+`
		 FROM supplier s
		 JOIN outlet o ON o.id = s.outlet_id
		 JOIN brand b ON b.id = o.brand_id
		 WHERE b.tenant_id = $1
		   AND ($2::uuid IS NULL OR s.outlet_id = $2::uuid)
		   AND ($3::boolean OR s.is_active)
		 ORDER BY s.outlet_id, s.code
		 LIMIT $4`,
		tenantID, nullableID(filter.OutletID), filter.IncludeInactive, defaultSupplierListLimit,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing suppliers: %w", err)
	}
	defer rows.Close()
	out := []Supplier{}
	for rows.Next() {
		s, _, err := scanSupplier(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

func (r *pgRepository) SupplierItemsForSuppliers(ctx context.Context, supplierIDs []string) (map[string][]SupplierItem, error) {
	out := map[string][]SupplierItem{}
	if len(supplierIDs) == 0 {
		return out, nil
	}
	rows, err := r.pool.Query(ctx,
		`SELECT si.id, si.supplier_id, si.inventory_item_id, si.purchase_unit, si.pack_size_micro,
		        si.quantity_dimension, si.last_price_paise, si.is_preferred
		 FROM supplier_item si
		 WHERE si.supplier_id = ANY($1::uuid[])
		 ORDER BY si.supplier_id, si.inventory_item_id, si.purchase_unit`,
		supplierIDs,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing supplier items: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var it SupplierItem
		var dim string
		if err := rows.Scan(&it.ID, &it.SupplierID, &it.InventoryItemID, &it.PurchaseUnit, &it.PackSizeMicro,
			&dim, &it.LastPricePaise, &it.IsPreferred); err != nil {
			return nil, fmt.Errorf("procurement: scanning supplier item: %w", err)
		}
		// Read back EXACTLY as stored, never re-derived from inventory_item on
		// the way out: a read that healed a mismatch would hide the very
		// disagreement the write path exists to reject (ADR-019 §6).
		it.QuantityDimension = Dimension(dim)
		it.SchemaVersion = 1
		out[it.SupplierID] = append(out[it.SupplierID], it)
	}
	return out, rows.Err()
}

func (r *pgRepository) ListPurchaseOrders(ctx context.Context, tenantID string, filter PurchaseOrderFilter) ([]PurchaseOrder, error) {
	limit := filter.Limit
	if limit <= 0 {
		limit = defaultPurchaseOrderListLimit
	}
	if limit > maxPurchaseOrderListLimit {
		limit = maxPurchaseOrderListLimit
	}
	var statuses []string
	for _, s := range filter.Statuses {
		statuses = append(statuses, string(s))
	}
	rows, err := r.pool.Query(ctx,
		`SELECT `+purchaseOrderColumnsQualified+`
		 FROM purchase_order po
		 JOIN outlet o ON o.id = po.outlet_id
		 JOIN brand b ON b.id = o.brand_id
		 WHERE b.tenant_id = $1
		   AND ($2::uuid IS NULL OR po.outlet_id = $2::uuid)
		   AND ($3::uuid IS NULL OR po.supplier_id = $3::uuid)
		   AND ($4::text[] IS NULL OR po.status = ANY($4::text[]))
		 ORDER BY po.created_at DESC, po.id
		 LIMIT $5`,
		tenantID, nullableID(filter.OutletID), nullableID(filter.SupplierID), statuses, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing purchase orders: %w", err)
	}
	defer rows.Close()
	out := []PurchaseOrder{}
	for rows.Next() {
		po, _, err := scanPurchaseOrder(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, po)
	}
	return out, rows.Err()
}

func (r *pgRepository) PurchaseOrderLinesForOrders(ctx context.Context, purchaseOrderIDs []string) (map[string][]PurchaseOrderLine, error) {
	out := map[string][]PurchaseOrderLine{}
	if len(purchaseOrderIDs) == 0 {
		return out, nil
	}
	rows, err := r.pool.Query(ctx,
		purchaseOrderLineSelect+` WHERE purchase_order_id = ANY($1::uuid[])
		 ORDER BY purchase_order_id, line_number`,
		purchaseOrderIDs,
	)
	if err != nil {
		return nil, fmt.Errorf("procurement: listing purchase order lines for orders: %w", err)
	}
	lines, err := scanPurchaseOrderLines(rows)
	if err != nil {
		return nil, err
	}
	for _, l := range lines {
		out[l.PurchaseOrderID] = append(out[l.PurchaseOrderID], l)
	}
	return out, nil
}

// AmendPurchaseOrder updates an EXISTING order and NULLS BOTH APPROVAL COLUMNS
// in the same UPDATE. It never inserts: a missing row is httpx.ErrNotFound, not
// a silent create, because "amend" and "raise" are different decisions and an
// amend that quietly created a row would have no raiser recorded anywhere.
//
// Both approval columns go to NULL TOGETHER, which is what keeps
// purchase_order_approval_is_whole satisfied. Nothing in this package clears
// one alone, exactly as nothing sets one alone.
func (r *pgRepository) AmendPurchaseOrder(ctx context.Context, tx pgx.Tx, po PurchaseOrder) error {
	tag, err := tx.Exec(ctx,
		`UPDATE purchase_order
		 SET supplier_id = $1, po_number = $2, status = $3, expected_date = $4, notes = $5,
		     total_paise = $6, approved_by_user_id = NULL, approved_at = NULL,
		     updated_at = $7, config_version = $8
		 WHERE id = $9`,
		po.SupplierID, po.PoNumber, string(po.Status), po.ExpectedDate, po.Notes,
		po.TotalPaise, time.Now().UTC(), po.ConfigVersion, po.ID,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: po_number %q already exists in this outlet", httpx.ErrConflict, po.PoNumber)
		}
		return fmt.Errorf("procurement: amending purchase order: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("%w: purchase order %s", httpx.ErrNotFound, po.ID)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM purchase_order_line WHERE purchase_order_id = $1`, po.ID); err != nil {
		return fmt.Errorf("procurement: clearing purchase order lines: %w", err)
	}
	for _, l := range po.Lines {
		if _, err := tx.Exec(ctx,
			`INSERT INTO purchase_order_line (id, purchase_order_id, inventory_item_id, line_number,
			                                  purchase_unit, ordered_quantity_micro, quantity_dimension,
			                                  unit_price_paise, line_total_paise)
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)`,
			l.ID, po.ID, l.InventoryItemID, l.LineNumber, l.PurchaseUnit, l.OrderedQuantityMicro,
			string(l.QuantityDimension), l.UnitPricePaise, l.LineTotalPaise,
		); err != nil {
			if storage.IsUniqueViolation(err) {
				return fmt.Errorf("%w: purchase order line_number %d is duplicated", httpx.ErrConflict, l.LineNumber)
			}
			return fmt.Errorf("procurement: inserting purchase order line: %w", err)
		}
	}
	return nil
}
