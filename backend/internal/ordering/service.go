package ordering

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
)

// Service implements the ordering business commands. Per docs/spec/sync.md
// §50.1 the edge is the authority for order transactions: every mutating
// method here takes a contracts.SyncEnvelope and replays it — it never
// originates state, never merges, and never generates an order id.
type Service struct {
	repo Repository
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo}
}

func requireEnvelope(env contracts.SyncEnvelope, expectAggregate contracts.AggregateType) error {
	// Both of these are route-mismatch cases per
	// packages/contracts/openapi/openapi.yaml's EnvelopeRouteMismatch
	// response (422): the route pins an aggregate_type, and §50.1 pins
	// that aggregate's direction. Either mismatch is a protocol violation,
	// never a coercion, so both wrap ErrAuthorityViolation so the HTTP
	// layer can map them to 422 (distinct from 400 malformed/missing-field
	// input below).
	if env.AggregateType != expectAggregate {
		return fmt.Errorf("%w: expected aggregate_type %q, got %q", ErrAuthorityViolation, expectAggregate, env.AggregateType)
	}
	if err := validateAuthority(env.AggregateType, env.Direction); err != nil {
		return err
	}
	if strings.TrimSpace(env.RecordID) == "" {
		return fmt.Errorf("%w: record_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(env.TenantID) == "" {
		return fmt.Errorf("%w: tenant_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(env.OutletID) == "" {
		return fmt.Errorf("%w: outlet_id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(env.DeviceID) == "" {
		return fmt.Errorf("%w: device_id is required", httpx.ErrInvalidInput)
	}
	if env.Version < 1 {
		return fmt.Errorf("%w: version must be >= 1", httpx.ErrInvalidInput)
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

// IngestOrder replays an edge-created order. The order id, all timestamps
// and every financial figure arrive as given — the cloud does not compute,
// default or re-derive them (tax/discount calculation is Milestone 3).
// Replaying the identical envelope twice (edge retry) creates exactly one
// row: idempotency keyed on the envelope's record_id.
func (s *Service) IngestOrder(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, order contracts.CanonicalOrder) (StoredOrder, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeOrder); err != nil {
		return StoredOrder{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StoredOrder{}, err
	}
	if strings.TrimSpace(order.HollerOrderID) == "" {
		return StoredOrder{}, fmt.Errorf("%w: holler_order_id is required", httpx.ErrInvalidInput)
	}
	if order.HollerOrderID != env.RecordID {
		return StoredOrder{}, fmt.Errorf("%w: payload holler_order_id must match envelope record_id", httpx.ErrInvalidInput)
	}
	if order.OutletID != env.OutletID {
		return StoredOrder{}, fmt.Errorf("%w: payload outlet_id must match envelope outlet_id", httpx.ErrInvalidInput)
	}
	if !validCreationStatus(order.Status) {
		return StoredOrder{}, fmt.Errorf("%w: order cannot be created in status %q", ErrIllegalTransition, order.Status)
	}
	if order.Items == nil {
		order.Items = []contracts.OrderItem{}
	}

	stored, _, err := s.repo.InsertOrder(ctx, callerTenantID, env.DeviceID, env.Version, order)
	if err != nil {
		return StoredOrder{}, err
	}
	return stored, nil
}

// AppendItem appends a line item to a DRAFT order. Line items are
// append-only: this is the only write path for order_item and it never
// updates or deletes an existing row. Idempotent on the item's own id.
func (s *Service) AppendItem(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, orderID string, item contracts.OrderItem) (StoredOrder, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeOrder); err != nil {
		return StoredOrder{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StoredOrder{}, err
	}
	orderID = strings.TrimSpace(orderID)
	if orderID == "" {
		return StoredOrder{}, fmt.Errorf("%w: order id is required", httpx.ErrInvalidInput)
	}
	if strings.TrimSpace(item.ID) == "" {
		return StoredOrder{}, fmt.Errorf("%w: item id is required", httpx.ErrInvalidInput)
	}

	current, err := s.repo.GetByID(ctx, callerTenantID, orderID)
	if err != nil {
		return StoredOrder{}, err
	}
	// THE AMENDABLE SET IS THE EDGE'S, AND IT IS READ FROM CONTRACTS, NEVER
	// RESTATED HERE (contracts 0.8.3, ADR-028). This was `== DRAFT`, while
	// the edge has allowed a line to be added through PREPARING since #132-A
	// — a second Send on the same table, which is ordinary restaurant work.
	// Under an at-least-once outbox that mismatch is not one refused line: the
	// 409 is permanent, so every later row of that order queues behind it and
	// the till shows a wedged aggregate. §50.1 makes the edge the authority
	// for order transactions; the cloud replays what the outlet did and does
	// not get an amendable set of its own.
	if !contracts.IsOrderItemAmendable(current.Status) {
		return StoredOrder{}, fmt.Errorf("%w: items can only be appended while an order is amendable (%v), order is %q", httpx.ErrConflict, contracts.OrderItemAmendableStatuses, current.Status)
	}

	if _, err := s.repo.AppendItem(ctx, callerTenantID, orderID, item); err != nil {
		return StoredOrder{}, err
	}
	return s.repo.GetByID(ctx, callerTenantID, orderID)
}

// replayVersion resolves the version a replayed transition stores.
//
// WHY A REPLAY IS NOT VERSION-SEQUENCED. `SyncEnvelope.version` is read from
// the LIVE edge aggregate row at the moment the outbox row is sent
// (edge/sync/src/worker.rs `load_aggregate_envelope_fields`), not stamped on
// the event when it was recorded. Every event still queued for one order
// therefore ships the SAME version — whatever the edge row has reached by
// then — and the edge carries no per-event version to send instead
// (local_outbox has no version column and the frozen OutboxEvent envelope has
// no field for one, packages/contracts sqlite/0001_init.sql +
// src/types/events.ts).
//
// So a cloud-side `version == current+1` rule was testing a number that
// cannot hold that property, and it failed in two directions, both observed
// on the live stack on 2026-09-16:
//
//   - The order synced while online, then more lines were added: the
//     transition arrived with a version several ahead, was refused 409
//     PERMANENTLY, and every later row of that aggregate stayed behind it
//     (per-aggregate ordering, M6 A2). Three orders wedged, five outbox rows
//     blocked.
//   - The order was taken entirely offline: create and transitions all
//     shipped the same version, the transition read as `version <=
//     current` — an already-applied replay — and was swallowed with 200. The
//     order sat DRAFT in the cloud forever, silently, with its items present.
//
// docs/spec/sync.md's conflict policy for the order aggregate is "state
// machine + command validation", and openapi.yaml documents the transition
// routes' 409 as an illegal transition — never as a version gap. The state
// machine is therefore the whole guard: `current.Status == to` is the
// idempotency test (it is the thing a duplicate delivery cannot change), and
// validTransition is the legality test.
//
// The stored version still only moves FORWARD, so nothing downstream that
// treats it as monotonic regresses: take the edge's number when it is ahead,
// otherwise step by one.
func replayVersion(envVersion, currentVersion int) int {
	if envVersion > currentVersion {
		return envVersion
	}
	return currentVersion + 1
}

// transition is the shared implementation behind SendToKitchen and Cancel:
// both are pure state-machine moves in Milestone 1 (KOT generation is
// Milestone 2; payment capture is out of scope entirely).
func (s *Service) transition(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, orderID string, to contracts.OrderStatus) (StoredOrder, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeOrder); err != nil {
		return StoredOrder{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StoredOrder{}, err
	}
	orderID = strings.TrimSpace(orderID)
	if orderID == "" {
		return StoredOrder{}, fmt.Errorf("%w: order id is required", httpx.ErrInvalidInput)
	}

	current, err := s.repo.GetByID(ctx, callerTenantID, orderID)
	if err != nil {
		return StoredOrder{}, err
	}

	// Idempotent replay: the edge resent a transition this order has
	// already taken. Return the current row rather than re-applying, and
	// never as an error — the state, not the envelope's version, is what
	// says whether this move has happened (see replayVersion).
	if current.Status == to {
		return current, nil
	}
	if !validTransition(current.Status, to) {
		return StoredOrder{}, fmt.Errorf("%w: cannot move order from %q to %q", ErrIllegalTransition, current.Status, to)
	}

	stored, applied, err := s.repo.UpdateStatus(ctx, callerTenantID, orderID, current.Version, replayVersion(env.Version, current.Version), to)
	if err != nil {
		return StoredOrder{}, err
	}
	if !applied {
		// Lost a race with a concurrent replay; the row has already moved
		// on, which is fine as long as it moved on legally — surface the
		// row currently stored rather than a false error.
		return stored, nil
	}
	return stored, nil
}

// Confirm replays the DRAFT->CONFIRMED transition (contracts 0.2.5). It is a
// dedicated method rather than a call into transition(): transition's
// UpdateStatus call has no way to carry a payload, and confirmed_at must be
// stamped from the envelope's payload, not the server clock — widening the
// generic transition/UpdateStatus path to accept an optional payload would
// let SendToKitchen/Cancel's callers reach a code path that must never carry
// one (ADR-011 0.2.5 addendum). confirmedAt is taken as given: the edge
// recorded it, §50.1 makes the edge authoritative for order transactions,
// and the cloud never substitutes time.Now().
func (s *Service) Confirm(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, orderID string, confirmedAt time.Time) (StoredOrder, error) {
	if err := requireEnvelope(env, contracts.AggregateTypeOrder); err != nil {
		return StoredOrder{}, err
	}
	if err := requireTenantMatch(callerTenantID, env); err != nil {
		return StoredOrder{}, err
	}
	orderID = strings.TrimSpace(orderID)
	if orderID == "" {
		return StoredOrder{}, fmt.Errorf("%w: order id is required", httpx.ErrInvalidInput)
	}
	if confirmedAt.IsZero() {
		return StoredOrder{}, fmt.Errorf("%w: confirmed_at is required", httpx.ErrInvalidInput)
	}

	current, err := s.repo.GetByID(ctx, callerTenantID, orderID)
	if err != nil {
		return StoredOrder{}, err
	}

	// Idempotent replay: the edge resent a confirmation this order has
	// already taken. Return the current row rather than re-applying or
	// shifting confirmed_at (see replayVersion).
	//
	// THE TEST IS EVIDENCE THAT A CONFIRM WAS APPLIED, NOT THE CURRENT
	// STATUS, because the order may legitimately have moved on since. A
	// redelivered confirm reaching a SENT_TO_KITCHEN order is the same lost
	// acknowledgement as one reaching a CONFIRMED order, and refusing it is
	// the same permanent 409 that strands every row behind it. `confirmed_at`
	// is that evidence: only this route sets it, and it outlives the status.
	// The status is checked too, for a cloud row confirmed before the column
	// was stamped by any production path (contracts 0.2.5 fixed that; rows
	// predating it exist).
	if current.Timestamps.ConfirmedAt != nil || current.Status == contracts.OrderStatusConfirmed {
		return current, nil
	}
	if current.Status != contracts.OrderStatusDraft {
		return StoredOrder{}, fmt.Errorf("%w: order must be DRAFT to confirm, is %q", httpx.ErrConflict, current.Status)
	}
	if !validTransition(current.Status, contracts.OrderStatusConfirmed) {
		return StoredOrder{}, fmt.Errorf("%w: cannot move order from %q to %q", ErrIllegalTransition, current.Status, contracts.OrderStatusConfirmed)
	}

	stored, applied, err := s.repo.ConfirmOrder(ctx, callerTenantID, orderID, current.Version, replayVersion(env.Version, current.Version), confirmedAt)
	if err != nil {
		return StoredOrder{}, err
	}
	if !applied {
		// Lost a race with a concurrent replay; surface the row as it
		// currently stands rather than a false error.
		return stored, nil
	}
	return stored, nil
}

// SendToKitchen transitions an order to SENT_TO_KITCHEN. KOT generation
// belongs to Milestone 2's kitchen context — this method changes order
// state only.
func (s *Service) SendToKitchen(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, orderID string) (StoredOrder, error) {
	return s.transition(ctx, callerTenantID, env, orderID, contracts.OrderStatusSentToKitchen)
}

// Cancel transitions an order to CANCELLED.
func (s *Service) Cancel(ctx context.Context, callerTenantID string, env contracts.SyncEnvelope, orderID, reason string) (StoredOrder, error) {
	if strings.TrimSpace(reason) == "" {
		return StoredOrder{}, fmt.Errorf("%w: cancellation reason is required", httpx.ErrInvalidInput)
	}
	return s.transition(ctx, callerTenantID, env, orderID, contracts.OrderStatusCancelled)
}

// GetOrder returns a single order, scoped to the caller's tenant.
func (s *Service) GetOrder(ctx context.Context, callerTenantID, orderID string) (StoredOrder, error) {
	if callerTenantID == "" {
		return StoredOrder{}, httpx.ErrUnauthorized
	}
	orderID = strings.TrimSpace(orderID)
	if orderID == "" {
		return StoredOrder{}, fmt.Errorf("%w: order id is required", httpx.ErrInvalidInput)
	}
	return s.repo.GetByID(ctx, callerTenantID, orderID)
}

// ListOrders returns every order for outletID, scoped to the caller's
// tenant. This is the only reporting Milestone 1 permits (CLAUDE.md).
func (s *Service) ListOrders(ctx context.Context, callerTenantID, outletID string) ([]StoredOrder, error) {
	if callerTenantID == "" {
		return nil, httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	if outletID == "" {
		return nil, fmt.Errorf("%w: outlet id is required", httpx.ErrInvalidInput)
	}
	orders, err := s.repo.ListByOutlet(ctx, callerTenantID, outletID)
	if err != nil {
		return nil, err
	}
	if orders == nil {
		orders = []StoredOrder{}
	}
	return orders, nil
}
