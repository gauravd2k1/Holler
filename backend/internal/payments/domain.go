// Package payments ingests the three Milestone 3 edge-authoritative billing
// aggregates — invoice, payment, cash_shift (ADR-016 §1) — via the same
// envelope-wrapped replay pattern backend/internal/ordering and
// backend/internal/kitchen already use (ADR-012). This package NEVER mints
// an invoice number, transitions an invoice, or captures a payment: the rule
// ADR-014 set for kot.status, applied to money. It only replays what the
// edge already decided.
package payments

import (
	"context"
	"errors"
	"fmt"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/platform/httpx"
)

// Invoice, Payment and CashShift are the wire shapes ingested from the edge;
// they mirror the contracts types exactly (CLAUDE.md: import contract types,
// never hand-roll mirrors).
type Invoice = contracts.Invoice
type Payment = contracts.Payment
type CashShift = contracts.CashShift

// ErrAuthorityViolation marks an envelope whose aggregate_type or direction
// does not match the route it arrived on (§50.1) — mapped to 422
// EnvelopeRouteMismatch by the HTTP layer, mirroring ordering/kitchen.
var ErrAuthorityViolation = errors.New("payments: envelope authority violation")

// ErrRoundingViolation marks an invoice that fails contracts.Invoice.
// SumsCorrectly (ADR-016 §3) — mapped to 422 naming the rule, never a raw
// driver constraint error.
var ErrRoundingViolation = errors.New("payments: invoice does not satisfy the ADR-016 rounding policy")

// ErrShiftNotAccounted marks a CLOSED cash_shift replay that fails
// contracts.CashShift.IsFullyAccounted (§39) — mapped to 422.
var ErrShiftNotAccounted = errors.New("payments: cash shift is not fully accounted for")

// Repository is the persistence boundary Service depends on. Every method
// takes tenantID as an explicit, mandatory parameter and every
// implementation must use it in the query itself — never as a post-hoc check
// on the loaded row — mirroring backend/internal/ordering's rule.
type Repository interface {
	// InsertInvoice is idempotent on id (ON CONFLICT DO NOTHING): a
	// duplicate replay of the same record_id is a no-op. A genuinely
	// different invoice reusing an already-issued (outlet_id, series_id,
	// invoice_number) triple returns ErrDuplicateInvoiceNumber rather than
	// a raw constraint error (§33: numbers are never generated twice).
	InsertInvoice(ctx context.Context, tenantID string, inv Invoice) (stored Invoice, inserted bool, err error)
	GetInvoice(ctx context.Context, tenantID, invoiceID string) (Invoice, error)

	// InsertPayment is idempotent on id. Payments are APPEND-ONLY (§53): this
	// is the only write path, there is no update.
	InsertPayment(ctx context.Context, tenantID string, p Payment) (stored Payment, inserted bool, err error)
	GetPayment(ctx context.Context, tenantID, paymentID string) (Payment, error)

	// InsertCashShift is idempotent on id — the "shift opened" replay.
	InsertCashShift(ctx context.Context, tenantID string, s CashShift) (stored CashShift, inserted bool, err error)
	// CloseCashShift applies the "shift closed" replay to an existing shift
	// row: it is the only path that ever moves status to CLOSED. Idempotent
	// on version — a duplicate close replay returns the row unchanged.
	CloseCashShift(ctx context.Context, tenantID string, s CashShift) (stored CashShift, applied bool, err error)
	GetCashShift(ctx context.Context, tenantID, shiftID string) (CashShift, error)
}

// ErrDuplicateInvoiceNumber is returned by Repository.InsertInvoice when a
// DIFFERENT invoice id reuses an already-issued (outlet_id, series_id,
// invoice_number) triple. Mapped to httpx.ErrConflict (409) — uniqueness is
// scoped to the outlet+series, never global (ADR-016 §Contract review
// rubric).
var ErrDuplicateInvoiceNumber = fmt.Errorf("%w: an invoice with this number already exists for this outlet and series", httpx.ErrConflict)
