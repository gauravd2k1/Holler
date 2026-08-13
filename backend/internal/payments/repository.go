package payments

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
)

// pgUniqueViolation is the PostgreSQL SQLSTATE for a unique_violation,
// mirroring backend/internal/kitchen and backend/internal/tables's own copy
// of the same constant.
const pgUniqueViolation = "23505"

// idxInvoiceOutletSeriesNumber is the constraint name §33's uniqueness rule
// is enforced by (packages/contracts/postgres/0007_m3_billing.sql). Used to
// distinguish "a different invoice reused this number" (409,
// ErrDuplicateInvoiceNumber) from any other unique_violation.
const idxInvoiceOutletSeriesNumber = "idx_invoice_outlet_series_number"

// businessDateLayout formats a Postgres DATE column back to the outlet-local
// YYYY-MM-DD string contracts.Invoice/CashShift.BusinessDate carries on the
// wire (CLAUDE.md: the business day may cross midnight, so this is never a
// full timestamp).
const businessDateLayout = "2006-01-02"

func isUniqueViolation(err error) (*pgconn.PgError, bool) {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == pgUniqueViolation {
		return pgErr, true
	}
	return nil, false
}

// PostgresRepository is the Repository implementation backed by the
// packages/contracts/postgres schema.
type PostgresRepository struct {
	pool postgres.Pool
}

func NewPostgresRepository(pool postgres.Pool) *PostgresRepository {
	return &PostgresRepository{pool: pool}
}

// --- invoice -----------------------------------------------------------

// InsertInvoice's idempotency is ON CONFLICT (id) DO NOTHING: an identical
// replay of the same record_id is a no-op, mirroring
// backend/internal/ordering.InsertOrder's own documented trade-off. A
// DIFFERENT invoice id reusing an already-issued (outlet_id, series_id,
// invoice_number) triple is not suppressed by that target and surfaces as a
// unique_violation on idxInvoiceOutletSeriesNumber, translated here to
// ErrDuplicateInvoiceNumber rather than a raw driver error.
func (r *PostgresRepository) InsertInvoice(ctx context.Context, tenantID string, inv Invoice) (Invoice, bool, error) {
	taxSnapshot, err := json.Marshal(inv.TaxSnapshot)
	if err != nil {
		return Invoice{}, false, fmt.Errorf("payments: marshalling tax_snapshot: %w", err)
	}
	fiscalProfile, err := json.Marshal(inv.FiscalProfile)
	if err != nil {
		return Invoice{}, false, fmt.Errorf("payments: marshalling fiscal_profile: %w", err)
	}

	tag, err := r.pool.Exec(ctx,
		`INSERT INTO invoice (
			id, outlet_id, order_id, split_group_id, split_index, split_count,
			series_id, invoice_number, invoice_date, business_date,
			status, cancelled_reason, cancelled_at,
			customer_name, customer_phone, customer_gstin, place_of_supply_state_code,
			subtotal_paise, discount_paise, taxable_value_paise, cgst_paise, sgst_paise,
			igst_paise, cess_paise, round_off_paise, grand_total_paise,
			compliance_version_id, tax_snapshot, fiscal_profile,
			channel, tax_liability_party, eco_operator_name, eco_operator_gstin, supply_classification,
			created_by_user_id, created_at, updated_at, version
		 )
		 SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
			$18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, $34,
			$35, $36, $37, $38
		 WHERE EXISTS (SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id WHERE o.id = $2 AND b.tenant_id = $39)
		 ON CONFLICT (id) DO NOTHING`,
		inv.ID, inv.OutletID, inv.OrderID, inv.SplitGroupID, invSplitIndex(inv), invSplitCount(inv),
		inv.SeriesID, inv.InvoiceNumber, inv.InvoiceDate, inv.BusinessDate,
		string(inv.Status), inv.CancelledReason, inv.CancelledAt,
		inv.CustomerName, inv.CustomerPhone, inv.CustomerGSTIN, inv.PlaceOfSupplyStateCode,
		inv.SubtotalPaise, inv.DiscountPaise, inv.TaxableValuePaise, inv.CGSTPaise, inv.SGSTPaise,
		inv.IGSTPaise, inv.CessPaise, inv.RoundOffPaise, inv.GrandTotalPaise,
		inv.ComplianceVersionID, taxSnapshot, fiscalProfile,
		string(inv.Channel), string(inv.TaxLiabilityParty), inv.ECOOperatorName, inv.ECOOperatorGSTIN, inv.SupplyClassification,
		inv.CreatedByUserID, inv.CreatedAt, inv.UpdatedAt, inv.Version,
		tenantID,
	)
	if err != nil {
		if pgErr, ok := isUniqueViolation(err); ok && pgErr.ConstraintName == idxInvoiceOutletSeriesNumber {
			return Invoice{}, false, ErrDuplicateInvoiceNumber
		}
		return Invoice{}, false, fmt.Errorf("payments: inserting invoice: %w", err)
	}

	inserted := tag.RowsAffected() > 0
	if inserted {
		for i := range inv.Lines {
			if err := r.insertInvoiceLine(ctx, inv.ID, inv.Lines[i]); err != nil {
				return Invoice{}, false, err
			}
		}
	}

	stored, getErr := r.GetInvoice(ctx, tenantID, inv.ID)
	if getErr != nil {
		if !inserted {
			return Invoice{}, false, httpx.ErrNotFound
		}
		return Invoice{}, false, getErr
	}
	return stored, inserted, nil
}

func invSplitIndex(inv Invoice) int {
	if inv.SplitIndex < 1 {
		return 1
	}
	return inv.SplitIndex
}

func invSplitCount(inv Invoice) int {
	if inv.SplitCount < 1 {
		return 1
	}
	return inv.SplitCount
}

func (r *PostgresRepository) insertInvoiceLine(ctx context.Context, invoiceID string, l contracts.InvoiceLine) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO invoice_line (
			id, invoice_id, order_item_id, line_no, description, hsn_sac, quantity,
			unit_price_paise, gross_paise, discount_paise, taxable_value_paise, tax_profile_id,
			cgst_rate_bps, cgst_paise, sgst_rate_bps, sgst_paise, igst_rate_bps, igst_paise,
			cess_rate_bps, cess_paise, total_paise
		 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)
		 ON CONFLICT (id) DO NOTHING`,
		l.ID, invoiceID, l.OrderItemID, l.LineNo, l.Description, l.HSNSAC, l.Quantity,
		l.UnitPricePaise, l.GrossPaise, l.DiscountPaise, l.TaxableValuePaise, l.TaxProfileID,
		l.CGSTRateBps, l.CGSTPaise, l.SGSTRateBps, l.SGSTPaise, l.IGSTRateBps, l.IGSTPaise,
		l.CessRateBps, l.CessPaise, l.TotalPaise,
	)
	if err != nil {
		return fmt.Errorf("payments: inserting invoice_line: %w", err)
	}
	return nil
}

func (r *PostgresRepository) GetInvoice(ctx context.Context, tenantID, invoiceID string) (Invoice, error) {
	var inv Invoice
	var status, channel, taxLiability string
	var taxSnapshot, fiscalProfile []byte
	var businessDate time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT i.id, i.outlet_id, i.order_id, i.split_group_id, i.split_index, i.split_count,
			i.series_id, i.invoice_number, i.invoice_date, i.business_date,
			i.status, i.cancelled_reason, i.cancelled_at,
			i.customer_name, i.customer_phone, i.customer_gstin, i.place_of_supply_state_code,
			i.subtotal_paise, i.discount_paise, i.taxable_value_paise, i.cgst_paise, i.sgst_paise,
			i.igst_paise, i.cess_paise, i.round_off_paise, i.grand_total_paise,
			i.compliance_version_id, i.tax_snapshot, i.fiscal_profile,
			i.channel, i.tax_liability_party, i.eco_operator_name, i.eco_operator_gstin, i.supply_classification,
			i.created_by_user_id, i.created_at, i.updated_at, i.version
		 FROM invoice i
		 JOIN outlet o ON o.id = i.outlet_id
		 JOIN brand b ON b.id = o.brand_id
		 WHERE i.id = $1 AND b.tenant_id = $2`,
		invoiceID, tenantID,
	).Scan(
		&inv.ID, &inv.OutletID, &inv.OrderID, &inv.SplitGroupID, &inv.SplitIndex, &inv.SplitCount,
		&inv.SeriesID, &inv.InvoiceNumber, &inv.InvoiceDate, &businessDate,
		&status, &inv.CancelledReason, &inv.CancelledAt,
		&inv.CustomerName, &inv.CustomerPhone, &inv.CustomerGSTIN, &inv.PlaceOfSupplyStateCode,
		&inv.SubtotalPaise, &inv.DiscountPaise, &inv.TaxableValuePaise, &inv.CGSTPaise, &inv.SGSTPaise,
		&inv.IGSTPaise, &inv.CessPaise, &inv.RoundOffPaise, &inv.GrandTotalPaise,
		&inv.ComplianceVersionID, &taxSnapshot, &fiscalProfile,
		&channel, &taxLiability, &inv.ECOOperatorName, &inv.ECOOperatorGSTIN, &inv.SupplyClassification,
		&inv.CreatedByUserID, &inv.CreatedAt, &inv.UpdatedAt, &inv.Version,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return Invoice{}, httpx.ErrNotFound
	}
	if err != nil {
		return Invoice{}, fmt.Errorf("payments: querying invoice: %w", err)
	}
	inv.BusinessDate = businessDate.Format(businessDateLayout)
	inv.Status = contracts.InvoiceStatus(status)
	inv.Channel = contracts.Channel(channel)
	inv.TaxLiabilityParty = contracts.TaxLiabilityParty(taxLiability)
	if len(taxSnapshot) > 0 {
		if err := json.Unmarshal(taxSnapshot, &inv.TaxSnapshot); err != nil {
			return Invoice{}, fmt.Errorf("payments: decoding tax_snapshot: %w", err)
		}
	}
	if len(fiscalProfile) > 0 {
		if err := json.Unmarshal(fiscalProfile, &inv.FiscalProfile); err != nil {
			return Invoice{}, fmt.Errorf("payments: decoding fiscal_profile: %w", err)
		}
	}

	lines, err := r.linesForInvoice(ctx, invoiceID)
	if err != nil {
		return Invoice{}, err
	}
	inv.Lines = lines
	return inv, nil
}

func (r *PostgresRepository) linesForInvoice(ctx context.Context, invoiceID string) ([]contracts.InvoiceLine, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, invoice_id, order_item_id, line_no, description, hsn_sac, quantity,
			unit_price_paise, gross_paise, discount_paise, taxable_value_paise, tax_profile_id,
			cgst_rate_bps, cgst_paise, sgst_rate_bps, sgst_paise, igst_rate_bps, igst_paise,
			cess_rate_bps, cess_paise, total_paise
		 FROM invoice_line WHERE invoice_id = $1 ORDER BY line_no`,
		invoiceID,
	)
	if err != nil {
		return nil, fmt.Errorf("payments: listing invoice_line: %w", err)
	}
	defer rows.Close()

	lines := make([]contracts.InvoiceLine, 0)
	for rows.Next() {
		var l contracts.InvoiceLine
		if err := rows.Scan(
			&l.ID, &l.InvoiceID, &l.OrderItemID, &l.LineNo, &l.Description, &l.HSNSAC, &l.Quantity,
			&l.UnitPricePaise, &l.GrossPaise, &l.DiscountPaise, &l.TaxableValuePaise, &l.TaxProfileID,
			&l.CGSTRateBps, &l.CGSTPaise, &l.SGSTRateBps, &l.SGSTPaise, &l.IGSTRateBps, &l.IGSTPaise,
			&l.CessRateBps, &l.CessPaise, &l.TotalPaise,
		); err != nil {
			return nil, fmt.Errorf("payments: scanning invoice_line: %w", err)
		}
		l.SchemaVersion = 1
		lines = append(lines, l)
	}
	return lines, rows.Err()
}

// --- payment -------------------------------------------------------------

func (r *PostgresRepository) InsertPayment(ctx context.Context, tenantID string, p Payment) (Payment, bool, error) {
	tag, err := r.pool.Exec(ctx,
		`INSERT INTO payment (
			id, outlet_id, order_id, cash_shift_id, method, status, amount_paise,
			tendered_paise, change_paise, reference, external_id, reverses_payment_id,
			captured_at, created_by_user_id, created_at, updated_at, version
		 )
		 SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17
		 WHERE EXISTS (SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id WHERE o.id = $2 AND b.tenant_id = $18)
		 ON CONFLICT (id) DO NOTHING`,
		p.ID, p.OutletID, p.OrderID, p.CashShiftID, string(p.Method), string(p.Status), p.AmountPaise,
		p.TenderedPaise, p.ChangePaise, p.Reference, p.ExternalID, p.ReversesPaymentID,
		p.CapturedAt, p.CreatedByUserID, p.CreatedAt, p.UpdatedAt, p.Version,
		tenantID,
	)
	if err != nil {
		return Payment{}, false, fmt.Errorf("payments: inserting payment: %w", err)
	}
	inserted := tag.RowsAffected() > 0
	if inserted {
		for i := range p.Allocations {
			if err := r.insertAllocation(ctx, p.ID, p.Allocations[i]); err != nil {
				return Payment{}, false, err
			}
		}
	}

	stored, getErr := r.GetPayment(ctx, tenantID, p.ID)
	if getErr != nil {
		if !inserted {
			return Payment{}, false, httpx.ErrNotFound
		}
		return Payment{}, false, getErr
	}
	return stored, inserted, nil
}

func (r *PostgresRepository) insertAllocation(ctx context.Context, paymentID string, a contracts.PaymentAllocation) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO payment_allocation (id, payment_id, invoice_id, amount_paise)
		 VALUES ($1,$2,$3,$4) ON CONFLICT (id) DO NOTHING`,
		a.ID, paymentID, a.InvoiceID, a.AmountPaise,
	)
	if err != nil {
		return fmt.Errorf("payments: inserting payment_allocation: %w", err)
	}
	return nil
}

func (r *PostgresRepository) GetPayment(ctx context.Context, tenantID, paymentID string) (Payment, error) {
	var p Payment
	var method, status string
	err := r.pool.QueryRow(ctx,
		`SELECT p.id, p.outlet_id, p.order_id, p.cash_shift_id, p.method, p.status, p.amount_paise,
			p.tendered_paise, p.change_paise, p.reference, p.external_id, p.reverses_payment_id,
			p.captured_at, p.created_by_user_id, p.created_at, p.updated_at, p.version
		 FROM payment p
		 JOIN outlet o ON o.id = p.outlet_id
		 JOIN brand b ON b.id = o.brand_id
		 WHERE p.id = $1 AND b.tenant_id = $2`,
		paymentID, tenantID,
	).Scan(
		&p.ID, &p.OutletID, &p.OrderID, &p.CashShiftID, &method, &status, &p.AmountPaise,
		&p.TenderedPaise, &p.ChangePaise, &p.Reference, &p.ExternalID, &p.ReversesPaymentID,
		&p.CapturedAt, &p.CreatedByUserID, &p.CreatedAt, &p.UpdatedAt, &p.Version,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return Payment{}, httpx.ErrNotFound
	}
	if err != nil {
		return Payment{}, fmt.Errorf("payments: querying payment: %w", err)
	}
	p.Method = contracts.PaymentMethod(method)
	p.Status = contracts.PaymentCaptureStatus(status)

	rows, err := r.pool.Query(ctx,
		`SELECT id, payment_id, invoice_id, amount_paise FROM payment_allocation WHERE payment_id = $1`,
		paymentID,
	)
	if err != nil {
		return Payment{}, fmt.Errorf("payments: listing payment_allocation: %w", err)
	}
	defer rows.Close()
	allocations := make([]contracts.PaymentAllocation, 0)
	for rows.Next() {
		var a contracts.PaymentAllocation
		if err := rows.Scan(&a.ID, &a.PaymentID, &a.InvoiceID, &a.AmountPaise); err != nil {
			return Payment{}, fmt.Errorf("payments: scanning payment_allocation: %w", err)
		}
		a.SchemaVersion = 1
		allocations = append(allocations, a)
	}
	p.Allocations = allocations
	return p, rows.Err()
}

// --- cash_shift ------------------------------------------------------------

func (r *PostgresRepository) InsertCashShift(ctx context.Context, tenantID string, s CashShift) (CashShift, bool, error) {
	tag, err := r.pool.Exec(ctx,
		`INSERT INTO cash_shift (
			id, outlet_id, device_id, cashier_user_id, status, opened_at, opening_cash_paise,
			closed_at, expected_cash_paise, actual_cash_paise, variance_paise, variance_reason,
			business_date, created_at, updated_at, version
		 )
		 SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16
		 WHERE EXISTS (SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id WHERE o.id = $2 AND b.tenant_id = $17)
		 ON CONFLICT (id) DO NOTHING`,
		s.ID, s.OutletID, s.DeviceID, s.CashierUserID, string(s.Status), s.OpenedAt, s.OpeningCashPaise,
		s.ClosedAt, s.ExpectedCashPaise, s.ActualCashPaise, s.VariancePaise, s.VarianceReason,
		s.BusinessDate, s.CreatedAt, s.UpdatedAt, s.Version,
		tenantID,
	)
	if err != nil {
		return CashShift{}, false, fmt.Errorf("payments: inserting cash_shift: %w", err)
	}
	inserted := tag.RowsAffected() > 0
	if inserted {
		for i := range s.Movements {
			if err := r.insertMovement(ctx, s.ID, s.Movements[i]); err != nil {
				return CashShift{}, false, err
			}
		}
	}

	stored, getErr := r.GetCashShift(ctx, tenantID, s.ID)
	if getErr != nil {
		if !inserted {
			return CashShift{}, false, httpx.ErrNotFound
		}
		return CashShift{}, false, getErr
	}
	return stored, inserted, nil
}

// CloseCashShift is UPDATE-only: it is the sole path that ever moves
// cash_shift.status to CLOSED, mirroring backend/internal/kitchen's
// UpdateKotStatus (§50.1/ADR-014 applied to money). It is idempotent on
// version: a shift already at or past newVersion is a no-op returning the
// current row, exactly like backend/internal/ordering.UpdateStatus.
func (r *PostgresRepository) CloseCashShift(ctx context.Context, tenantID string, s CashShift) (CashShift, bool, error) {
	current, err := r.GetCashShift(ctx, tenantID, s.ID)
	if err != nil {
		return CashShift{}, false, err
	}
	if s.Version <= current.Version {
		return current, false, nil
	}

	tag, err := r.pool.Exec(ctx,
		`UPDATE cash_shift cs SET status = $1, closed_at = $2, expected_cash_paise = $3,
			actual_cash_paise = $4, variance_paise = $5, variance_reason = $6,
			version = $7, updated_at = now()
		 FROM outlet o, brand b
		 WHERE cs.id = $8 AND o.id = cs.outlet_id AND b.id = o.brand_id AND b.tenant_id = $9`,
		string(s.Status), s.ClosedAt, s.ExpectedCashPaise, s.ActualCashPaise, s.VariancePaise, s.VarianceReason,
		s.Version, s.ID, tenantID,
	)
	if err != nil {
		return CashShift{}, false, fmt.Errorf("payments: closing cash_shift: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return CashShift{}, false, httpx.ErrNotFound
	}
	for i := range s.Movements {
		if err := r.insertMovement(ctx, s.ID, s.Movements[i]); err != nil {
			return CashShift{}, false, err
		}
	}

	stored, err := r.GetCashShift(ctx, tenantID, s.ID)
	if err != nil {
		return CashShift{}, false, err
	}
	return stored, true, nil
}

func (r *PostgresRepository) insertMovement(ctx context.Context, shiftID string, m contracts.CashMovement) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO cash_movement (id, cash_shift_id, kind, amount_paise, reason, payment_id, created_by_user_id, created_at)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (id) DO NOTHING`,
		m.ID, shiftID, string(m.Kind), m.AmountPaise, m.Reason, m.PaymentID, m.CreatedByUserID, m.CreatedAt,
	)
	if err != nil {
		return fmt.Errorf("payments: inserting cash_movement: %w", err)
	}
	return nil
}

func (r *PostgresRepository) GetCashShift(ctx context.Context, tenantID, shiftID string) (CashShift, error) {
	var s CashShift
	var status string
	var businessDate time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT cs.id, cs.outlet_id, cs.device_id, cs.cashier_user_id, cs.status, cs.opened_at, cs.opening_cash_paise,
			cs.closed_at, cs.expected_cash_paise, cs.actual_cash_paise, cs.variance_paise, cs.variance_reason,
			cs.business_date, cs.created_at, cs.updated_at, cs.version
		 FROM cash_shift cs
		 JOIN outlet o ON o.id = cs.outlet_id
		 JOIN brand b ON b.id = o.brand_id
		 WHERE cs.id = $1 AND b.tenant_id = $2`,
		shiftID, tenantID,
	).Scan(
		&s.ID, &s.OutletID, &s.DeviceID, &s.CashierUserID, &status, &s.OpenedAt, &s.OpeningCashPaise,
		&s.ClosedAt, &s.ExpectedCashPaise, &s.ActualCashPaise, &s.VariancePaise, &s.VarianceReason,
		&businessDate, &s.CreatedAt, &s.UpdatedAt, &s.Version,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return CashShift{}, httpx.ErrNotFound
	}
	if err != nil {
		return CashShift{}, fmt.Errorf("payments: querying cash_shift: %w", err)
	}
	s.Status = contracts.CashShiftStatus(status)
	s.BusinessDate = businessDate.Format(businessDateLayout)

	rows, err := r.pool.Query(ctx,
		`SELECT id, cash_shift_id, kind, amount_paise, reason, payment_id, created_by_user_id, created_at
		 FROM cash_movement WHERE cash_shift_id = $1 ORDER BY created_at`,
		shiftID,
	)
	if err != nil {
		return CashShift{}, fmt.Errorf("payments: listing cash_movement: %w", err)
	}
	defer rows.Close()
	movements := make([]contracts.CashMovement, 0)
	for rows.Next() {
		var m contracts.CashMovement
		var kind string
		if err := rows.Scan(&m.ID, &m.CashShiftID, &kind, &m.AmountPaise, &m.Reason, &m.PaymentID, &m.CreatedByUserID, &m.CreatedAt); err != nil {
			return CashShift{}, fmt.Errorf("payments: scanning cash_movement: %w", err)
		}
		m.Kind = contracts.CashMovementKind(kind)
		m.SchemaVersion = 1
		movements = append(movements, m)
	}
	s.Movements = movements
	return s, rows.Err()
}
