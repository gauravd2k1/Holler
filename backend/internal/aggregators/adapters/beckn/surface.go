package beckn

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// The Beckn message surface (M6 Phase C).
//
// WHICH SIDE WE ARE ON, AND IT IS THE OPPOSITE OF WHAT THE FIRST CUT ASSUMED.
// A restaurant is the SELLER — the BPP. So the buyer app (BAP) sends us
// `search`, `select`, `init`, `confirm`, `status`, `cancel` and `update`, and we
// answer each with its `on_*` callback.
//
// The first version of this adapter parsed `on_confirm` as an inbound order,
// which is the BUYER's view: `on_confirm` is what a SELLER sends. It happened to
// work, because ONDC's request and callback envelopes share `message.order` --
// so the fixture parsed cleanly and nothing failed. That is exactly the shape of
// defect worth stating out loud: the code was right about the JSON and wrong
// about the direction, and no test built from the same assumption could have
// caught it.
//
// EVERY ACTION IS ASYNCHRONOUS. Beckn's request/callback split is the whole
// reason this adapter is the interesting half of the pair: the BAP posts an
// action, we ACK immediately, and the real answer goes back later as a separate
// `on_*` POST to the BAP's own URI. Nothing in the core knows that -- the sync
// adapter answers in its response body and satisfies the same interface.

// InboundAction is a Beckn action a BAP sends to us.
type InboundAction string

const (
	ActionSearch  InboundAction = "search"
	ActionSelect  InboundAction = "select"
	ActionInit    InboundAction = "init"
	ActionConfirm InboundAction = "confirm"
	ActionStatus  InboundAction = "status"
	ActionCancel  InboundAction = "cancel"
	ActionUpdate  InboundAction = "update"
)

// callbackFor maps each inbound action to the callback it is answered with.
//
// A TABLE, NOT A SWITCH IN SEVEN PLACES. The pairing is a fact about the
// protocol, and stating it once means a missing callback is a missing map entry
// rather than a branch somebody forgot to add.
var callbackFor = map[InboundAction]string{
	ActionSearch:  "on_search",
	ActionSelect:  "on_select",
	ActionInit:    "on_init",
	ActionConfirm: "on_confirm",
	ActionStatus:  "on_status",
	ActionCancel:  "on_cancel",
	ActionUpdate:  "on_update",
}

// SupportedActions is the surface this adapter implements, for the fake and for
// tests to enumerate rather than restate.
func SupportedActions() []InboundAction {
	return []InboundAction{
		ActionSearch, ActionSelect, ActionInit, ActionConfirm,
		ActionStatus, ActionCancel, ActionUpdate,
	}
}

// CallbackAction returns the `on_*` that answers an action.
func CallbackAction(a InboundAction) (string, bool) {
	c, ok := callbackFor[a]
	return c, ok
}

// Envelope is the ONDC context wrapper, mirroring release-2.0.2. See
// fixtures/SOURCES.md for the artefacts and the commit these were pinned at.
type Envelope struct {
	Context Context         `json:"context"`
	Message json.RawMessage `json:"message"`
}

type Context struct {
	Domain        string `json:"domain"`
	Action        string `json:"action"`
	Version       string `json:"version"`
	BapID         string `json:"bap_id"`
	BapURI        string `json:"bap_uri"`
	BppID         string `json:"bpp_id"`
	BppURI        string `json:"bpp_uri"`
	TransactionID string `json:"transaction_id"`
	MessageID     string `json:"message_id"`
	Timestamp     string `json:"timestamp"`
	TTL           string `json:"ttl"`
}

// Ack is the immediate synchronous reply to any inbound action.
//
// IT IS NOT THE ANSWER. Beckn's ACK says only "received and will be processed";
// the answer follows as a separate `on_*` POST, possibly seconds or minutes
// later. Treating an ACK as success is how an integration reports orders
// confirmed that a seller never saw.
type Ack struct {
	Message struct {
		Ack struct {
			Status string `json:"status"`
		} `json:"ack"`
	} `json:"message"`
	Error *AckError `json:"error,omitempty"`
}

type AckError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func NewAck() Ack {
	var a Ack
	a.Message.Ack.Status = "ACK"
	return a
}

// NewNack refuses a message. Beckn's own vocabulary: NACK means the message was
// not accepted for processing at all, which is different from accepting it and
// answering unfavourably later.
func NewNack(code, message string) Ack {
	var a Ack
	a.Message.Ack.Status = "NACK"
	a.Error = &AckError{Code: code, Message: message}
	return a
}

// ParseEnvelope validates the parts of the context every action shares.
//
// WHAT IT REFUSES AND WHY. A missing `message_id` is refused because it is the
// idempotency key: without it a retry -- and platforms retry on any doubt --
// produces a second document. An unknown action is refused because answering
// the wrong callback is worse than answering none: a BAP waiting on `on_confirm`
// that receives `on_status` has an order in an unknown state.
func ParseEnvelope(raw []byte) (Envelope, InboundAction, error) {
	var env Envelope
	if err := json.Unmarshal(raw, &env); err != nil {
		return Envelope{}, "", fmt.Errorf("beckn: decoding envelope: %w", err)
	}
	if env.Context.MessageID == "" {
		return Envelope{}, "", fmt.Errorf("beckn: envelope carries no message_id")
	}
	if env.Context.TransactionID == "" {
		// The transaction id threads every message of one order together. A
		// callback without it cannot be correlated by the BAP.
		return Envelope{}, "", fmt.Errorf("beckn: envelope carries no transaction_id")
	}
	action := InboundAction(strings.ToLower(env.Context.Action))
	if _, ok := callbackFor[action]; !ok {
		return Envelope{}, "", fmt.Errorf("beckn: unsupported action %q", env.Context.Action)
	}
	return env, action, nil
}

// BuildCallbackContext turns an inbound context into the one its callback
// carries.
//
// THE PARTICIPANT IDS DO NOT SWAP, AND THAT IS EASY TO GET WRONG. `bap_*` stays
// the buyer's and `bpp_*` stays ours in BOTH directions -- they identify the
// participants, not the sender and receiver. The `transaction_id` is carried
// through unchanged so the BAP can correlate; the `message_id` is NEW, because
// the callback is its own message.
func BuildCallbackContext(in Context, action InboundAction, newMessageID string, now time.Time) (Context, error) {
	callback, ok := callbackFor[action]
	if !ok {
		return Context{}, fmt.Errorf("beckn: no callback defined for action %q", action)
	}
	out := in
	out.Action = callback
	out.MessageID = newMessageID
	out.Timestamp = now.UTC().Format("2006-01-02T15:04:05.000Z")
	return out, nil
}

// BuildCallback assembles a complete callback envelope.
func BuildCallback(in Context, action InboundAction, newMessageID string, now time.Time, message any) ([]byte, error) {
	ctx, err := BuildCallbackContext(in, action, newMessageID, now)
	if err != nil {
		return nil, err
	}
	body, err := json.Marshal(message)
	if err != nil {
		return nil, fmt.Errorf("beckn: encoding callback message: %w", err)
	}
	return json.Marshal(Envelope{Context: ctx, Message: body})
}

// CancelMessage is the shape `cancel` carries, from the published example: an
// order id and a reason code, not an order document.
//
// Worth stating because it is the one action whose message is NOT an order:
// code written against `message.order` for every action would fail here, and
// would fail on the action that matters most -- a cancellation nobody processes
// is a rider dispatched to collect food for a customer who cancelled.
type CancelMessage struct {
	OrderID              string `json:"order_id"`
	CancellationReasonID string `json:"cancellation_reason_id"`
}

func ParseCancel(message json.RawMessage) (CancelMessage, error) {
	var m CancelMessage
	if err := json.Unmarshal(message, &m); err != nil {
		return CancelMessage{}, fmt.Errorf("beckn: decoding cancel message: %w", err)
	}
	if m.OrderID == "" {
		return CancelMessage{}, fmt.Errorf("beckn: cancel carries no order_id")
	}
	return m, nil
}

// StatusMessage is what `status` carries: an order id and nothing else.
type StatusMessage struct {
	OrderID string `json:"order_id"`
}

func ParseStatus(message json.RawMessage) (StatusMessage, error) {
	var m StatusMessage
	if err := json.Unmarshal(message, &m); err != nil {
		return StatusMessage{}, fmt.Errorf("beckn: decoding status message: %w", err)
	}
	if m.OrderID == "" {
		return StatusMessage{}, fmt.Errorf("beckn: status carries no order_id")
	}
	return m, nil
}

// ActionCarriesOrder reports whether an action's message is an order document.
//
// THE DISTINCTION IS NOT COSMETIC. `cancel` carries `{order_id,
// cancellation_reason_id}` and `status` carries `{order_id}` -- code written
// against `message.order` for every action fails on exactly those two, and one
// of them is a cancellation. A cancellation nobody processes is a rider
// dispatched for a customer who cancelled.
func ActionCarriesOrder(a InboundAction) bool {
	switch a {
	case ActionSelect, ActionInit, ActionConfirm, ActionUpdate:
		return true
	case ActionSearch, ActionStatus, ActionCancel:
		// search carries intent, not an order; status and cancel carry an id.
		return false
	}
	return false
}
