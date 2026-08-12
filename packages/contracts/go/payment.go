// Payments, payment allocation and the cash shift — added at 0.4.0 (ADR-016,
// Milestone 3). Mirrors src/types/payment.ts.
//
// All EDGE-AUTHORITATIVE (§50.1). AggregateTypePayment has been in
// AggregateAuthority as EDGE_TO_CLOUD since Milestone 0.5 with no payload
// behind it; 0.4.0 fills the shape in. That is a fill-in, not a new authority
// claim — the direction was decided when the map was written (ADR-016 §Payment).
//
// §34: never `order.payment_method = "UPI"`. A ₹2,000 bill settled as ₹500
// cash + ₹1,000 UPI + ₹500 card is three Payments, not one field.
//
// APPEND-ONLY (docs/spec/payments.md §Conflict policy, §53). Nothing mutates a
// captured payment: a void or refund appends a reversal row pointing at the
// original through ReversesPaymentID. Financial records are never
// last-write-wins.
package contracts

import "time"

type PaymentMethod string

const (
	PaymentMethodCash           PaymentMethod = "CASH"
	PaymentMethodUPI            PaymentMethod = "UPI"
	PaymentMethodCreditCard     PaymentMethod = "CREDIT_CARD"
	PaymentMethodDebitCard      PaymentMethod = "DEBIT_CARD"
	PaymentMethodWallet         PaymentMethod = "WALLET"
	PaymentMethodGiftCard       PaymentMethod = "GIFT_CARD"
	PaymentMethodLoyaltyPoints  PaymentMethod = "LOYALTY_POINTS"
	PaymentMethodBankTransfer   PaymentMethod = "BANK_TRANSFER"
	PaymentMethodAggregatorPaid PaymentMethod = "AGGREGATOR_PAID"
	PaymentMethodHouseAccount   PaymentMethod = "HOUSE_ACCOUNT"
	PaymentMethodCredit         PaymentMethod = "CREDIT"
)

// PaymentCaptureStatus is the lifecycle of ONE TENDER's capture attempt. It is
// deliberately not CanonicalOrder.PaymentStatus, which is the order's overall
// standing (UNPAID / PARTIALLY_PAID / PAID / REFUNDED). A ₹2,000 order can sit
// at PARTIALLY_PAID while one of its three tenders is CAPTURED, another FAILED
// and a third PENDING — collapsing the two would lose exactly that
// distinction, which is the whole point of §34's separate Payment entity.
//
// Milestone 3 delivers CASH and split tenders only; gateway capture lands in
// Milestone 7 (§81 EXCLUDES online payment gateways). The states are modelled
// now so a Razorpay attempt has somewhere to go without a contract change.
type PaymentCaptureStatus string

const (
	PaymentCaptureStatusPending  PaymentCaptureStatus = "PENDING"
	PaymentCaptureStatusCaptured PaymentCaptureStatus = "CAPTURED"
	PaymentCaptureStatusFailed   PaymentCaptureStatus = "FAILED"
	PaymentCaptureStatusVoided   PaymentCaptureStatus = "VOIDED"
	PaymentCaptureStatusRefunded PaymentCaptureStatus = "REFUNDED"
)

// PaymentAllocation records how one tender settles against one or more
// invoices. This is what lets split payment and split bill compose: one card
// swipe can settle two parts of a split group, and one part can be settled by
// three tenders.
type PaymentAllocation struct {
	ID            string `json:"id"`
	PaymentID     string `json:"payment_id"`
	InvoiceID     string `json:"invoice_id"`
	AmountPaise   int    `json:"amount_paise"`
	SchemaVersion int    `json:"schema_version"`
}

type Payment struct {
	ID          string  `json:"id"`
	OutletID    string  `json:"outlet_id"`
	OrderID     string  `json:"order_id"`
	CashShiftID *string `json:"cash_shift_id"`

	Method PaymentMethod        `json:"method"`
	Status PaymentCaptureStatus `json:"status"`
	// Negative on a reversal row.
	AmountPaise int `json:"amount_paise"`
	// Cash only — what the customer handed over, and the change given back.
	// Set on a non-cash tender these would corrupt the expected-cash
	// derivation for the whole shift, so a CHECK in both stores forbids it.
	TenderedPaise *int `json:"tendered_paise"`
	ChangePaise   *int `json:"change_paise"`

	Reference         *string    `json:"reference"`   // UTR / auth code / manual card slip number
	ExternalID        *string    `json:"external_id"` // gateway id; Milestone 7
	ReversesPaymentID *string    `json:"reverses_payment_id"`
	CapturedAt        *time.Time `json:"captured_at"`

	Allocations []PaymentAllocation `json:"allocations"`

	CreatedByUserID string    `json:"created_by_user_id"`
	CreatedAt       time.Time `json:"created_at"`
	UpdatedAt       time.Time `json:"updated_at"`
	Version         int       `json:"version"`
	SchemaVersion   int       `json:"schema_version"`
}

type CashShiftStatus string

const (
	CashShiftStatusOpen   CashShiftStatus = "OPEN"
	CashShiftStatusClosed CashShiftStatus = "CLOSED"
)

type CashMovementKind string

const (
	CashMovementOpeningFloat CashMovementKind = "OPENING_FLOAT"
	CashMovementCashSale     CashMovementKind = "CASH_SALE"
	CashMovementCashRefund   CashMovementKind = "CASH_REFUND"
	CashMovementPaidIn       CashMovementKind = "PAID_IN"
	CashMovementPaidOut      CashMovementKind = "PAID_OUT"
)

// CashMovement is every movement of physical cash through the drawer (§39).
// A child row inside the shift's payload. Append-only: a correction is another
// movement, never an edit.
type CashMovement struct {
	ID          string           `json:"id"`
	CashShiftID string           `json:"cash_shift_id"`
	Kind        CashMovementKind `json:"kind"`
	// Signed: PAID_OUT and CASH_REFUND are negative.
	AmountPaise     int       `json:"amount_paise"`
	Reason          *string   `json:"reason"`
	PaymentID       *string   `json:"payment_id"`
	CreatedByUserID string    `json:"created_by_user_id"`
	CreatedAt       time.Time `json:"created_at"`
	SchemaVersion   int       `json:"schema_version"`
}

// CashShift is a cashier-specific register (§39). Expected cash is derived
// from movements; actual is counted by a human; variance is the difference and
// needs a reason.
type CashShift struct {
	ID            string          `json:"id"`
	OutletID      string          `json:"outlet_id"`
	DeviceID      string          `json:"device_id"`
	CashierUserID string          `json:"cashier_user_id"`
	Status        CashShiftStatus `json:"status"`

	OpenedAt         time.Time `json:"opened_at"`
	OpeningCashPaise int       `json:"opening_cash_paise"`

	ClosedAt          *time.Time `json:"closed_at"`
	ExpectedCashPaise *int       `json:"expected_cash_paise"`
	ActualCashPaise   *int       `json:"actual_cash_paise"`
	VariancePaise     *int       `json:"variance_paise"`
	VarianceReason    *string    `json:"variance_reason"`

	// Outlet-local YYYY-MM-DD; the business day may cross midnight.
	BusinessDate string `json:"business_date"`

	Movements []CashMovement `json:"movements"`

	CreatedAt     time.Time `json:"created_at"`
	UpdatedAt     time.Time `json:"updated_at"`
	Version       int       `json:"version"`
	SchemaVersion int       `json:"schema_version"`
}

// IsFullyAccounted reports whether a CLOSED shift carries everything §39
// requires. A register closed without its count can never be reconciled
// afterwards, so an ingest handler rejects it rather than storing it.
func (s CashShift) IsFullyAccounted() bool {
	if s.Status == CashShiftStatusOpen {
		return true
	}
	if s.ClosedAt == nil || s.ExpectedCashPaise == nil || s.ActualCashPaise == nil || s.VariancePaise == nil {
		return false
	}
	// §39 requires a reason for a variance.
	if *s.VariancePaise != 0 && (s.VarianceReason == nil || *s.VarianceReason == "") {
		return false
	}
	return true
}
