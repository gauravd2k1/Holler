package procurement

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/holler/backend/internal/platform/id"
	contracts "github.com/holler/contracts"
)

// The 0.7.0 (ADR-024) read path, against a live Postgres.
//
// WHY A ROUND TRIP AND NOT AN ASSERTION ON THE MAPPER. Contracts 0.5.9 shipped
// a dropped field that a 201-echo comparison structurally could not see: the
// handler echoed the struct it had decoded, so the field was absent from both
// sides and the comparison was green. The only check that catches that shape is
// ingest through the real write path, read back through the real read path, and
// compare against a fixture neither of them produced.
//
// AND WHY THE FIXTURE HAS TWO RECEIPTS. That same defect survived four versions
// because the fidelity fixture was a wastage entry, on which every provenance
// field is legitimately null — a null round-trips through a nonexistent field
// perfectly. So one receipt here carries a REAL supplier row and one carries
// neither link. A test that only covers the null case proves nothing about the
// populated one.
//
// WHAT THIS FIXTURE DOES NOT COVER, STATED SO IT IS NOT MISREAD AS COVERAGE:
// no receipt here carries a purchase_order_id or a grn_line.purchase_order_line_id.
// Seeding an approved purchase order is more scaffolding than this read test
// needs, and TestPostgres_GrnNeverBlocksOnAPurchaseOrder already exercises the
// PO-linked write path — but neither reads one back through the 0.7.0 route, so
// the PO provenance group is UNPROVEN on the read path.
func TestPostgres_GoodsReceiptReadRoundTrip(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "GrnRead")
	svc := newPgService(pool)

	// --- receipt 1: full provenance -----------------------------------------
	withPOID := id.New()
	supplierID := id.New()
	// A REAL supplier row, not a stub id. supplier_id is a foreign key, so a
	// synthesised uuid would be rejected by the database and the "full
	// provenance" half of this fixture would silently become a second copy of
	// the bare half -- which is exactly the shape of the 0.5.9 defect this
	// test exists to prevent.
	if _, _, err := svc.CreateSupplier(ctx, fx.tenantID, NewSupplierInput{
		Supplier: Supplier{
			ID: supplierID, OutletID: fx.outletID,
			Code: "SUP-" + idSuffix(supplierID), Name: "Round Trip Traders",
			PaymentTermsDays: 30, IsActive: true,
		},
	}); err != nil {
		t.Fatalf("seeding the supplier: %v", err)
	}

	withPO := GoodsReceiptNote{
		ID: withPOID, OutletID: fx.outletID,
		// A supplier but NO purchase order: the two provenance links are
		// independent, and a receipt against a supplier with no PO is the
		// ordinary case this market produces.
		PurchaseOrderID: nil, SupplierID: &supplierID,
		GrnNumber:  "GRN-RT-" + idSuffix(withPOID),
		ReceivedAt: "2026-09-06T09:15:00Z", ReceivedByUserID: fx.userID,
		BusinessDate: "2026-09-06",
		Lines: []GrnLine{{
			ID: id.New(), InventoryItemID: fx.massItemID, LineNumber: 1,
			EnteredPurchaseUnit:  "50kg sack",
			EnteredQuantityMicro: 3_000_000, QuantityDimension: DimensionMass,
			BaseQuantityMicro: 150_000_000, PackSizeMicroApplied: 50_000_000,
			UnitCostPaise: 240, LineTotalPaise: 360000,
		}},
	}
	if _, err := svc.IngestGoodsReceiptNote(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeGoodsReceiptNote, withPOID), withPO); err != nil {
		t.Fatalf("ingesting the full-provenance receipt: %v", err)
	}

	// --- receipt 2: no PO, no supplier, no PO line --------------------------
	bareID := id.New()
	bare := GoodsReceiptNote{
		ID: bareID, OutletID: fx.outletID,
		PurchaseOrderID: nil, SupplierID: nil,
		GrnNumber:  "GRN-RT-" + idSuffix(bareID),
		ReceivedAt: "2026-09-06T11:45:00Z", ReceivedByUserID: fx.userID,
		BusinessDate: "2026-09-06",
		Lines: []GrnLine{{
			ID: id.New(), InventoryItemID: fx.countItemID, LineNumber: 1,
			PurchaseOrderLineID:  nil,
			EnteredPurchaseUnit:  "crate",
			EnteredQuantityMicro: 4_000_000, QuantityDimension: DimensionCount,
			BaseQuantityMicro: 48_000_000, PackSizeMicroApplied: 12_000_000,
			UnitCostPaise: 1500, LineTotalPaise: 72000,
		}},
	}
	if _, err := svc.IngestGoodsReceiptNote(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeGoodsReceiptNote, bareID), bare); err != nil {
		t.Fatalf("a receipt with no PO and no supplier must be ACCEPTED: %v", err)
	}

	// --- read one back, field by field --------------------------------------
	got, gaps, err := svc.GetGoodsReceipt(ctx, fx.outletID, withPOID)
	if err != nil {
		t.Fatalf("GetGoodsReceipt: %v", err)
	}
	if gaps == nil {
		t.Error("gaps must be an empty slice, never nil: a nil serialises to JSON null and a reader cannot tell 'no gaps' from 'gaps not loaded'")
	}
	read := goodsReceiptToRead(got)

	if read.PurchaseOrderID != nil {
		t.Errorf("purchase_order_id must stay NULL on a receipt raised without one, got %v", *read.PurchaseOrderID)
	}
	if read.SupplierID == nil || *read.SupplierID != supplierID {
		t.Errorf("supplier_id round-tripped as %v, want %s", read.SupplierID, supplierID)
	}
	if len(read.Lines) != 1 {
		t.Fatalf("lines round-tripped as %d, want 1", len(read.Lines))
	}
	line := read.Lines[0]

	// ALL THREE QUANTITY FIELDS, ASSERTED SEPARATELY. ADR-019 §3 requires
	// "what did they actually type?" to stay answerable from the row, so a
	// read path that kept only base_quantity_micro would satisfy a test that
	// checked the quantity and destroy the thing the column exists for.
	if line.EnteredQuantityMicro != 3_000_000 {
		t.Errorf("entered_quantity_micro = %d, want 3000000", line.EnteredQuantityMicro)
	}
	if line.BaseQuantityMicro != 150_000_000 {
		t.Errorf("base_quantity_micro = %d, want 150000000", line.BaseQuantityMicro)
	}
	if line.PackSizeMicroApplied != 50_000_000 {
		t.Errorf("pack_size_micro_applied = %d, want 50000000", line.PackSizeMicroApplied)
	}
	if line.LineTotalPaise != 360000 {
		t.Errorf("line_total_paise = %d, want 360000", line.LineTotalPaise)
	}
	if line.QuantityDimension != string(DimensionMass) {
		t.Errorf("quantity_dimension = %q, want %q", line.QuantityDimension, DimensionMass)
	}

	// --- the bare receipt keeps its nulls -----------------------------------
	bareGot, _, err := svc.GetGoodsReceipt(ctx, fx.outletID, bareID)
	if err != nil {
		t.Fatalf("GetGoodsReceipt (bare): %v", err)
	}
	bareRead := goodsReceiptToRead(bareGot)
	if bareRead.PurchaseOrderID != nil {
		t.Errorf("purchase_order_id must stay NULL, got %v — a read path may never invent a link the receipt does not have", *bareRead.PurchaseOrderID)
	}
	if bareRead.SupplierID != nil {
		t.Errorf("supplier_id must stay NULL, got %v", *bareRead.SupplierID)
	}
	if bareRead.Lines[0].PurchaseOrderLineID != nil {
		t.Errorf("purchase_order_line_id must stay NULL, got %v", *bareRead.Lines[0].PurchaseOrderLineID)
	}

	// --- the serialised bytes, not just the struct --------------------------
	// A struct comparison passes on a field the JSON tag drops. This is the
	// half of the check that sees the wire.
	raw, err := json.Marshal(bareRead)
	if err != nil {
		t.Fatalf("marshalling: %v", err)
	}
	var wire map[string]any
	if err := json.Unmarshal(raw, &wire); err != nil {
		t.Fatalf("unmarshalling: %v", err)
	}
	for _, field := range []string{
		"purchase_order_id", "supplier_id", "grn_number", "received_at",
		"business_date", "outlet_id", "lines",
	} {
		if _, present := wire[field]; !present {
			t.Errorf("%s is absent from the serialised receipt — a dropped field is invisible to a struct comparison", field)
		}
	}
	if wire["purchase_order_id"] != nil {
		t.Error("purchase_order_id must serialise as null, not be omitted: absent and null are different answers to 'was there a PO?'")
	}
}

// TestPostgres_GoodsReceiptReadIsTenantScoped pins the rule that a well-formed
// id from another outlet MISSES rather than matching. The existing by-id lookup
// is scoped by id alone — correct for ingest idempotency, and exactly wrong for
// a browser, where it would serve one tenant's receipts to another.
func TestPostgres_GoodsReceiptReadIsTenantScoped(t *testing.T) {
	pool := setupPool(t)
	ctx := context.Background()
	fx := newPgFixture(t, pool, "GrnScope")
	svc := newPgService(pool)

	grnID := id.New()
	grn := GoodsReceiptNote{
		ID: grnID, OutletID: fx.outletID,
		GrnNumber:  "GRN-SC-" + idSuffix(grnID),
		ReceivedAt: "2026-09-06T12:00:00Z", ReceivedByUserID: fx.userID,
		BusinessDate: "2026-09-06",
		Lines: []GrnLine{{
			ID: id.New(), InventoryItemID: fx.massItemID, LineNumber: 1,
			EnteredPurchaseUnit:  "50kg sack",
			EnteredQuantityMicro: 1_000_000, QuantityDimension: DimensionMass,
			BaseQuantityMicro: 50_000_000, PackSizeMicroApplied: 50_000_000,
			UnitCostPaise: 240, LineTotalPaise: 120000,
		}},
	}
	if _, err := svc.IngestGoodsReceiptNote(ctx, fx.tenantID,
		pgEnvelope(fx, contracts.AggregateTypeGoodsReceiptNote, grnID), grn); err != nil {
		t.Fatalf("ingesting: %v", err)
	}

	// The same id, asked for as the OTHER outlet in the same tenant.
	if _, _, err := svc.GetGoodsReceipt(ctx, fx.otherOutletID, grnID); err == nil {
		t.Fatal("a receipt belonging to another outlet must NOT be readable; an identifier is not a security boundary (§74)")
	}

	// And it is still readable by its own outlet, so the check above is not
	// passing for the trivial reason that nothing is readable at all.
	if _, _, err := svc.GetGoodsReceipt(ctx, fx.outletID, grnID); err != nil {
		t.Fatalf("the owning outlet must still be able to read it: %v", err)
	}
}
