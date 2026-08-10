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
	want := []string{"password_hash", "pin_hash", "token_hash"}
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
		"OrderConfirmed",
		"KOTCreated",
		"KOTStatusChanged",
		"OrderReady",
		"SentToKitchen",
		"OrderCancelled",
		"TableSessionOpened",
		"TableSessionUpdated",
	}
	if !reflect.DeepEqual(OutboxEventTypes, want) {
		t.Fatalf("OutboxEventTypes drifted from TypeScript: got %v want %v", OutboxEventTypes, want)
	}
}

func TestAuditEventFixtureRoundTrip(t *testing.T) {
	var event AuditEvent
	roundTrip(t, "audit_event.json", &event)
}

func TestWireFixturesCarryNoCredentials(t *testing.T) {
	for _, fixture := range []string{"app_user.json", "order.json", "table_session.json", "audit_event.json"} {
		raw, err := os.ReadFile(filepath.Join("..", "fixtures", fixture))
		if err != nil {
			t.Fatalf("reading fixture %s: %v", fixture, err)
		}
		for _, field := range AuditRedactedFields {
			if strings.Contains(string(raw), field) {
				t.Fatalf("%s contains credential field %q", fixture, field)
			}
		}
	}
}
