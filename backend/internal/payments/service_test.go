package payments

import (
	"context"
	"errors"
	"testing"
	"time"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/platform/httpx"
)

// fakeRepo is an in-memory Repository used to test Service without a live
// Postgres, mirroring backend/internal/ordering's fakeRepo pattern.
type fakeRepo struct {
	invoices     map[string]Invoice
	invoiceIndex map[string]string // "outletID|seriesID|number" -> invoice id
	payments     map[string]Payment
	shifts       map[string]CashShift
}

func newFakeRepo() *fakeRepo {
	return &fakeRepo{
		invoices:     map[string]Invoice{},
		invoiceIndex: map[string]string{},
		payments:     map[string]Payment{},
		shifts:       map[string]CashShift{},
	}
}

func (f *fakeRepo) InsertInvoice(ctx context.Context, tenantID string, inv Invoice) (Invoice, bool, error) {
	if existing, ok := f.invoices[inv.ID]; ok {
		return existing, false, nil
	}
	key := inv.OutletID + "|" + inv.SeriesID + "|" + inv.InvoiceNumber
	if _, taken := f.invoiceIndex[key]; taken {
		return Invoice{}, false, ErrDuplicateInvoiceNumber
	}
	f.invoices[inv.ID] = inv
	f.invoiceIndex[key] = inv.ID
	return inv, true, nil
}

func (f *fakeRepo) GetInvoice(ctx context.Context, tenantID, invoiceID string) (Invoice, error) {
	inv, ok := f.invoices[invoiceID]
	if !ok {
		return Invoice{}, httpx.ErrNotFound
	}
	return inv, nil
}

func (f *fakeRepo) InsertPayment(ctx context.Context, tenantID string, p Payment) (Payment, bool, error) {
	if existing, ok := f.payments[p.ID]; ok {
		return existing, false, nil
	}
	f.payments[p.ID] = p
	return p, true, nil
}

func (f *fakeRepo) GetPayment(ctx context.Context, tenantID, paymentID string) (Payment, error) {
	p, ok := f.payments[paymentID]
	if !ok {
		return Payment{}, httpx.ErrNotFound
	}
	return p, nil
}

func (f *fakeRepo) InsertCashShift(ctx context.Context, tenantID string, s CashShift) (CashShift, bool, error) {
	if existing, ok := f.shifts[s.ID]; ok {
		return existing, false, nil
	}
	f.shifts[s.ID] = s
	return s, true, nil
}

func (f *fakeRepo) CloseCashShift(ctx context.Context, tenantID string, s CashShift) (CashShift, bool, error) {
	current, ok := f.shifts[s.ID]
	if !ok {
		return CashShift{}, false, httpx.ErrNotFound
	}
	if s.Version <= current.Version {
		return current, false, nil
	}
	f.shifts[s.ID] = s
	return s, true, nil
}

func (f *fakeRepo) GetCashShift(ctx context.Context, tenantID, shiftID string) (CashShift, error) {
	s, ok := f.shifts[shiftID]
	if !ok {
		return CashShift{}, httpx.ErrNotFound
	}
	return s, nil
}

const (
	testTenantID = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa"
	testOutletID = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb"
	testDeviceID = "cccccccc-cccc-7ccc-8ccc-cccccccccccc"
)

func invoiceEnvelope(recordID string, version int) contracts.SyncEnvelope {
	now := time.Now().UTC()
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      testTenantID,
		OutletID:      testOutletID,
		DeviceID:      testDeviceID,
		AggregateType: contracts.AggregateTypeInvoice,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

func baseInvoice(id string) Invoice {
	now := time.Now().UTC()
	return Invoice{
		ID:                     id,
		OutletID:               testOutletID,
		OrderID:                "dddddddd-dddd-7ddd-8ddd-dddddddddddd",
		SplitIndex:             1,
		SplitCount:             1,
		SeriesID:               "eeeeeeee-eeee-7eee-8eee-eeeeeeeeeeee",
		InvoiceNumber:          "FY26/PNQ/000001",
		InvoiceDate:            now,
		BusinessDate:           "2026-08-14",
		Status:                 contracts.InvoiceStatusIssued,
		PlaceOfSupplyStateCode: "27",
		Lines:                  []contracts.InvoiceLine{},
		SubtotalPaise:          10000,
		TaxableValuePaise:      10000,
		CGSTPaise:              250,
		SGSTPaise:              250,
		RoundOffPaise:          0,
		GrandTotalPaise:        10500,
		ComplianceVersionID:    "ffffffff-ffff-7fff-8fff-ffffffffffff",
		TaxSnapshot:            map[string]interface{}{"rate": "5%"},
		FiscalProfile:          map[string]interface{}{"gstin": "27ABCDE1234F1Z5"},
		Channel:                "DIRECT",
		TaxLiabilityParty:      contracts.TaxLiabilityRestaurant,
		CreatedByUserID:        "11111111-1111-7111-8111-111111111111",
		CreatedAt:              now,
		UpdatedAt:              now,
		Version:                1,
		SchemaVersion:          1,
	}
}

func TestIngestInvoice_HappyPath(t *testing.T) {
	svc := NewService(newFakeRepo())
	inv := baseInvoice("22222222-2222-7222-8222-222222222222")

	stored, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, invoiceEnvelope(inv.ID, 1), inv)
	if err != nil {
		t.Fatalf("IngestInvoice: %v", err)
	}
	if stored.InvoiceNumber != inv.InvoiceNumber {
		t.Fatalf("expected invoice_number %s, got %s", inv.InvoiceNumber, stored.InvoiceNumber)
	}
}

func TestIngestInvoice_DuplicateRecordIDIsIdempotent(t *testing.T) {
	repo := newFakeRepo()
	svc := NewService(repo)
	inv := baseInvoice("22222222-2222-7222-8222-222222222222")

	if _, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, invoiceEnvelope(inv.ID, 1), inv); err != nil {
		t.Fatalf("first IngestInvoice: %v", err)
	}
	if _, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, invoiceEnvelope(inv.ID, 1), inv); err != nil {
		t.Fatalf("replayed IngestInvoice: %v", err)
	}
	if len(repo.invoices) != 1 {
		t.Fatalf("expected exactly one stored invoice after replay, got %d", len(repo.invoices))
	}
}

func TestIngestInvoice_DuplicateInvoiceNumberDifferentIDIsConflict(t *testing.T) {
	repo := newFakeRepo()
	svc := NewService(repo)
	first := baseInvoice("22222222-2222-7222-8222-222222222222")
	if _, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, invoiceEnvelope(first.ID, 1), first); err != nil {
		t.Fatalf("first IngestInvoice: %v", err)
	}

	second := baseInvoice("33333333-3333-7333-8333-333333333333") // same series+number, different id
	_, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, invoiceEnvelope(second.ID, 1), second)
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected httpx.ErrConflict for a duplicate invoice_number, got %v", err)
	}
}

func TestIngestInvoice_RoundingViolationIsRejected(t *testing.T) {
	svc := NewService(newFakeRepo())
	inv := baseInvoice("22222222-2222-7222-8222-222222222222")
	inv.GrandTotalPaise = 10499 // does not sum, and does not settle in whole rupees

	_, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, invoiceEnvelope(inv.ID, 1), inv)
	if !errors.Is(err, ErrRoundingViolation) {
		t.Fatalf("expected ErrRoundingViolation, got %v", err)
	}
}

func TestIngestInvoice_WrongAggregateTypeIsAuthorityViolation(t *testing.T) {
	svc := NewService(newFakeRepo())
	inv := baseInvoice("22222222-2222-7222-8222-222222222222")
	env := invoiceEnvelope(inv.ID, 1)
	env.AggregateType = contracts.AggregateTypePayment

	_, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, env, inv)
	if !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("expected ErrAuthorityViolation, got %v", err)
	}
}

func TestIngestInvoice_CloudToEdgeDirectionIsAuthorityViolation(t *testing.T) {
	svc := NewService(newFakeRepo())
	inv := baseInvoice("22222222-2222-7222-8222-222222222222")
	env := invoiceEnvelope(inv.ID, 1)
	env.Direction = contracts.SyncDirectionCloudToEdge

	_, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, env, inv)
	if !errors.Is(err, ErrAuthorityViolation) {
		t.Fatalf("expected ErrAuthorityViolation, got %v", err)
	}
}

func TestIngestInvoice_WrongTenantIsForbidden(t *testing.T) {
	svc := NewService(newFakeRepo())
	inv := baseInvoice("22222222-2222-7222-8222-222222222222")
	env := invoiceEnvelope(inv.ID, 1)
	env.TenantID = "99999999-9999-7999-8999-999999999999"

	_, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, env, inv)
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected httpx.ErrForbidden for a tenant mismatch against the device credential, got %v", err)
	}
}

func TestIngestInvoice_WrongOutletIsForbidden(t *testing.T) {
	svc := NewService(newFakeRepo())
	inv := baseInvoice("22222222-2222-7222-8222-222222222222")
	env := invoiceEnvelope(inv.ID, 1)
	env.OutletID = "99999999-9999-7999-8999-999999999999"

	_, err := svc.IngestInvoice(context.Background(), testTenantID, testOutletID, env, inv)
	if !errors.Is(err, httpx.ErrForbidden) {
		t.Fatalf("expected httpx.ErrForbidden for an outlet mismatch against the device credential, got %v", err)
	}
}

// --- payment -----------------------------------------------------------

func paymentEnvelope(recordID string, version int) contracts.SyncEnvelope {
	now := time.Now().UTC()
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      testTenantID,
		OutletID:      testOutletID,
		DeviceID:      testDeviceID,
		AggregateType: contracts.AggregateTypePayment,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

func basePayment(id string) Payment {
	now := time.Now().UTC()
	return Payment{
		ID:              id,
		OutletID:        testOutletID,
		OrderID:         "dddddddd-dddd-7ddd-8ddd-dddddddddddd",
		Method:          contracts.PaymentMethodCash,
		Status:          contracts.PaymentCaptureStatusCaptured,
		AmountPaise:     10500,
		Allocations:     []contracts.PaymentAllocation{},
		CreatedByUserID: "11111111-1111-7111-8111-111111111111",
		CreatedAt:       now,
		UpdatedAt:       now,
		Version:         1,
		SchemaVersion:   1,
	}
}

func TestIngestPayment_HappyPath(t *testing.T) {
	svc := NewService(newFakeRepo())
	p := basePayment("44444444-4444-7444-8444-444444444444")

	stored, err := svc.IngestPayment(context.Background(), testTenantID, testOutletID, paymentEnvelope(p.ID, 1), p)
	if err != nil {
		t.Fatalf("IngestPayment: %v", err)
	}
	if stored.AmountPaise != p.AmountPaise {
		t.Fatalf("expected amount_paise %d, got %d", p.AmountPaise, stored.AmountPaise)
	}
}

// TestIngestPayment_ReversalIsAppendOnly proves a void/refund is a NEW
// payment row carrying reverses_payment_id and a non-positive amount — the
// service never updates the original (§53).
func TestIngestPayment_ReversalIsAppendOnly(t *testing.T) {
	repo := newFakeRepo()
	svc := NewService(repo)
	original := basePayment("44444444-4444-7444-8444-444444444444")
	if _, err := svc.IngestPayment(context.Background(), testTenantID, testOutletID, paymentEnvelope(original.ID, 1), original); err != nil {
		t.Fatalf("ingesting original payment: %v", err)
	}

	reversal := basePayment("55555555-5555-7555-8555-555555555555")
	reversal.AmountPaise = -10500
	reversal.ReversesPaymentID = &original.ID
	if _, err := svc.IngestPayment(context.Background(), testTenantID, testOutletID, paymentEnvelope(reversal.ID, 1), reversal); err != nil {
		t.Fatalf("ingesting reversal payment: %v", err)
	}

	if len(repo.payments) != 2 {
		t.Fatalf("expected exactly two payment rows (original + reversal), got %d", len(repo.payments))
	}
	if got := repo.payments[original.ID]; got.Status != contracts.PaymentCaptureStatusCaptured {
		t.Fatalf("expected the original payment untouched, got status %s", got.Status)
	}
}

func TestIngestPayment_PositiveReversalAmountIsRejected(t *testing.T) {
	svc := NewService(newFakeRepo())
	p := basePayment("44444444-4444-7444-8444-444444444444")
	originalID := "66666666-6666-7666-8666-666666666666"
	p.ReversesPaymentID = &originalID
	p.AmountPaise = 100 // must be <= 0 on a reversal

	_, err := svc.IngestPayment(context.Background(), testTenantID, testOutletID, paymentEnvelope(p.ID, 1), p)
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected httpx.ErrInvalidInput for a positive-amount reversal, got %v", err)
	}
}

// --- cash_shift ----------------------------------------------------------

func cashShiftEnvelope(recordID string, version int) contracts.SyncEnvelope {
	now := time.Now().UTC()
	return contracts.SyncEnvelope{
		RecordID:      recordID,
		TenantID:      testTenantID,
		OutletID:      testOutletID,
		DeviceID:      testDeviceID,
		AggregateType: contracts.AggregateTypeCashShift,
		Direction:     contracts.SyncDirectionEdgeToCloud,
		CreatedAt:     now,
		UpdatedAt:     now,
		Version:       version,
		SyncStatus:    contracts.SyncStatusPending,
	}
}

func baseOpenShift(id string) CashShift {
	now := time.Now().UTC()
	return CashShift{
		ID:               id,
		OutletID:         testOutletID,
		DeviceID:         testDeviceID,
		CashierUserID:    "11111111-1111-7111-8111-111111111111",
		Status:           contracts.CashShiftStatusOpen,
		OpenedAt:         now,
		OpeningCashPaise: 500000,
		BusinessDate:     "2026-08-14",
		Movements:        []contracts.CashMovement{},
		CreatedAt:        now,
		UpdatedAt:        now,
		Version:          1,
		SchemaVersion:    1,
	}
}

func TestIngestCashShift_HappyPath(t *testing.T) {
	svc := NewService(newFakeRepo())
	s := baseOpenShift("77777777-7777-7777-8777-777777777777")

	stored, err := svc.IngestCashShift(context.Background(), testTenantID, testOutletID, cashShiftEnvelope(s.ID, 1), s)
	if err != nil {
		t.Fatalf("IngestCashShift: %v", err)
	}
	if stored.Status != contracts.CashShiftStatusOpen {
		t.Fatalf("expected OPEN, got %s", stored.Status)
	}
}

func TestCloseCashShift_MissingCountIsRejected(t *testing.T) {
	repo := newFakeRepo()
	svc := NewService(repo)
	s := baseOpenShift("77777777-7777-7777-8777-777777777777")
	if _, err := svc.IngestCashShift(context.Background(), testTenantID, testOutletID, cashShiftEnvelope(s.ID, 1), s); err != nil {
		t.Fatalf("opening shift: %v", err)
	}

	closed := s
	closed.Status = contracts.CashShiftStatusClosed
	closed.Version = 2
	// closed_at/expected/actual/variance deliberately left unset.

	_, err := svc.CloseCashShift(context.Background(), testTenantID, testOutletID, cashShiftEnvelope(s.ID, 2), s.ID, closed)
	if !errors.Is(err, ErrShiftNotAccounted) {
		t.Fatalf("expected ErrShiftNotAccounted, got %v", err)
	}
}

func TestCloseCashShift_NonZeroVarianceWithoutReasonIsRejected(t *testing.T) {
	repo := newFakeRepo()
	svc := NewService(repo)
	s := baseOpenShift("77777777-7777-7777-8777-777777777777")
	if _, err := svc.IngestCashShift(context.Background(), testTenantID, testOutletID, cashShiftEnvelope(s.ID, 1), s); err != nil {
		t.Fatalf("opening shift: %v", err)
	}

	now := time.Now().UTC()
	expected := 500000
	actual := 499000
	variance := -1000
	closed := s
	closed.Status = contracts.CashShiftStatusClosed
	closed.Version = 2
	closed.ClosedAt = &now
	closed.ExpectedCashPaise = &expected
	closed.ActualCashPaise = &actual
	closed.VariancePaise = &variance
	// variance_reason deliberately left nil.

	_, err := svc.CloseCashShift(context.Background(), testTenantID, testOutletID, cashShiftEnvelope(s.ID, 2), s.ID, closed)
	if !errors.Is(err, ErrShiftNotAccounted) {
		t.Fatalf("expected ErrShiftNotAccounted for an unexplained variance, got %v", err)
	}
}

func TestCloseCashShift_HappyPath(t *testing.T) {
	repo := newFakeRepo()
	svc := NewService(repo)
	s := baseOpenShift("77777777-7777-7777-8777-777777777777")
	if _, err := svc.IngestCashShift(context.Background(), testTenantID, testOutletID, cashShiftEnvelope(s.ID, 1), s); err != nil {
		t.Fatalf("opening shift: %v", err)
	}

	now := time.Now().UTC()
	expected := 500000
	actual := 500000
	variance := 0
	closed := s
	closed.Status = contracts.CashShiftStatusClosed
	closed.Version = 2
	closed.ClosedAt = &now
	closed.ExpectedCashPaise = &expected
	closed.ActualCashPaise = &actual
	closed.VariancePaise = &variance

	stored, err := svc.CloseCashShift(context.Background(), testTenantID, testOutletID, cashShiftEnvelope(s.ID, 2), s.ID, closed)
	if err != nil {
		t.Fatalf("CloseCashShift: %v", err)
	}
	if stored.Status != contracts.CashShiftStatusClosed {
		t.Fatalf("expected CLOSED, got %s", stored.Status)
	}
}
