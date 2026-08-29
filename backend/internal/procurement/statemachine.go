package procurement

import (
	"fmt"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// ErrAuthorityViolation mirrors backend/internal/inventory and
// backend/internal/ordering's sentinel exactly: returned when a sync
// envelope's aggregate_type/direction contradicts the route it arrived on, or
// contracts.AggregateAuthority for that aggregate type. The HTTP layer maps it
// to 422 EnvelopeRouteMismatch per §50.1 — a protocol violation, never a
// coercion.
var ErrAuthorityViolation = fmt.Errorf("%w: sync envelope direction violates aggregate authority", httpx.ErrInvalidInput)

// ErrDimensionMismatch is returned when an author-chosen quantity_dimension
// disagrees with the referenced inventory_item's own dimension (ADR-019 §6,
// contracts 0.5.2's rule on four more tables). 422, never coerced: there is
// nothing to convert through, and reclassifying an item is a migration rather
// than an edit.
//
// THE CLOUD IS THE SIDE THAT REJECTS. The edge cannot — it degrades to a
// DIMENSION_MISMATCH grn_gap and still accepts the receipt, because refusing a
// delivery standing in the doorway is the outage.
var ErrDimensionMismatch = fmt.Errorf("%w: quantity_dimension does not match the referenced inventory_item's dimension", httpx.ErrInvalidInput)

// ErrPurchaseOrderNotApprovable is returned when an approve call targets an
// order in a status that cannot be approved (already APPROVED, CANCELLED,
// CLOSED, SENT).
var ErrPurchaseOrderNotApprovable = fmt.Errorf("%w: purchase order is not in an approvable status", httpx.ErrConflict)

// ApprovalRefusal is the §64 error the approve route returns when either of
// the TWO approval gates refuses: the caller's roles do not carry
// procurement.approve, or role.po_approval_limit_paise is NULL/below the
// order's total.
//
// IT CARRIES THE NUMBERS AND A NEXT ACTION, DELIBERATELY. A bare "Forbidden"
// leaves a buyer with a delivery due and nothing to act on; the message names
// the order total, the caller's ceiling (or that they have none) and which
// roles can approve it instead. Acceptance criterion 5 is observed in the
// admin UI, which renders these fields.
//
// LimitPaise nil means "this caller may not approve ANY amount", which is what
// a NULL role limit means — absence is never read as unlimited (the
// printer_role rule, ADR-019 §5). Nil here and 0 here are different facts and
// must not be collapsed by a consumer.
type ApprovalRefusal struct {
	Code         string
	TotalPaise   int64
	LimitPaise   *int64
	Alternatives []string
	Reason       string
}

// approvalRefusalCodeNoPermission and approvalRefusalCodeOverLimit are the two
// gates, kept distinct on the wire so an admin can tell "you may never approve"
// from "you may approve, but not this much".
const (
	approvalRefusalCodeNoPermission = "po_approval_permission_missing"
	approvalRefusalCodeOverLimit    = "po_exceeds_approval_limit"
)

func (e *ApprovalRefusal) Error() string {
	limit := "no approval limit is configured for your role, so you may not approve any amount"
	if e.LimitPaise != nil {
		limit = fmt.Sprintf("your role's approval limit is %d paise", *e.LimitPaise)
	}
	next := "ask a tenant administrator to raise your role's po_approval_limit_paise"
	if len(e.Alternatives) > 0 {
		next = fmt.Sprintf("ask one of these roles to approve it instead: %v", e.Alternatives)
	}
	return fmt.Sprintf("%s: this purchase order totals %d paise and %s. Next: %s",
		e.Reason, e.TotalPaise, limit, next)
}

// Unwrap makes errors.Is(err, httpx.ErrForbidden) true, so any caller that has
// not special-cased this type still produces a 403 rather than a 500.
func (e *ApprovalRefusal) Unwrap() error { return httpx.ErrForbidden }

// validateAuthority enforces the §50.1 authority rule: an envelope for
// aggregateType must carry exactly the direction contracts.AggregateAuthority
// assigns it. Anything else is rejected outright rather than coerced.
func validateAuthority(aggregateType contracts.AggregateType, direction contracts.SyncDirection) error {
	want, known := contracts.AggregateAuthority[aggregateType]
	if !known {
		return fmt.Errorf("%w: unknown aggregate type %q", httpx.ErrInvalidInput, aggregateType)
	}
	if direction != want {
		return fmt.Errorf("%w: aggregate %q requires direction %q, got %q", ErrAuthorityViolation, aggregateType, want, direction)
	}
	return nil
}

// requireEnvelope is the single-type route pin, mirroring
// backend/internal/inventory/statemachine.go's function of the same name.
func requireEnvelope(env contracts.SyncEnvelope, expectAggregate contracts.AggregateType) error {
	if env.AggregateType != expectAggregate {
		return fmt.Errorf("%w: expected aggregate_type %q, got %q", ErrAuthorityViolation, expectAggregate, env.AggregateType)
	}
	return requireEnvelopeCommon(env)
}

func requireEnvelopeCommon(env contracts.SyncEnvelope) error {
	if err := validateAuthority(env.AggregateType, env.Direction); err != nil {
		return err
	}
	if env.RecordID == "" {
		return fmt.Errorf("%w: record_id is required", httpx.ErrInvalidInput)
	}
	if env.TenantID == "" {
		return fmt.Errorf("%w: tenant_id is required", httpx.ErrInvalidInput)
	}
	if env.OutletID == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if env.DeviceID == "" {
		return fmt.Errorf("%w: device_id is required", httpx.ErrInvalidInput)
	}
	if env.Version < 1 {
		return fmt.Errorf("%w: version must be >= 1", httpx.ErrInvalidInput)
	}
	return nil
}

// requireTenantMatch guards against a device replaying an envelope for a
// tenant other than the one its credential authenticates as.
func requireTenantMatch(callerTenantID string, env contracts.SyncEnvelope) error {
	if callerTenantID == "" {
		return httpx.ErrUnauthorized
	}
	if env.TenantID != callerTenantID {
		return httpx.ErrForbidden
	}
	return nil
}

// canApprove lists the purchase order statuses an approval may move from.
// APPROVED/SENT/CLOSED already have an approver on the row (the
// purchase_order_approved_states_need_an_approver CHECK), and CANCELLED is
// terminal.
func canApprove(status PurchaseOrderStatus) bool {
	switch status {
	case PurchaseOrderStatusDraft, PurchaseOrderStatusPendingApproval:
		return true
	default:
		return false
	}
}
