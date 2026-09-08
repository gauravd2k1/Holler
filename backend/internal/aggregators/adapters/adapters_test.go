package adapters_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/holler/backend/internal/aggregators"
	"github.com/holler/backend/internal/aggregators/adapters/beckn"
	"github.com/holler/backend/internal/aggregators/adapters/syncrest"
)

// M6 Phase C. THE POINT OF THIS FILE IS THE PAIR, NOT EITHER ADAPTER.
//
// NOTE THE FIXTURE THESE USE: `confirm.json`, not `on_confirm.json`. We are the
// SELLER (BPP), so a buyer app sends us `confirm` and we answer `on_confirm`.
// An earlier version of this test fed `on_confirm` — a message we SEND — into
// the inbound path, and it passed, because ONDC's request and callback
// envelopes share `message.order`. Right about the JSON, wrong about the
// direction, and no test built from the same assumption could have caught it.
//
// Both adapters are driven through the SAME aggregators.Adapter interface, with
// the same assertions, from payloads that look nothing alike: one an ONDC
// context/message envelope with decimal rupee STRINGS, the other flat JSON with
// integer paise. If both satisfy the same table without the test needing to
// know which is which, the abstraction carries both shapes — which is the only
// thing a second adapter is for.
//
// A one-adapter abstraction is indistinguishable from no abstraction, and the
// way that failure hides is a test suite that only ever exercises one.

func loadFixture(t *testing.T, name string) []byte {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("beckn", "fixtures", name))
	if err != nil {
		t.Fatalf("reading fixture %s: %v", name, err)
	}
	return raw
}

// resolveKnown maps exactly one platform item and returns nil for everything
// else — so every test here exercises the unmapped path as well as the mapped
// one, rather than treating "nothing resolves" as a separate edge case.
func resolveKnown(known, menuItemID string) func(context.Context, string) (*string, error) {
	return func(_ context.Context, externalItemID string) (*string, error) {
		if externalItemID == known {
			id := menuItemID
			return &id, nil
		}
		return nil, nil
	}
}

// The sync-REST fake. A WORKING FAKE, not a stub: a real HTTP server the
// adapter really talks to. A stub would prove the interface compiles, which was
// never in doubt.
func syncRestFake(t *testing.T, onState func(body map[string]any)) *httptest.Server {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/orders/state", func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if onState != nil {
			onState(body)
		}
		// SYNCHRONOUS: the platform's decision is in this response. The Beckn
		// adapter gets its answer minutes later on a separate callback. Neither
		// difference reaches the core.
		w.Header().Set("content-type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"accepted":true,"status":"CONFIRMED"}`))
	})
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	return server
}

func syncRestPayload() []byte {
	// Deliberately nothing like the Beckn envelope: flat, paise on the wire,
	// its own vocabulary for every field.
	return []byte(`{
	  "order_ref": "SR-90210",
	  "event_id": "evt-7f3a",
	  "status": "PLACED",
	  "revision": 3,
	  "placed_at": "2026-09-08T12:20:00Z",
	  "total_paise": 13887,
	  "items": [
	    {"sku": "I1", "name": "Masala Chai", "qty": 2, "price_paise": 4500},
	    {"sku": "SKU-UNKNOWN", "name": "Unmappable Regional Special", "qty": 1, "price_paise": 887}
	  ]
	}`)
}

// ONE TABLE, BOTH ADAPTERS. The assertions do not branch on which adapter is
// under test; they cannot, because the interface gives them nothing to branch
// on.
func TestBothAdaptersSatisfyTheSameContract(t *testing.T) {
	ctx := context.Background()
	server := syncRestFake(t, nil)

	cases := []struct {
		name               string
		adapter            aggregators.Adapter
		raw                []byte
		wantExternalID     string
		wantStatus         string
		wantLineCount      int
		wantFirstUnitPaise int64
		wantTotalPaise     int64
	}{
		{
			name:           "asynchronous callback platform",
			adapter:        beckn.New(resolveKnown("I1", "menu-item-1"), func(context.Context, []byte) error { return nil }),
			raw:            loadFixture(t, "confirm.json"),
			wantExternalID: "O1",
			// "Created", not "Accepted": a buyer-originated `confirm` states the
			// order as the BUYER sees it. "Accepted" is what WE would put in the
			// on_confirm we send back. The two fixtures differing here is the
			// direction mistake showing up in the data.
			wantStatus:         "Created",
			wantLineCount:      2,
			wantFirstUnitPaise: 4500,
			wantTotalPaise:     13887,
		},
		{
			name:               "synchronous REST platform",
			adapter:            syncrest.New(server.URL, server.Client(), resolveKnown("I1", "menu-item-1")),
			raw:                syncRestPayload(),
			wantExternalID:     "SR-90210",
			wantStatus:         "PLACED",
			wantLineCount:      2,
			wantFirstUnitPaise: 4500,
			wantTotalPaise:     13887,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := tc.adapter.ParseInbound(ctx, tc.raw)
			if err != nil {
				t.Fatalf("ParseInbound: %v", err)
			}

			if got.ExternalOrderID != tc.wantExternalID {
				t.Errorf("external order id = %q, want %q", got.ExternalOrderID, tc.wantExternalID)
			}
			// VERBATIM. Each platform's own word for its state, unmapped. A
			// status coerced into a shared vocabulary here is how a state we
			// have never seen becomes one we think we understand.
			if got.PlatformStatus != tc.wantStatus {
				t.Errorf("platform status = %q, want %q verbatim", got.PlatformStatus, tc.wantStatus)
			}
			if got.MessageID == "" {
				t.Error("no message id: without one a platform retry produces a second document")
			}
			if got.RawPayload == nil {
				t.Error("raw payload was not retained; a mapping bug is unfalsifiable after the fact without it")
			}
			if len(got.Lines) != tc.wantLineCount {
				t.Fatalf("lines = %d, want %d", len(got.Lines), tc.wantLineCount)
			}

			// THE MAPPED LINE.
			if got.Lines[0].MenuItemID == nil || *got.Lines[0].MenuItemID != "menu-item-1" {
				t.Errorf("line 1 menu_item_id = %v, want menu-item-1", got.Lines[0].MenuItemID)
			}
			// THE UNMAPPED LINE, AND IT MUST SURVIVE. ADR-022 rule 4: a line
			// that cannot be mapped is RECORDED, NOT REFUSED. Refusing a
			// delivery order that is already cooking is the outage, not the
			// protection.
			if got.Lines[1].MenuItemID != nil {
				t.Errorf("line 2 resolved to %v, but nothing local matches it", *got.Lines[1].MenuItemID)
			}
			if got.Lines[1].ExternalItemName == "" {
				t.Error("an unmapped line lost its platform name — that name is the only description of what the customer ordered")
			}

			// MONEY. Both arrive as 4500 paise per unit and 53650 total, from
			// completely different wire representations: a decimal rupee STRING
			// on one side, integer paise on the other.
			if got.Lines[0].StatedUnitPricePaise == nil || *got.Lines[0].StatedUnitPricePaise != tc.wantFirstUnitPaise {
				t.Errorf("line 1 unit price = %v paise, want %d", got.Lines[0].StatedUnitPricePaise, tc.wantFirstUnitPaise)
			}
			if got.StatedTotalPaise == nil || *got.StatedTotalPaise != tc.wantTotalPaise {
				t.Errorf("order total = %v paise, want %d", got.StatedTotalPaise, tc.wantTotalPaise)
			}

			// And both accept an outbound state change through the same call,
			// despite one answering synchronously and the other not answering
			// here at all.
			if err := tc.adapter.PushOrderState(ctx, aggregators.OrderStateChange{
				ExternalOrderID: tc.wantExternalID,
				LocalState:      "READY",
				OccurredAt:      "2026-09-08T12:30:00Z",
			}); err != nil {
				t.Errorf("PushOrderState: %v", err)
			}
		})
	}
}

// THE MONEY CONVERSION, ON ITS OWN, BECAUSE IT IS A MONEY PATH.
//
// ONDC prices are decimal STRINGS in rupees; this product stores integer paise.
// `strconv.ParseFloat(v, 64) * 100` gives 53649.999999999993 for "536.50" — a
// value that rounds correctly most of the time, which is what makes it
// dangerous. The fixture carries a fractional price precisely so a float
// implementation cannot pass.
func TestBecknRupeeStringsConvertWithoutFloatDrift(t *testing.T) {
	ctx := context.Background()
	adapter := beckn.New(resolveKnown("I1", "menu-item-1"), func(context.Context, []byte) error { return nil })

	got, err := adapter.ParseInbound(ctx, loadFixture(t, "confirm.json"))
	if err != nil {
		t.Fatalf("ParseInbound: %v", err)
	}

	// THE FIXTURE VALUES ARE CHOSEN, NOT ARBITRARY, AND THAT MATTERS.
	//
	// An earlier version of this test used "406.50" and asserted it converted to
	// 40650 — and a deliberately-injected float implementation PASSED, because
	// 406.5 * 100 is 40650.000000000006 and truncates to 40650. The test claimed
	// a float could not pass and was wrong: most values survive the round trip,
	// which is exactly what makes the bug ship.
	//
	// "8.87" does not survive it: 8.87 * 100 is 886.9999999999999, which
	// truncates to 886 — one paisa lost on that line, silently, on every order
	// carrying it.
	//
	// The TOTAL of "138.87" was measured too and it converts correctly under
	// both implementations, so it is asserted for correctness and is NOT
	// carrying the falsification. Saying which assertion does the work is the
	// point: this test previously claimed a property it did not have.
	if got.Lines[1].StatedUnitPricePaise == nil {
		t.Fatal("fractional price did not convert at all")
	}
	if v := *got.Lines[1].StatedUnitPricePaise; v != 887 {
		t.Errorf("fractional price converted to %d paise, want 887 (a float parse yields 886)", v)
	}
	if got.StatedTotalPaise == nil || *got.StatedTotalPaise != 13887 {
		t.Errorf("total converted to %v paise, want 13887", got.StatedTotalPaise)
	}
}

// A retry must be recognisable as one. Both platforms retry; neither may
// produce a second document.
func TestBothAdaptersRefuseAMessageWithNoIdempotencyKey(t *testing.T) {
	ctx := context.Background()

	if _, err := beckn.New(nil, nil).ParseInbound(ctx, []byte(`{"context":{},"message":{"order":{"id":"O1"}}}`)); err == nil {
		t.Error("a callback with no message id must be refused: without it a retry becomes a duplicate order")
	}
	if _, err := syncrest.New("", nil, nil).ParseInbound(ctx, []byte(`{"order_ref":"SR-1"}`)); err == nil {
		t.Error("an order with no event id must be refused for the same reason")
	}
}

// The synchronous adapter really talks to the fake, and the fake really
// receives our vocabulary mapped into ITS vocabulary — in the adapter, where
// that mapping belongs.
func TestSyncRestPushReachesThePlatformInItsOwnVocabulary(t *testing.T) {
	var seen map[string]any
	server := syncRestFake(t, func(body map[string]any) { seen = body })

	adapter := syncrest.New(server.URL, server.Client(), nil)
	if err := adapter.PushOrderState(context.Background(), aggregators.OrderStateChange{
		ExternalOrderID: "SR-90210",
		LocalState:      "READY",
		OccurredAt:      "2026-09-08T12:30:00Z",
	}); err != nil {
		t.Fatalf("PushOrderState: %v", err)
	}

	if seen == nil {
		t.Fatal("the fake platform received nothing")
	}
	// Our "READY" became the platform's "READY_FOR_PICKUP". The Beckn adapter
	// maps the same local state to an entirely different string, which is what
	// makes having two adapters worth anything.
	if seen["status"] != "READY_FOR_PICKUP" {
		t.Errorf("platform received status %v, want READY_FOR_PICKUP", seen["status"])
	}
}

// An unimplemented adapter FAILS LOUDLY. It never returns success, never
// returns empty, and is never selectable by default: an empty order list from a
// platform that was never called is indistinguishable from a quiet Tuesday, and
// the first person to notice is a restaurant wondering where its orders went.
func TestAnUnconfiguredAdapterRefusesRatherThanSucceedingQuietly(t *testing.T) {
	err := syncrest.New("", nil, nil).PushOrderState(context.Background(), aggregators.OrderStateChange{})
	if err == nil {
		t.Fatal("an adapter with nowhere to send returned success")
	}
	if err != aggregators.ErrPlatformNotImplemented {
		t.Errorf("got %v, want ErrPlatformNotImplemented", err)
	}
}
