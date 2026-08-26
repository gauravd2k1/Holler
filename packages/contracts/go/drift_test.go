// Contract-drift test (ADR-008): fixtures under packages/contracts/fixtures/
// must round-trip identically through the Go representation. The mirrored
// TypeScript check lives in src/types/drift.test.ts.
package contracts

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func roundTrip(t *testing.T, fixture string, target interface{}) {
	t.Helper()
	path := filepath.Join("..", "fixtures", fixture)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("reading fixture %s: %v", fixture, err)
	}

	if err := json.Unmarshal(raw, target); err != nil {
		t.Fatalf("unmarshal %s: %v", fixture, err)
	}

	reMarshaled, err := json.Marshal(target)
	if err != nil {
		t.Fatalf("marshal %s: %v", fixture, err)
	}

	var original, roundTripped map[string]interface{}
	if err := json.Unmarshal(raw, &original); err != nil {
		t.Fatalf("unmarshal original %s for comparison: %v", fixture, err)
	}
	if err := json.Unmarshal(reMarshaled, &roundTripped); err != nil {
		t.Fatalf("unmarshal round-tripped %s for comparison: %v", fixture, err)
	}

	if !reflect.DeepEqual(original, roundTripped) {
		t.Fatalf("%s did not round-trip identically\noriginal:  %s\nroundtrip: %s", fixture, raw, reMarshaled)
	}
}

func TestOrderFixtureRoundTrip(t *testing.T) {
	var order CanonicalOrder
	roundTrip(t, "order.json", &order)
}

func TestKotFixtureRoundTrip(t *testing.T) {
	var kot Kot
	roundTrip(t, "kot.json", &kot)
}

func TestSyncEnvelopeAuthorityRule(t *testing.T) {
	// Mirrors the §50.1 authority check encoded in src/types/sync.ts.
	if AggregateAuthority[AggregateTypeOrder] != SyncDirectionEdgeToCloud {
		t.Fatalf("order aggregate must be EDGE_TO_CLOUD per §50.1")
	}
	if AggregateAuthority[AggregateTypeMenuItem] != SyncDirectionCloudToEdge {
		t.Fatalf("menu_item aggregate must be CLOUD_TO_EDGE per §50.1")
	}
}

func TestAppUserFixtureRoundTrip(t *testing.T) {
	var user AppUser
	roundTrip(t, "app_user.json", &user)
}

func TestRestaurantTableFixtureRoundTrip(t *testing.T) {
	var table RestaurantTable
	roundTrip(t, "restaurant_table.json", &table)
}

func TestTableSessionFixtureRoundTrip(t *testing.T) {
	var session TableSession
	roundTrip(t, "table_session.json", &session)
}

// ADR-011: a table's definition is config (cloud→edge); a seating is an
// operational transaction (edge→cloud). No aggregate is bidirectional.
func TestMilestone1AggregateAuthority(t *testing.T) {
	expected := map[AggregateType]SyncDirection{
		AggregateTypeTableSession:    SyncDirectionEdgeToCloud,
		AggregateTypeAppUser:         SyncDirectionCloudToEdge,
		AggregateTypeRole:            SyncDirectionCloudToEdge,
		AggregateTypeRestaurantTable: SyncDirectionCloudToEdge,
	}
	for aggregate, direction := range expected {
		if AggregateAuthority[aggregate] != direction {
			t.Fatalf("%s must be %s per ADR-011, got %s", aggregate, direction, AggregateAuthority[aggregate])
		}
	}
}

// Mirrors AUDIT_REDACTED_FIELDS in src/types/identity.ts. Credential material
// must never reach an audit_event value map or the wire (ADR-011).
func TestAuditRedactedFields(t *testing.T) {
	want := []string{"password_hash", "pin_hash", "token_hash", "device_token_hash", "credential_hash"}
	if !reflect.DeepEqual(AuditRedactedFields, want) {
		t.Fatalf("AuditRedactedFields drifted from TypeScript: got %v want %v", AuditRedactedFields, want)
	}
}

func TestMenuItemFixtureRoundTrip(t *testing.T) {
	var item MenuItem
	roundTrip(t, "menu_item.json", &item)
}

func TestMenuItemModifierFixtureRoundTrip(t *testing.T) {
	var modifier MenuItemModifier
	roundTrip(t, "menu_item_modifier.json", &modifier)
}

// Milestone 2 boundary-crossing tables (0.3.0, ADR-014). Each has a row in
// both stores, so a drifted shape breaks replay silently.

func TestStationFixtureRoundTrip(t *testing.T) {
	var station Station
	roundTrip(t, "station.json", &station)
}

func TestMenuItemStationFixtureRoundTrip(t *testing.T) {
	var routing MenuItemStation
	roundTrip(t, "menu_item_station.json", &routing)
}

func TestPrinterFixtureRoundTrip(t *testing.T) {
	var printer Printer
	roundTrip(t, "printer.json", &printer)
}

func TestStationPrinterFixtureRoundTrip(t *testing.T) {
	var routing StationPrinter
	roundTrip(t, "station_printer.json", &routing)
}

// 0.4.7. The fixture uses BILL deliberately: KITCHEN would round-trip even if
// the enum were mis-mirrored as a plain string, because station_printer
// already proves the join-row shape. BILL is the member that only exists
// because an invoice needs a print target.
func TestPrinterRoleFixtureRoundTrip(t *testing.T) {
	var role PrinterRole
	roundTrip(t, "printer_role.json", &role)
}

// Edge-local: SQLite only, no Postgres mirror, no wire route. Round-tripped
// anyway so the Go and TypeScript shapes cannot drift apart.
func TestPrintJobFixtureRoundTrip(t *testing.T) {
	var job PrintJob
	roundTrip(t, "print_job.json", &job)
}

// ADR-014: stations and printers are config (cloud→edge); the ticket at the
// station is not. The ADR-011 RestaurantTable/TableSession split, applied to
// the kitchen.
func TestMilestone2AggregateAuthority(t *testing.T) {
	expected := map[AggregateType]SyncDirection{
		AggregateTypeStation: SyncDirectionCloudToEdge,
		AggregateTypePrinter: SyncDirectionCloudToEdge,
		AggregateTypeKot:     SyncDirectionEdgeToCloud,
	}
	for aggregate, direction := range expected {
		if AggregateAuthority[aggregate] != direction {
			t.Fatalf("%s must be %s per ADR-014, got %s", aggregate, direction, AggregateAuthority[aggregate])
		}
	}
}

// print_job and kot_status_history are deliberately not aggregates — see
// printer.go and the refresh_token precedent. Adding either to
// AggregateAuthority gives it a sync direction it must not have.
func TestEdgeLocalTablesAreNotAggregates(t *testing.T) {
	for _, forbidden := range []AggregateType{"print_job", "kot_status_history", "refresh_token"} {
		if _, listed := AggregateAuthority[forbidden]; listed {
			t.Fatalf("%q must not be an AggregateType: it crosses no boundary", forbidden)
		}
	}
}

// Mirrors OUTBOX_EVENT_TYPES in src/types/events.ts, same order. The edge
// crates hold these as Rust literals with no compile-time link, so
// scripts/check-event-type-drift.mjs greps them against this list too.
func TestOutboxEventTypes(t *testing.T) {
	want := []string{
		"OrderCreated",
		"ItemAdded",
		"ItemRemoved",
		"ItemQuantityChanged",
		"OrderConfirmed",
		"KOTCreated",
		"KOTStatusChanged",
		"OrderReady",
		"SentToKitchen",
		"OrderCancelled",
		"TableSessionOpened",
		"TableSessionUpdated",
		// Milestone 3 billing (0.4.0, ADR-016).
		"InvoiceCreated",
		"PaymentReceived",
		"PaymentRefunded",
		"CashShiftOpened",
		"CashShiftClosed",
		// Milestone 4 (0.5.5): a completed stocktake is an individually
		// meaningful, low-volume fact, so it rides the outbox while the
		// ledger rides the entry_seq cursor.
		"StockCountOpened",
		"StockCountCompleted",
	}
	if !reflect.DeepEqual(OutboxEventTypes, want) {
		t.Fatalf("OutboxEventTypes drifted from TypeScript: got %v want %v", OutboxEventTypes, want)
	}
}

func TestAuditEventFixtureRoundTrip(t *testing.T) {
	var event AuditEvent
	roundTrip(t, "audit_event.json", &event)
}

// credentialBearingFixtures are the ONLY fixtures permitted to contain
// credential material (0.3.1, ADR-015). Naming them as exceptions rather than
// skipping the sweep keeps the rule enforceable: a second credential-bearing
// fixture fails until someone justifies it in an ADR.
var credentialBearingFixtures = map[string]bool{
	"edge_user_cache_entry.json":        true,
	"edge_user_cache_entry_no_pin.json": true,
	// Added at 0.4.3 and justified in the ADR-017 amendment, exactly as this
	// guard demands. The device credential hash syncs to an enrolled edge so a
	// LAN handshake can be verified with the uplink down — the ADR-011 pattern
	// applied to devices, for the same offline-first reason. It carries an
	// Argon2id VERIFIER, never a bearer token; see the assertion below.
	"edge_device_credential.json": true,
}

// The device carrier is held to the same standard the user carriers are. Its
// column is spelled token_hash, but the value must be an Argon2id verifier —
// something a presented token is checked against — never a replayable bearer
// token. If a plaintext token ever landed here, the hash prefix would be the
// first casualty.
func TestEdgeDeviceCredentialHoldsAVerifierNotAToken(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "fixtures", "edge_device_credential.json"))
	if err != nil {
		t.Fatalf("reading fixture: %v", err)
	}
	var cred EdgeDeviceCredential
	if err := json.Unmarshal(raw, &cred); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if !strings.HasPrefix(cred.CredentialHash, "$argon2id$") {
		t.Fatalf("credential_hash must be an Argon2id verifier, got %q", cred.CredentialHash)
	}
	for _, forbidden := range []string{"refresh_token", "access_token", "plaintext"} {
		if strings.Contains(string(raw), forbidden) {
			t.Fatalf("device credential fixture must not contain %q", forbidden)
		}
	}
}

func TestEdgeUserCacheEntryFixtureRoundTrip(t *testing.T) {
	var entry EdgeUserCacheEntry
	roundTrip(t, "edge_user_cache_entry.json", &entry)
	if entry.PinHash == nil {
		t.Fatal("pin_hash must survive the round trip as a non-nil value")
	}
	if !strings.HasPrefix(*entry.PinHash, "$argon2id$") {
		t.Fatalf("pin_hash must be an Argon2id encoded string, got %q", *entry.PinHash)
	}
	if !strings.HasPrefix(entry.PasswordHash, "$argon2id$") {
		t.Fatalf("password_hash must be an Argon2id encoded string, got %q", entry.PasswordHash)
	}
}

// A PIN pad is the primary offline login at a POS, so the null case is not an
// edge case — it is every user who has not set one. Nullable handling is also
// exactly where a mirror silently drops a field, so roundTrip's strict
// comparison (which fails if the key vanishes) is the point of this test.
func TestEdgeUserCacheEntryFixtureRoundTripWithoutPin(t *testing.T) {
	var entry EdgeUserCacheEntry
	roundTrip(t, "edge_user_cache_entry_no_pin.json", &entry)
	if entry.PinHash != nil {
		t.Fatalf("pin_hash must round-trip as nil, got %q", *entry.PinHash)
	}
}

// The exception is exactly as wide as it claims: verifiers only, never a
// bearer. A cache entry that could be replayed as a session would defeat the
// containment the whole design rests on.
func TestCredentialCarriersHoldVerifiersOnly(t *testing.T) {
	for fixture := range credentialBearingFixtures {
		raw, err := os.ReadFile(filepath.Join("..", "fixtures", fixture))
		if err != nil {
			t.Fatalf("reading fixture %s: %v", fixture, err)
		}
		for _, forbidden := range []string{"token_hash", "refresh_token", "access_token", "session"} {
			if strings.Contains(string(raw), forbidden) {
				t.Fatalf("%s contains bearer material %q — it may carry verifiers only", fixture, forbidden)
			}
		}
	}
}

func TestEdgeUserCacheIsNotAnAggregate(t *testing.T) {
	for _, forbidden := range []AggregateType{"edge_user_cache_entry", "app_user_credential"} {
		if _, listed := AggregateAuthority[forbidden]; listed {
			t.Fatalf("%q must not be an AggregateType: it never syncs up", forbidden)
		}
	}
}

// Sweeps every fixture except the named carriers, so a NEW fixture is covered
// automatically — the previous hard-coded four-name list did not do that.
func TestWireFixturesCarryNoCredentials(t *testing.T) {
	entries, err := os.ReadDir(filepath.Join("..", "fixtures"))
	if err != nil {
		t.Fatalf("reading fixtures directory: %v", err)
	}
	swept := 0
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasSuffix(name, ".json") || credentialBearingFixtures[name] {
			continue
		}
		raw, err := os.ReadFile(filepath.Join("..", "fixtures", name))
		if err != nil {
			t.Fatalf("reading fixture %s: %v", name, err)
		}
		for _, field := range AuditRedactedFields {
			if strings.Contains(string(raw), field) {
				t.Fatalf("%s contains credential field %q", name, field)
			}
		}
		swept++
	}
	if swept == 0 {
		t.Fatal("credential sweep matched no fixtures — the check is vacuous")
	}
}

// ----------------------------------------------------------------------------
// Milestone 3 billing (0.4.0, ADR-016)
// ----------------------------------------------------------------------------

func TestInvoiceFixtureRoundTrip(t *testing.T) {
	var invoice Invoice
	roundTrip(t, "invoice.json", &invoice)
}

func TestPaymentFixtureRoundTrip(t *testing.T) {
	var payment Payment
	roundTrip(t, "payment.json", &payment)
}

func TestCashShiftFixtureRoundTrip(t *testing.T) {
	var shift CashShift
	roundTrip(t, "cash_shift.json", &shift)
}

func TestTaxProfileFixtureRoundTrip(t *testing.T) {
	var profile TaxProfile
	roundTrip(t, "tax_profile.json", &profile)
}

// ADR-016: the outlet issues bills and takes money with the uplink down, so
// both are edge-authoritative and the cloud only replays. The rules governing
// them are management decisions, so those are cloud config — the same cut
// ADR-014 drew between a station's definition and the ticket at it.
func TestMilestone3AggregateAuthority(t *testing.T) {
	expected := map[AggregateType]SyncDirection{
		AggregateTypeInvoice:            SyncDirectionEdgeToCloud,
		AggregateTypeCashShift:          SyncDirectionEdgeToCloud,
		AggregateTypePayment:            SyncDirectionEdgeToCloud,
		AggregateTypeTaxProfile:         SyncDirectionCloudToEdge,
		AggregateTypeComplianceVersion:  SyncDirectionCloudToEdge,
		AggregateTypeInvoiceSeries:      SyncDirectionCloudToEdge,
		AggregateTypeDiscountDefinition: SyncDirectionCloudToEdge,
	}
	for aggregate, direction := range expected {
		if AggregateAuthority[aggregate] != direction {
			t.Fatalf("%s must be %s per §50.1 (ADR-016)", aggregate, direction)
		}
	}
}

// The invoice counter is edge-local. Giving it a sync direction would make the
// cloud a second writer of invoice numbers, which is exactly what §33's "never
// generate duplicate invoice numbers" forbids — the print_job precedent
// applied to numbering. Child rows are absent for the ordinary reason: they
// travel inside their parent's payload.
func TestBillingCounterAndChildRowsAreNotAggregates(t *testing.T) {
	for _, forbidden := range []AggregateType{
		"invoice_sequence",
		"invoice_line",
		"payment_allocation",
		"cash_movement",
		"tax_rule",
		"outlet_fiscal_profile",
	} {
		if _, exists := AggregateAuthority[forbidden]; exists {
			t.Fatalf("%s must not be an aggregate: it is edge-local or a child row (ADR-016)", forbidden)
		}
	}
}

// The ADR-016 rounding policy, asserted against the Go mirror. The same rule
// is a CHECK in both stores and a refine in Zod; this is the layer an ingest
// handler consults before writing, so that a malformed replay is refused with
// a reason rather than a driver-level constraint error.
func TestInvoiceRoundingPolicy(t *testing.T) {
	var valid Invoice
	roundTrip(t, "invoice.json", &valid)
	if !valid.SumsCorrectly() {
		t.Fatalf("the invoice fixture must satisfy the ADR-016 rounding policy")
	}

	cases := []struct {
		name   string
		mutate func(Invoice) Invoice
	}{
		{"grand total drifting from its parts", func(i Invoice) Invoice {
			i.GrandTotalPaise = 106000
			return i
		}},
		{"round-off absorbing an arithmetic error", func(i Invoice) Invoice {
			i.TaxableValuePaise, i.RoundOffPaise, i.GrandTotalPaise = 99940, 60, 105000
			return i
		}},
		{"a total that never settled in whole rupees", func(i Invoice) Invoice {
			i.TaxableValuePaise, i.GrandTotalPaise = 99999, 104999
			return i
		}},
	}
	for _, tc := range cases {
		if tc.mutate(valid).SumsCorrectly() {
			t.Fatalf("SumsCorrectly must reject: %s", tc.name)
		}
	}
}

// §39: a register closed without its count can never be reconciled afterwards,
// and a variance with no reason is an unexplained cash difference.
func TestCashShiftAccounting(t *testing.T) {
	var shift CashShift
	roundTrip(t, "cash_shift.json", &shift)
	if !shift.IsFullyAccounted() {
		t.Fatalf("the cash_shift fixture must be fully accounted")
	}

	missingCount := shift
	missingCount.ActualCashPaise = nil
	if missingCount.IsFullyAccounted() {
		t.Fatalf("a CLOSED shift with no counted cash must not be accepted")
	}

	noReason := shift
	noReason.VarianceReason = nil
	if noReason.IsFullyAccounted() {
		t.Fatalf("a non-zero variance with no reason must not be accepted (§39)")
	}
}

// EdgeDeviceCredential (0.4.3, ADR-017 amendment) — the second deliberate
// credential carrier after EdgeUserCacheEntry. Pinned because a mirror that
// silently drops token_hash would make offline LAN auth fail closed with no
// signal, and one that drops revoked_at would make a dead credential look live.
func TestEdgeDeviceCredentialFixtureRoundTrip(t *testing.T) {
	var cred EdgeDeviceCredential
	roundTrip(t, "edge_device_credential.json", &cred)
	if cred.CredentialHash == "" {
		t.Fatalf("credential_hash must survive the round trip; offline LAN auth depends on it")
	}
	if cred.RevokedAt != nil {
		t.Fatalf("fixture credential is live; revoked_at must be nil")
	}
}

// ---------------------------------------------------------------------------
// Milestone 4 — inventory and recipes (0.5.0, ADR-018)
// ---------------------------------------------------------------------------

func TestMilestone4FixturesRoundTrip(t *testing.T) {
	var item InventoryItem
	roundTrip(t, "inventory_item.json", &item)
	var conv ItemUnitConversion
	roundTrip(t, "item_unit_conversion.json", &conv)
	var recipe Recipe
	roundTrip(t, "recipe.json", &recipe)
	var ingredient RecipeIngredient
	roundTrip(t, "recipe_ingredient.json", &ingredient)
	var delta ModifierIngredientDelta
	roundTrip(t, "modifier_ingredient_delta.json", &delta)
	var entry StockLedgerEntry
	roundTrip(t, "stock_ledger_entry.json", &entry)
	// A SECOND ledger fixture, populated where the first is null. The first
	// carries a RECIPE deduction, so every count-provenance field on it is
	// null — and a field absent from the struct round-trips a null perfectly.
	// That is exactly how source_stock_count_id crossed two milestones on the
	// wire unnoticed: green on absent data, in the test written to catch it
	// (contracts 0.5.9). Any provenance group added later needs its own row
	// here for the same reason.
	var adjustment StockLedgerEntry
	roundTrip(t, "stock_ledger_entry_count_adjustment.json", &adjustment)
	if adjustment.SourceStockCountID == nil {
		t.Fatal("stock_ledger_entry.source_stock_count_id was dropped on decode: a " +
			"COUNT_ADJUSTMENT that loses the count it came from is an append-only row " +
			"whose provenance is gone permanently (contracts 0.5.9, ADR-018)")
	}
	var count StockCount
	roundTrip(t, "stock_count.json", &count)
	var line StockCountLine
	roundTrip(t, "stock_count_line.json", &line)
	var gap StockDeductionGap
	roundTrip(t, "stock_deduction_gap.json", &gap)
}

// ADR-018: the same cut every milestone before it drew. A raw material's
// definition and a recipe are management decisions; consuming, wasting and
// counting stock are shop-floor transactions the outlet performs with the
// uplink down.
func TestMilestone4AggregateAuthority(t *testing.T) {
	expected := map[AggregateType]SyncDirection{
		AggregateTypeInventoryItem:     SyncDirectionCloudToEdge,
		AggregateTypeRecipe:            SyncDirectionCloudToEdge,
		AggregateTypeStockLedgerEntry:  SyncDirectionEdgeToCloud,
		AggregateTypeStockCount:        SyncDirectionEdgeToCloud,
		AggregateTypeStockDeductionGap: SyncDirectionEdgeToCloud,
	}
	for aggregate, direction := range expected {
		if AggregateAuthority[aggregate] != direction {
			t.Fatalf("%s must be %s per §50.1 (ADR-018)", aggregate, direction)
		}
	}
}

// stock_balance_snapshot is edge-local: the cloud MAY re-derive its own stock
// view by summing the ingested ledger, but mirroring the edge's projection
// would make it a second authority on stock — the same mistake mirroring
// invoice_sequence would make about invoice numbers. The rest are child rows,
// absent for the ordinary reason: they travel inside their parent's payload.
func TestInventoryProjectionAndChildRowsAreNotAggregates(t *testing.T) {
	for _, forbidden := range []AggregateType{
		"stock_balance_snapshot",
		"item_unit_conversion",
		"recipe_ingredient",
		"modifier_ingredient_delta",
		"stock_count_line",
	} {
		if _, exists := AggregateAuthority[forbidden]; exists {
			t.Fatalf("%s must not be an aggregate: it is edge-local or a child row (ADR-018)", forbidden)
		}
	}
}

// The deferred columns must stay INERT in M4. yield_factor_ppm defaults to the
// identity and nothing reads it; unit_cost_paise is null until M5. Pinned by
// exact assertion, the synthesized-canonical-field precedent — the day either
// starts carrying real data, this fails and says so.
func TestMilestone4DeferredFieldsAreInert(t *testing.T) {
	var item InventoryItem
	roundTrip(t, "inventory_item.json", &item)
	if item.YieldFactorPPM != YieldFactorPPMIdentity {
		t.Fatalf("inventory_item.yield_factor_ppm must be the identity %d in M4, got %d (ADR-018 §8)",
			YieldFactorPPMIdentity, item.YieldFactorPPM)
	}

	var ingredient RecipeIngredient
	roundTrip(t, "recipe_ingredient.json", &ingredient)
	if ingredient.YieldFactorPPM != YieldFactorPPMIdentity {
		t.Fatalf("recipe_ingredient.yield_factor_ppm must be the identity %d in M4, got %d (ADR-018 §8)",
			YieldFactorPPMIdentity, ingredient.YieldFactorPPM)
	}

	var entry StockLedgerEntry
	roundTrip(t, "stock_ledger_entry.json", &entry)
	if entry.UnitCostPaise != nil {
		t.Fatalf("stock_ledger_entry.unit_cost_paise lands in M5 and must be null in M4, got %d (ADR-018 §8)",
			*entry.UnitCostPaise)
	}
}

// Tier 1 is physics and lives in code; Tier 2 is per-item packaging and lives
// in item_unit_conversion. The two sets must stay disjoint: two sources of
// truth for kg→g would need a silent precedence rule between disagreeing
// numbers, which is how a deduction becomes quietly wrong.
//
// Cross-dimension conversion is deliberately absent here — density varies per
// ingredient, so g↔ml is not a physical constant.
func TestDimensionalConversionsAreWithinDimensionOnly(t *testing.T) {
	expected := map[string]DimensionalConversion{
		"mg":    {Dimension: DimensionMass, Micro: 1_000},
		"g":     {Dimension: DimensionMass, Micro: 1_000_000},
		"kg":    {Dimension: DimensionMass, Micro: 1_000_000_000},
		"ml":    {Dimension: DimensionVolume, Micro: 1_000},
		"l":     {Dimension: DimensionVolume, Micro: 1_000_000},
		"piece": {Dimension: DimensionCount, Micro: 1_000_000},
		"dozen": {Dimension: DimensionCount, Micro: 12_000_000},
	}
	if len(DimensionalConversions) != len(expected) {
		t.Fatalf("DimensionalConversions has %d entries, expected %d — a new one must be a physical constant, not a per-item property",
			len(DimensionalConversions), len(expected))
	}
	for unit, want := range expected {
		got, ok := DimensionalConversions[unit]
		if !ok {
			t.Fatalf("DimensionalConversions is missing %q", unit)
		}
		if got != want {
			t.Fatalf("DimensionalConversions[%q] = %+v, want %+v — these are physical constants and must match the TypeScript mirror exactly",
				unit, got, want)
		}
	}
}

// M4 adds three permissions and one that rides along. wastage.approve is
// deliberately absent: the approval workflow moves to M5 with the append-only
// approval row that enforces it, and an unused permission is a documented
// obligation dressed as structural enforcement.
func TestMilestone4Permissions(t *testing.T) {
	for _, required := range []Permission{
		PermissionInventoryManage,
		PermissionInventoryCount,
		PermissionRecipeManage,
		PermissionBillingManage,
	} {
		if required == "" {
			t.Fatalf("permission constant is empty")
		}
	}
	if Permission("wastage.approve") == PermissionInventoryManage {
		t.Fatalf("wastage.approve must not exist in 0.5.0 (ADR-018 §11)")
	}
}
