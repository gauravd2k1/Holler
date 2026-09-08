package beckn_test

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/holler/backend/internal/aggregators/adapters/beckn"
)

// The Beckn message surface, driven by ONDC's own published payloads.
//
// EVERY FIXTURE HERE IS DERIVED FROM ONDC-RET-Specifications release-2.0.2, at
// the commit pinned in fixtures/SOURCES.md — not from our reading of the prose.
// A fake we author from the spec proves only that we agree with ourselves, and
// this is the one place where no external check exists until certification.
//
// WHAT THESE PROVE: SHAPE. Not integration. No message here has ever been
// exchanged with an ONDC system, and M6 C8 is recorded as SHAPE ONLY for exactly
// that reason.

func fixture(t *testing.T, name string) []byte {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("fixtures", name))
	if err != nil {
		t.Fatalf("reading %s: %v", name, err)
	}
	return raw
}

// EVERY ACTION IS ANSWERED, AND THE TABLE IS ENUMERATED FROM THE SURFACE
// ITSELF rather than restated here. A test that lists the actions it expects
// passes when a new action is added and never wired — the missing one is
// exactly what you would want to hear about.
func TestEverySupportedActionHasACallback(t *testing.T) {
	actions := beckn.SupportedActions()
	if len(actions) != 7 {
		t.Fatalf("the surface carries %d actions, want the 7 the plan names", len(actions))
	}
	for _, a := range actions {
		callback, ok := beckn.CallbackAction(a)
		if !ok {
			t.Errorf("action %q has no callback: a BAP that sends it waits forever", a)
			continue
		}
		if callback != "on_"+string(a) {
			t.Errorf("action %q is answered by %q, want on_%s", a, callback, a)
		}
	}
}

// The three published payloads parse, and each is recognised as its own action.
func TestPublishedPayloadsParseAsTheirOwnAction(t *testing.T) {
	cases := []struct {
		file string
		want beckn.InboundAction
	}{
		{"confirm.json", beckn.ActionConfirm},
		{"cancel.json", beckn.ActionCancel},
		{"status.json", beckn.ActionStatus},
	}
	for _, tc := range cases {
		t.Run(tc.file, func(t *testing.T) {
			_, action, err := beckn.ParseEnvelope(fixture(t, tc.file))
			if err != nil {
				t.Fatalf("ParseEnvelope: %v", err)
			}
			if action != tc.want {
				t.Errorf("parsed as %q, want %q", action, tc.want)
			}
		})
	}
}

// CANCEL AND STATUS DO NOT CARRY AN ORDER DOCUMENT, AND THAT IS THE POINT.
//
// Code written against `message.order` for every action fails on exactly these
// two — and one of them is a cancellation. A cancellation nobody processes is a
// rider dispatched to collect food for a customer who cancelled.
func TestCancelAndStatusCarryAnIdRatherThanAnOrder(t *testing.T) {
	env, action, err := beckn.ParseEnvelope(fixture(t, "cancel.json"))
	if err != nil {
		t.Fatalf("ParseEnvelope: %v", err)
	}
	if beckn.ActionCarriesOrder(action) {
		t.Fatal("cancel must not be treated as carrying an order document")
	}
	cancel, err := beckn.ParseCancel(env.Message)
	if err != nil {
		t.Fatalf("ParseCancel: %v", err)
	}
	if cancel.OrderID != "O1" {
		t.Errorf("cancel order_id = %q, want O1", cancel.OrderID)
	}
	if cancel.CancellationReasonID == "" {
		t.Error("the cancellation reason was dropped; it is the only explanation anyone gets")
	}

	env, action, err = beckn.ParseEnvelope(fixture(t, "status.json"))
	if err != nil {
		t.Fatalf("ParseEnvelope(status): %v", err)
	}
	if beckn.ActionCarriesOrder(action) {
		t.Fatal("status must not be treated as carrying an order document")
	}
	if st, err := beckn.ParseStatus(env.Message); err != nil || st.OrderID != "O1" {
		t.Errorf("ParseStatus = %+v, %v; want order_id O1", st, err)
	}
}

// Routing a non-order action through the order parser must FAIL LOUDLY rather
// than yield an empty order.
//
// An empty order from a cancellation is the worst available outcome: it looks
// like a valid message with nothing in it, and the natural handling of "an
// order with no lines" is to ignore it.
func TestTheOrderParserRefusesANonOrderAction(t *testing.T) {
	adapter := beckn.New(nil, nil)
	if _, err := adapter.ParseInbound(context.Background(), fixture(t, "cancel.json")); err == nil {
		t.Fatal("cancel parsed as an order; an empty order from a cancellation reads as a message worth ignoring")
	}
}

// THE CALLBACK'S CONTEXT: what changes, what must not.
//
// The participant ids do NOT swap. `bap_*` stays the buyer's and `bpp_*` stays
// ours in both directions — they identify the participants, not the sender.
// Swapping them is a plausible mistake that would make every callback
// unroutable, and it would look symmetric and correct in review.
func TestCallbackKeepsParticipantsAndTransactionButMintsANewMessageID(t *testing.T) {
	env, action, err := beckn.ParseEnvelope(fixture(t, "confirm.json"))
	if err != nil {
		t.Fatalf("ParseEnvelope: %v", err)
	}

	raw, err := beckn.BuildCallback(env.Context, action, "new-message-id",
		time.Date(2026, 9, 8, 12, 16, 0, 0, time.UTC), map[string]any{"order": map[string]any{"id": "O1"}})
	if err != nil {
		t.Fatalf("BuildCallback: %v", err)
	}

	var out beckn.Envelope
	if err := json.Unmarshal(raw, &out); err != nil {
		t.Fatalf("decoding callback: %v", err)
	}

	if out.Context.Action != "on_confirm" {
		t.Errorf("callback action = %q, want on_confirm", out.Context.Action)
	}
	if out.Context.TransactionID != env.Context.TransactionID {
		t.Error("transaction_id changed; the BAP cannot correlate the callback with its request")
	}
	if out.Context.MessageID == env.Context.MessageID {
		t.Error("message_id was reused; a callback is its own message and needs its own id")
	}
	// The two that are easy to get wrong.
	if out.Context.BapID != env.Context.BapID || out.Context.BapURI != env.Context.BapURI {
		t.Error("bap_* changed: participant ids identify the participants, not the sender — swapping them makes the callback unroutable")
	}
	if out.Context.BppID != env.Context.BppID {
		t.Error("bpp_* changed: same reason")
	}
}

// An ACK is not an answer, and the distinction is asserted so nobody collapses
// them later. Treating an ACK as success reports orders confirmed that a seller
// never saw.
func TestAckAndNackAreDistinguishable(t *testing.T) {
	ack, err := json.Marshal(beckn.NewAck())
	if err != nil {
		t.Fatalf("marshal ack: %v", err)
	}
	nack, err := json.Marshal(beckn.NewNack("30001", "provider not found"))
	if err != nil {
		t.Fatalf("marshal nack: %v", err)
	}

	var a, n struct {
		Message struct {
			Ack struct {
				Status string `json:"status"`
			} `json:"ack"`
		} `json:"message"`
		Error *struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.Unmarshal(ack, &a); err != nil {
		t.Fatalf("decode ack: %v", err)
	}
	if err := json.Unmarshal(nack, &n); err != nil {
		t.Fatalf("decode nack: %v", err)
	}

	if a.Message.Ack.Status != "ACK" || a.Error != nil {
		t.Errorf("ACK = %+v, want status ACK with no error", a)
	}
	if n.Message.Ack.Status != "NACK" || n.Error == nil || n.Error.Code == "" {
		t.Errorf("NACK = %+v, want status NACK carrying an error code", n)
	}
}

// An envelope with no message_id is refused, on every action.
//
// It is the idempotency key: platforms retry on any doubt, and without it a
// retry becomes a second order.
func TestAnEnvelopeWithNoMessageIDIsRefused(t *testing.T) {
	raw := []byte(`{"context":{"action":"confirm","transaction_id":"t1"},"message":{}}`)
	if _, _, err := beckn.ParseEnvelope(raw); err == nil {
		t.Fatal("an envelope with no message_id was accepted; a platform retry would become a second order")
	}
}

// An unknown action is refused rather than answered with the nearest callback.
//
// A BAP waiting on one callback that receives another has an order in an
// unknown state, which is worse than an order it knows failed.
func TestAnUnknownActionIsRefused(t *testing.T) {
	raw := []byte(`{"context":{"action":"rate","transaction_id":"t1","message_id":"m1"},"message":{}}`)
	if _, _, err := beckn.ParseEnvelope(raw); err == nil {
		t.Fatal("an unsupported action was accepted; answering the wrong callback leaves the order in an unknown state")
	}
}
