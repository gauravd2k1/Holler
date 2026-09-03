package payments_test

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/holler/backend/internal/payments"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	contracts "github.com/holler/contracts"
)

// M6 A1b — a payment replayed against an order the cloud does not have must
// be a permanent client-data fault, not a server fault.
//
// WHY THIS ONE IS THE WORST OF THE FOURTEEN. `payment` carries four foreign
// keys — outlet_id, order_id, cash_shift_id, reverses_payment_id — and
// IngestPayment does NOT pre-check any of them (contrast kitchen.IngestKot,
// which pre-checks its parent and so failed with a 404 instead). So the
// insert reaches Postgres, 23503 fires, and before this change the repository
// wrapped it with fmt.Errorf and the client was told 500.
//
// The consequence is money. `payment` is APPEND-ONLY in both stores — a
// tender is corrected by an appended reversal, never a mutation (contracts
// 0.4.5) — so nothing later cleans up a tender that failed to replay. The
// edge treats 5xx as transient, so it retried forever; before M6 A2 that
// stranded every row behind it, and after A2 it still blocks this
// aggregate's own later events, which for a payment means its reversal too.
//
// A cash drawer that balances at the till and never reaches the cloud is the
// exact failure an owner cannot detect and cannot reconstruct.
//
// Watched failing first on the pre-fix binary: 500-class, unmapped.
func TestIngest_Payment_ForUnknownOrderIsMissingReference(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	ctx := context.Background()

	svc := payments.NewService(payments.NewPostgresRepository(pool))

	// A well-formed order id that no row carries. This is the replayed-tender
	// case: the till took money against an order whose own envelope the cloud
	// refused, so the order will never exist here.
	missingOrderID := id.New()
	paymentID := id.New()
	now := time.Now().UTC()

	env := contracts.SyncEnvelope{
		RecordID:      paymentID,
		TenantID:      fx.tenantID,
		OutletID:      fx.outletID,
		DeviceID:      "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
		AggregateType: contracts.AggregateTypePayment,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       1,
		SyncStatus:    contracts.SyncStatusPending,
	}

	_, err := svc.IngestPayment(ctx, fx.tenantID, fx.outletID, env, payments.Payment{
		ID:              paymentID,
		OutletID:        fx.outletID,
		OrderID:         missingOrderID,
		Method:          contracts.PaymentMethodCash,
		Status:          contracts.PaymentCaptureStatusCaptured,
		AmountPaise:     12550,
		CapturedAt:      &now,
		CreatedByUserID: fx.userID,
		CreatedAt:       now,
		UpdatedAt:       now,
		Version:         1,
		SchemaVersion:   1,
	})

	if err == nil {
		t.Fatal("ingesting a payment for a non-existent order succeeded; the foreign key must refuse it")
	}
	if !errors.Is(err, httpx.ErrMissingReference) {
		t.Errorf("error = %v, want it to match httpx.ErrMissingReference (422 missing_reference). "+
			"Reported as a server fault, the edge treats it as transient and retries a tender that "+
			"can never land — and `payment` is append-only, so nothing later corrects it", err)
	}
	if errors.Is(err, httpx.ErrNotFound) {
		t.Errorf("error = %v, must not be ErrNotFound: 404 means \"no such route\" to the edge, "+
			"which classifies it as transient", err)
	}
	if strings.Contains(err.Error(), "23503") || strings.Contains(err.Error(), "SQLSTATE") {
		t.Errorf("error text leaks the driver detail to a caller: %v", err)
	}
	// The message must name the field an operator can act on.
	if !strings.Contains(err.Error(), "order_id") {
		t.Errorf("error %q does not name the offending field", err.Error())
	}
}

// The same route must keep working when the order DOES exist — the guard is
// worthless if it also refuses good tenders, and a fixture that only tests
// the failure cannot see that.
func TestIngest_Payment_ForKnownOrderStillSucceeds(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	ctx := context.Background()

	svc := payments.NewService(payments.NewPostgresRepository(pool))
	paymentID := id.New()
	now := time.Now().UTC()

	env := contracts.SyncEnvelope{
		RecordID:      paymentID,
		TenantID:      fx.tenantID,
		OutletID:      fx.outletID,
		DeviceID:      "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
		AggregateType: contracts.AggregateTypePayment,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       1,
		SyncStatus:    contracts.SyncStatusPending,
	}

	stored, err := svc.IngestPayment(ctx, fx.tenantID, fx.outletID, env, payments.Payment{
		ID:              paymentID,
		OutletID:        fx.outletID,
		OrderID:         fx.orderID,
		Method:          contracts.PaymentMethodCash,
		Status:          contracts.PaymentCaptureStatusCaptured,
		AmountPaise:     12550,
		CapturedAt:      &now,
		CreatedByUserID: fx.userID,
		CreatedAt:       now,
		UpdatedAt:       now,
		Version:         1,
		SchemaVersion:   1,
	})
	if err != nil {
		t.Fatalf("ingesting a payment for an existing order failed: %v", err)
	}
	if stored.ID != paymentID {
		t.Errorf("stored payment id = %q, want %q", stored.ID, paymentID)
	}
}
