// Contract-drift test (ADR-008): fixtures under packages/contracts/fixtures/
// must round-trip identically through the Go representation. The mirrored
// TypeScript check lives in src/types/drift.test.ts.
package contracts

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
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
