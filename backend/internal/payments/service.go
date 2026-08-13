package payments

import (
	"context"
	"fmt"
	"strings"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/platform/httpx"
)

// Service implements the payments ingest commands. Per §50.1 the edge is the
// authority for invoice, payment and cash_shift — every method here takes a
// contracts.SyncEnvelope and replays it. It never originates state, never
// computes tax, never mints an invoice number and never captures a payment
// (ADR-016).
type Service struct {
	repo Repository
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo}
}

// requireEnvelope checks the two protocol-level facts every ingest route
// must pin: the envelope's aggregate_type matches the route, and its
// direction matches §50.1's authority for that aggregate. Either mismatch is
// ErrAuthorityViolation, mapped to 422 by the HTTP layer — never coerced.
func requireEnvelope(env contracts.SyncEnvelope, expectAggregate contracts.AggregateType) error {
	if env.AggregateType != expectAggregate {
		return fmt.Errorf("%w: expected aggregate_type %q, got %q", ErrAuthorityViolation, expectAggregate, env.AggregateType)
	}
	requiredDirection, known := contracts.AggregateAuthority[expectAggregate]
	if !known {
		// contracts.AggregateAuthority is asserted total by a contract drift
		// test; this branch exists only to fail closed if that ever regresses.
		return fmt.Errorf("%w: %s has no configured sync direction", ErrAuthorityViolation, expectAggregate)
	}
	if env.Direction != requiredDirection {
		return fmt.Errorf("%w: expected direction %q for %s, got %q", ErrAuthorityViolation, requiredDirection, expectAggregate, env.Direction)
	}
	if strings.TrimSpace(env.RecordID) == "" {
		return fmt.Errorf("%w: record_id is required", httpx.ErrInvalidInput)
	}
	if env.Version < 1 {
		return fmt.Errorf("%w: version must be >= 1", httpx.ErrInvalidInput)
	}
	return nil
}

// requireCallerMatch guards against a caller replaying an envelope for a
// tenant/outlet other than the one their device credential resolves to
// (ADR-017 0.4.3 amendment): callerTenantID/callerOutletID come from the
// verified device_credential row, never from anything the caller supplied,
// and the envelope's own claims are checked against them rather than
// trusted.
func requireCallerMatch(callerTenantID, callerOutletID string, env contracts.SyncEnvelope) error {
	if callerTenantID == "" || callerOutletID == "" {
		return httpx.ErrUnauthorized
	}
	if env.TenantID != "" && env.TenantID != callerTenantID {
		return httpx.ErrForbidden
	}
	if env.OutletID != "" && env.OutletID != callerOutletID {
		return httpx.ErrForbidden
	}
	return nil
}

// IngestInvoice replays a GST invoice issued at the edge. Idempotent on
// record_id (§25): replaying the identical envelope twice creates exactly
// one row. A payload violating the ADR-016 rounding policy is rejected with
// ErrRoundingViolation rather than stored, and a genuinely different invoice
// reusing an already-issued (outlet, series, number) is rejected with
// ErrDuplicateInvoiceNumber (409) — uniqueness is scoped, never global.
func (s *Service) IngestInvoice(ctx context.Context, callerTenantID, callerOutletID string, env contracts.SyncEnvelope, inv Invoice) (Invoice, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeInvoice); err != nil {
		return Invoice{}, err
	}
	if err := requireCallerMatch(callerTenantID, callerOutletID, env); err != nil {
		return Invoice{}, err
	}
	if strings.TrimSpace(inv.ID) == "" {
		inv.ID = env.RecordID
	}
	if inv.ID != env.RecordID {
		return Invoice{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if inv.OutletID != callerOutletID {
		return Invoice{}, fmt.Errorf("%w: invoice outlet_id must match the authenticated device's outlet", httpx.ErrForbidden)
	}
	if strings.TrimSpace(inv.OrderID) == "" {
		return Invoice{}, fmt.Errorf("%w: order_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(inv.SeriesID) == "" || strings.TrimSpace(inv.InvoiceNumber) == "" {
		return Invoice{}, fmt.Errorf("%w: series_id and invoice_number are required", httpx.ErrInvalidInput)
	}
	if !inv.SumsCorrectly() {
		return Invoice{}, fmt.Errorf("%w", ErrRoundingViolation)
	}

	stored, _, err := s.repo.InsertInvoice(ctx, callerTenantID, inv)
	if err != nil {
		return Invoice{}, err
	}
	return stored, nil
}

func (s *Service) GetInvoice(ctx context.Context, callerTenantID, invoiceID string) (Invoice, error) {
	if callerTenantID == "" {
		return Invoice{}, httpx.ErrUnauthorized
	}
	return s.repo.GetInvoice(ctx, callerTenantID, invoiceID)
}

// IngestPayment replays one tender (PaymentReceived / PaymentRefunded).
// APPEND-ONLY (§53): a void or refund arrives as a NEW payment carrying
// reverses_payment_id and a non-positive amount — this method never updates
// an existing payment row.
func (s *Service) IngestPayment(ctx context.Context, callerTenantID, callerOutletID string, env contracts.SyncEnvelope, p Payment) (Payment, error) {
	if err := requireEnvelope(env, contracts.AggregateTypePayment); err != nil {
		return Payment{}, err
	}
	if err := requireCallerMatch(callerTenantID, callerOutletID, env); err != nil {
		return Payment{}, err
	}
	if strings.TrimSpace(p.ID) == "" {
		p.ID = env.RecordID
	}
	if p.ID != env.RecordID {
		return Payment{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if p.OutletID != callerOutletID {
		return Payment{}, fmt.Errorf("%w: payment outlet_id must match the authenticated device's outlet", httpx.ErrForbidden)
	}
	if strings.TrimSpace(p.OrderID) == "" {
		return Payment{}, fmt.Errorf("%w: order_id is required", httpx.ErrInvalidInput)
	}
	if p.ReversesPaymentID != nil && p.AmountPaise > 0 {
		return Payment{}, fmt.Errorf("%w: a reversal payment must carry a non-positive amount_paise", httpx.ErrInvalidInput)
	}
	if p.TenderedPaise != nil && p.Method != contracts.PaymentMethodCash {
		return Payment{}, fmt.Errorf("%w: tendered_paise may only be set on a CASH tender", httpx.ErrInvalidInput)
	}

	stored, _, err := s.repo.InsertPayment(ctx, callerTenantID, p)
	if err != nil {
		return Payment{}, err
	}
	return stored, nil
}

func (s *Service) GetPayment(ctx context.Context, callerTenantID, paymentID string) (Payment, error) {
	if callerTenantID == "" {
		return Payment{}, httpx.ErrUnauthorized
	}
	return s.repo.GetPayment(ctx, callerTenantID, paymentID)
}

// IngestCashShift replays a shift opened at the edge (CashShiftOpened).
// Idempotent on record_id.
func (s *Service) IngestCashShift(ctx context.Context, callerTenantID, callerOutletID string, env contracts.SyncEnvelope, shift CashShift) (CashShift, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeCashShift); err != nil {
		return CashShift{}, err
	}
	if err := requireCallerMatch(callerTenantID, callerOutletID, env); err != nil {
		return CashShift{}, err
	}
	if strings.TrimSpace(shift.ID) == "" {
		shift.ID = env.RecordID
	}
	if shift.ID != env.RecordID {
		return CashShift{}, fmt.Errorf("%w: payload id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if shift.OutletID != callerOutletID {
		return CashShift{}, fmt.Errorf("%w: cash_shift outlet_id must match the authenticated device's outlet", httpx.ErrForbidden)
	}
	if shift.Status != contracts.CashShiftStatusOpen {
		return CashShift{}, fmt.Errorf("%w: POST /cash-shifts only replays a shift's OPEN state; use the close route to replay CLOSED", httpx.ErrInvalidInput)
	}

	stored, _, err := s.repo.InsertCashShift(ctx, callerTenantID, shift)
	if err != nil {
		return CashShift{}, err
	}
	return stored, nil
}

// CloseCashShift replays a shift close recorded at the edge
// (CashShiftClosed). Rejects with ErrShiftNotAccounted a CLOSED replay
// missing its count, or a non-zero variance with no reason (§39) — using
// contracts.CashShift.IsFullyAccounted rather than reimplementing the rule.
func (s *Service) CloseCashShift(ctx context.Context, callerTenantID, callerOutletID string, env contracts.SyncEnvelope, shiftID string, shift CashShift) (CashShift, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeCashShift); err != nil {
		return CashShift{}, err
	}
	if err := requireCallerMatch(callerTenantID, callerOutletID, env); err != nil {
		return CashShift{}, err
	}
	shiftID = strings.TrimSpace(shiftID)
	if shiftID == "" {
		return CashShift{}, fmt.Errorf("%w: shift id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(shift.ID) == "" {
		shift.ID = shiftID
	}
	if shift.ID != shiftID {
		return CashShift{}, fmt.Errorf("%w: payload id must match the route shift id", httpx.ErrInvalidInput)
	}
	if env.RecordID != "" && env.RecordID != shiftID {
		return CashShift{}, fmt.Errorf("%w: envelope record_id must match the route shift id", httpx.ErrInvalidInput)
	}
	if shift.OutletID != callerOutletID {
		return CashShift{}, fmt.Errorf("%w: cash_shift outlet_id must match the authenticated device's outlet", httpx.ErrForbidden)
	}
	if shift.Status != contracts.CashShiftStatusClosed {
		return CashShift{}, fmt.Errorf("%w: the close route only replays a shift's CLOSED state", httpx.ErrInvalidInput)
	}
	if !shift.IsFullyAccounted() {
		return CashShift{}, fmt.Errorf("%w", ErrShiftNotAccounted)
	}

	stored, _, err := s.repo.CloseCashShift(ctx, callerTenantID, shift)
	if err != nil {
		return CashShift{}, err
	}
	return stored, nil
}

func (s *Service) GetCashShift(ctx context.Context, callerTenantID, shiftID string) (CashShift, error) {
	if callerTenantID == "" {
		return CashShift{}, httpx.ErrUnauthorized
	}
	return s.repo.GetCashShift(ctx, callerTenantID, shiftID)
}
