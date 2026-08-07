package ordering

import (
	"errors"
	"testing"

	contracts "github.com/holler/contracts"
)

// TestValidTransition_FullTable exercises every legal transition in
// docs/spec/ordering.md's state machine plus a representative set of
// illegal ones (including CLOSED -> DRAFT, the example from the spec).
func TestValidTransition_FullTable(t *testing.T) {
	legal := []struct {
		from, to contracts.OrderStatus
	}{
		{contracts.OrderStatusDraft, contracts.OrderStatusConfirmed},
		{contracts.OrderStatusConfirmed, contracts.OrderStatusSentToKitchen},
		{contracts.OrderStatusSentToKitchen, contracts.OrderStatusPreparing},
		{contracts.OrderStatusPreparing, contracts.OrderStatusReady},
		{contracts.OrderStatusReady, contracts.OrderStatusServed},
		{contracts.OrderStatusServed, contracts.OrderStatusBilled},
		{contracts.OrderStatusBilled, contracts.OrderStatusPaid},
		{contracts.OrderStatusPaid, contracts.OrderStatusClosed},
		{contracts.OrderStatusDraft, contracts.OrderStatusCancelled},
		{contracts.OrderStatusConfirmed, contracts.OrderStatusCancelled},
		{contracts.OrderStatusSentToKitchen, contracts.OrderStatusCancelled},
		{contracts.OrderStatusPreparing, contracts.OrderStatusCancelled},
		{contracts.OrderStatusReady, contracts.OrderStatusCancelled},
	}
	for _, tc := range legal {
		if !validTransition(tc.from, tc.to) {
			t.Errorf("expected %s -> %s to be legal", tc.from, tc.to)
		}
	}

	illegal := []struct {
		from, to contracts.OrderStatus
	}{
		{contracts.OrderStatusClosed, contracts.OrderStatusDraft},
		{contracts.OrderStatusDraft, contracts.OrderStatusSentToKitchen},
		{contracts.OrderStatusDraft, contracts.OrderStatusPreparing},
		{contracts.OrderStatusConfirmed, contracts.OrderStatusPreparing},
		{contracts.OrderStatusSentToKitchen, contracts.OrderStatusReady},
		{contracts.OrderStatusPreparing, contracts.OrderStatusServed},
		{contracts.OrderStatusReady, contracts.OrderStatusBilled},
		{contracts.OrderStatusServed, contracts.OrderStatusPaid},
		{contracts.OrderStatusBilled, contracts.OrderStatusClosed},
		{contracts.OrderStatusPaid, contracts.OrderStatusBilled},
		{contracts.OrderStatusClosed, contracts.OrderStatusClosed},
		{contracts.OrderStatusCancelled, contracts.OrderStatusDraft},
		{contracts.OrderStatusCancelled, contracts.OrderStatusConfirmed},
		{contracts.OrderStatusServed, contracts.OrderStatusCancelled}, // served is no longer cancellable
		{contracts.OrderStatusBilled, contracts.OrderStatusCancelled},
		{contracts.OrderStatusPaid, contracts.OrderStatusCancelled},
		{contracts.OrderStatusClosed, contracts.OrderStatusCancelled},
	}
	for _, tc := range illegal {
		if validTransition(tc.from, tc.to) {
			t.Errorf("expected %s -> %s to be illegal", tc.from, tc.to)
		}
	}
}

func TestValidCreationStatus(t *testing.T) {
	if !validCreationStatus(contracts.OrderStatusDraft) {
		t.Error("DRAFT must be a valid creation status")
	}
	if !validCreationStatus(contracts.OrderStatusConfirmed) {
		t.Error("CONFIRMED must be a valid creation status")
	}
	for _, status := range []contracts.OrderStatus{
		contracts.OrderStatusSentToKitchen, contracts.OrderStatusPreparing,
		contracts.OrderStatusReady, contracts.OrderStatusServed,
		contracts.OrderStatusBilled, contracts.OrderStatusPaid,
		contracts.OrderStatusClosed, contracts.OrderStatusCancelled,
	} {
		if validCreationStatus(status) {
			t.Errorf("status %s must not be a valid creation status", status)
		}
	}
}

func TestValidateAuthority_OrderMustBeEdgeToCloud(t *testing.T) {
	if err := validateAuthority(contracts.AggregateTypeOrder, contracts.SyncDirectionEdgeToCloud); err != nil {
		t.Fatalf("EDGE_TO_CLOUD order envelope should be accepted: %v", err)
	}

	err := validateAuthority(contracts.AggregateTypeOrder, contracts.SyncDirectionCloudToEdge)
	if err == nil {
		t.Fatal("expected CLOUD_TO_EDGE order envelope to be rejected as an authority violation")
	}
	if !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("expected ErrAuthorityViolation, got %v", err)
	}
}
