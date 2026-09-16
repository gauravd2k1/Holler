package main

import (
	"context"
	"testing"

	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/testdb"
)

// TestSeedCatalogueFromFile_WritesEveryTable writes the shared fixture into a
// real Postgres and reads rows back. It fails loudly (does not skip
// silently) if HOLLER_TEST_DATABASE_URL is unset -- see testdb.
//
// This runs against the whole call sequence main() uses:
// seedCatalogueFromFile, then seed() (cloud-only role/app_user/user_role,
// needed because goods_receipt_note.received_by_user_id and
// stock_ledger_entry.created_by_user_id reference app_user), then
// seedGoodsReceiptAndOpeningStock. Run twice to prove idempotency -- the
// second run must not error, including through the goods_receipt_note and
// stock_ledger_entry triggers that reject UPDATE.
func TestSeedCatalogueFromFile_WritesEveryTable(t *testing.T) {
	dbURL := testdb.RequireDatabaseURL(t)
	ctx := context.Background()

	pool, err := postgres.Open(ctx, dbURL)
	if err != nil {
		t.Fatalf("postgres.Open: %v", err)
	}
	defer pool.Close()

	if err := postgres.Migrate(ctx, pool, "../../../packages/contracts/postgres"); err != nil {
		t.Fatalf("postgres.Migrate: %v", err)
	}

	sf, err := loadSeedFile("testdata/demo-outlet-fixture.json")
	if err != nil {
		t.Fatalf("loadSeedFile: %v", err)
	}

	runOnce := func(label string) {
		if err := seedCatalogueFromFile(ctx, pool, sf); err != nil {
			t.Fatalf("%s: seedCatalogueFromFile: %v", label, err)
		}
		if err := seed(ctx, pool, "$argon2id$fixture-hash-not-a-real-credential"); err != nil {
			t.Fatalf("%s: seed: %v", label, err)
		}
		if err := seedGoodsReceiptAndOpeningStock(ctx, pool, sf); err != nil {
			t.Fatalf("%s: seedGoodsReceiptAndOpeningStock: %v", label, err)
		}
	}

	runOnce("first run")

	// THESE ASSERTIONS ARE ONLY FALSIFIABLE ON A FRESH DATABASE, and that is
	// worth knowing before trusting a green run. Every insert here is
	// ON CONFLICT DO NOTHING / DO UPDATE, so against a database that a
	// previous run already seeded, deleting the writing code entirely still
	// leaves the rows present and every assertion below still passes.
	// Confirmed by planting exactly that on 2026-09-16: skipping the
	// printer_role loop passed on a re-used database and failed on a fresh
	// one. Point HOLLER_TEST_DATABASE_URL at a database you create and drop
	// per run — which is what the scratch-database rule already requires.
	//
	// --- D10: the five cloud-owned families the cloud used to hold NONE of,
	// plus menu_item_station derived from the item's station_code.
	//
	// ROW-FOR-ROW, not a count: a count passes on a row inserted with the
	// wrong id, the wrong outlet or the wrong role, and "the printer exists"
	// is exactly the shape of check that let a printer with no role row read
	// as usable (contracts 0.4.7). ---
	assertRow := func(label, query string, args ...any) {
		t.Helper()
		var n int
		if err := pool.QueryRow(ctx, query, args...).Scan(&n); err != nil {
			t.Fatalf("counting %s: %v", label, err)
		}
		if n != 1 {
			t.Fatalf("expected exactly 1 %s row matching the fixture, got %d", label, n)
		}
	}

	// D11: the device row, seeded UNENROLLED. Both halves are asserted — the
	// row is present AND it carries no enrollment — because a seeded
	// enrolled_at would be a claim that a handshake happened.
	for _, d := range sf.Devices {
		assertRow("device",
			`SELECT count(*) FROM device
			 WHERE id = $1 AND outlet_id = $2 AND kind = $3 AND name = $4
			   AND enrolled_at IS NULL AND revoked_at IS NULL`,
			d.ID, d.OutletID, d.Kind, d.Name)
		assertRow("device with no credential",
			`SELECT count(*) FROM device d
			 WHERE d.id = $1
			   AND NOT EXISTS (SELECT 1 FROM device_credential c WHERE c.device_id = d.id)`,
			d.ID)
	}

	for _, tbl := range sf.RestaurantTables {
		assertRow("restaurant_table",
			`SELECT count(*) FROM restaurant_table
			 WHERE id = $1 AND outlet_id = $2 AND section = $3 AND label = $4
			   AND seat_count = $5 AND is_active = $6`,
			tbl.ID, tbl.OutletID, tbl.Section, tbl.Label, tbl.SeatCount, tbl.IsActive)
	}
	for _, st := range sf.Stations {
		assertRow("station",
			`SELECT count(*) FROM station
			 WHERE id = $1 AND outlet_id = $2 AND code = $3 AND name = $4
			   AND sort_order = $5 AND is_active = $6`,
			st.ID, st.OutletID, st.Code, st.Name, st.SortOrder, st.IsActive)
	}
	for _, pr := range sf.Printers {
		assertRow("printer",
			`SELECT count(*) FROM printer
			 WHERE id = $1 AND outlet_id = $2 AND name = $3 AND connection_kind = $4
			   AND address = $5 AND paper_width_mm = $6 AND is_active = $7`,
			pr.ID, pr.OutletID, pr.Name, pr.ConnectionKind, pr.Address, pr.PaperWidthMM, pr.IsActive)
	}
	for _, role := range sf.PrinterRoles {
		assertRow("printer_role",
			`SELECT count(*) FROM printer_role WHERE printer_id = $1 AND role = $2`,
			role.PrinterID, role.Role)
	}
	for _, sp := range sf.StationPrinters {
		assertRow("station_printer",
			`SELECT count(*) FROM station_printer WHERE station_id = $1 AND printer_id = $2`,
			sp.StationID, sp.PrinterID)
	}
	// menu_item_station is DERIVED from station_code, which the cloud seeder
	// used to discard under a comment claiming Postgres had no such table.
	stationIDByCode := map[string]string{}
	for _, st := range sf.Stations {
		stationIDByCode[st.Code] = st.ID
	}
	for _, mi := range sf.MenuItems {
		if mi.StationCode == "" {
			continue
		}
		assertRow("menu_item_station",
			`SELECT count(*) FROM menu_item_station WHERE menu_item_id = $1 AND station_id = $2`,
			mi.ID, stationIDByCode[mi.StationCode])
	}

	// --- Assert rows exist before asserting anything about their content
	// (the "fixtures did not insert" rule) ---

	var menuItemCount int
	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM menu_item WHERE id = $1`,
		"0191afff-0000-7000-8000-000000000011").Scan(&menuItemCount); err != nil {
		t.Fatalf("counting menu_item: %v", err)
	}
	if menuItemCount != 1 {
		t.Fatalf("expected exactly 1 menu_item row for the fixture id, got %d", menuItemCount)
	}

	var hsnSac string
	if err := pool.QueryRow(ctx,
		`SELECT hsn_sac FROM menu_item WHERE id = $1`,
		"0191afff-0000-7000-8000-000000000011").Scan(&hsnSac); err != nil {
		t.Fatalf("reading menu_item.hsn_sac: %v", err)
	}
	if hsnSac != "9963" {
		t.Fatalf("expected hsn_sac 9963, got %q", hsnSac)
	}

	var variantCount int
	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM menu_item_variant WHERE menu_item_id = $1 AND is_default = TRUE`,
		"0191afff-0000-7000-8000-000000000011").Scan(&variantCount); err != nil {
		t.Fatalf("counting default menu_item_variant: %v", err)
	}
	if variantCount != 1 {
		t.Fatalf("expected exactly 1 default variant, got %d", variantCount)
	}

	var quantityDimension string
	if err := pool.QueryRow(ctx,
		`SELECT quantity_dimension FROM recipe_ingredient WHERE id = $1`,
		"0191afff-0000-7000-8000-000000000031").Scan(&quantityDimension); err != nil {
		t.Fatalf("reading recipe_ingredient.quantity_dimension: %v", err)
	}
	if quantityDimension != "MASS" {
		t.Fatalf("expected quantity_dimension MASS (the authored unit), got %q", quantityDimension)
	}

	var grnCount int
	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM goods_receipt_note WHERE id = $1`,
		"0191afff-0000-7000-8000-000000000060").Scan(&grnCount); err != nil {
		t.Fatalf("counting goods_receipt_note: %v", err)
	}
	if grnCount != 1 {
		t.Fatalf("expected exactly 1 goods_receipt_note row, got %d", grnCount)
	}

	var grnLineCount int
	var enteredQty, baseQty, lineTotal int64
	if err := pool.QueryRow(ctx,
		`SELECT count(*), max(entered_quantity_micro), max(base_quantity_micro), max(line_total_paise)
		   FROM grn_line WHERE grn_id = $1`,
		"0191afff-0000-7000-8000-000000000060").Scan(&grnLineCount, &enteredQty, &baseQty, &lineTotal); err != nil {
		t.Fatalf("reading grn_line: %v", err)
	}
	if grnLineCount != 1 {
		t.Fatalf("expected exactly 1 grn_line row, got %d", grnLineCount)
	}
	if enteredQty != 10000000 || baseQty != 2500000000 || lineTotal != 200000 {
		t.Fatalf("grn_line quantities/total did not round-trip: entered=%d base=%d total=%d",
			enteredQty, baseQty, lineTotal)
	}

	var receiptLedgerCount int
	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM stock_ledger_entry WHERE source_grn_id = $1 AND origin = 'GOODS_RECEIPT'`,
		"0191afff-0000-7000-8000-000000000060").Scan(&receiptLedgerCount); err != nil {
		t.Fatalf("counting GRN stock_ledger_entry rows: %v", err)
	}
	if receiptLedgerCount != 1 {
		t.Fatalf("expected exactly 1 GOODS_RECEIPT stock_ledger_entry row, got %d", receiptLedgerCount)
	}

	var openingStockCount int
	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM stock_ledger_entry WHERE id = $1 AND origin = 'MANUAL'`,
		"0191afff-0000-7000-8000-000000000070").Scan(&openingStockCount); err != nil {
		t.Fatalf("counting opening_stock stock_ledger_entry row: %v", err)
	}
	if openingStockCount != 1 {
		t.Fatalf("expected exactly 1 opening-stock stock_ledger_entry row, got %d", openingStockCount)
	}

	var supplierItemCount int
	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM supplier_item WHERE id = $1`,
		"0191afff-0000-7000-8000-000000000051").Scan(&supplierItemCount); err != nil {
		t.Fatalf("counting supplier_item: %v", err)
	}
	if supplierItemCount != 1 {
		t.Fatalf("expected exactly 1 supplier_item row, got %d", supplierItemCount)
	}

	// --- Idempotency: a second run must not error, including through the
	// goods_receipt_note (IMMUTABLE) and stock_ledger_entry (append-only)
	// triggers, which would fire on an ON CONFLICT DO UPDATE but not on the
	// DO NOTHING this code uses for those two tables. ---
	runOnce("second run (idempotency)")

	if err := pool.QueryRow(ctx,
		`SELECT count(*) FROM goods_receipt_note WHERE id = $1`,
		"0191afff-0000-7000-8000-000000000060").Scan(&grnCount); err != nil {
		t.Fatalf("counting goods_receipt_note after second run: %v", err)
	}
	if grnCount != 1 {
		t.Fatalf("expected exactly 1 goods_receipt_note row after a second run, got %d", grnCount)
	}
}
